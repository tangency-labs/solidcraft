use super::*;
use serde_json::json;
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_viewport::StandardView;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}
fn sample() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"}));
    // A blind hole on the bottom, completely hidden from above.
    run(&mut s, "solid.cylinder", json!({"radius":3,"height":4,"base":[10,10,0],"operation":"cut","targets":["Plate"]}));
    s
}
fn camera(view: StandardView) -> Camera {
    let mut c = Camera { target: Vec3::new(20., 15., 5.), fov: 0., distance: 150., ..Default::default() };
    c.set_view(view);
    c
}
fn export(s: &mut Session, c: Camera, hidden: &str) -> Value {
    run(s, CMD, json!({"camera":c,"hidden":hidden}))
}
/// The LINE entities of an export's DXF.
fn lines(v: &Value) -> Vec<(Vec2, Vec2)> {
    solidcraft_io::read_dxf(v["dxf"].as_str().unwrap())
        .unwrap()
        .into_iter()
        .filter_map(|g| if let solidcraft_io::Geom2::Line(a, b) = g { Some((a, b)) } else { None })
        .collect()
}

#[test]
fn view_dxf_cancellation_at_each_stage_preserves_the_destination_and_document() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct CancelAt {
        stage: &'static str,
        cancelled: AtomicBool,
        reached: AtomicBool,
    }
    impl solidcraft_kernel::watch::Watch for CancelAt {
        fn cancelled(&self) -> bool {
            self.cancelled.load(Ordering::Relaxed)
        }
        fn progress(&self, stage: &'static str, done: usize, _total: usize) {
            if stage == self.stage && (done > 0 || stage == "dxf_write") {
                self.reached.store(true, Ordering::Relaxed);
                self.cancelled.store(true, Ordering::Relaxed);
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("solidcraft-dxf-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("drawing.dxf");
    let mut s = sample();
    let doc = s.doc.clone();
    let undo = s.undo.len();
    for stage in ["dxf_meshes", "dxf_index", "dxf_projection", "dxf_occlusion", "dxf_write"] {
        std::fs::write(&path, b"previous drawing").unwrap();
        let watch = Arc::new(CancelAt { stage, cancelled: AtomicBool::new(false), reached: AtomicBool::new(false) });
        let result = solidcraft_kernel::watch::watching(watch.clone(), || {
            s.execute(CMD, &json!({"camera":camera(StandardView::Top),"path":path.to_string_lossy()}))
        });
        assert!(result.unwrap_err().to_string().contains("cancelled"), "{stage}");
        assert!(watch.reached.load(Ordering::Relaxed), "{stage}");
        assert_eq!(std::fs::read(&path).unwrap(), b"previous drawing", "{stage}");
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.undo.len(), undo);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn view_dxf_blind_back_hole_is_omitted_or_dashed_and_front_hole_is_visible() {
    let mut s = sample();
    let undo = s.undo.len();
    let doc = s.doc.clone();
    let top = export(&mut s, camera(StandardView::Top), "omit");
    let dashed = export(&mut s, camera(StandardView::Top), "dashed");
    assert!(top["visible"].as_u64().unwrap() > 0);
    assert_eq!(top["hidden"], 0);
    assert!(dashed["hidden"].as_u64().unwrap() > 12, "{dashed}");
    assert_eq!(top["visible"], dashed["visible"]);
    let top_file = solidcraft_io::read_dxf_file(top["dxf"].as_str().unwrap()).unwrap();
    let dashed_file = solidcraft_io::read_dxf_file(dashed["dxf"].as_str().unwrap()).unwrap();
    assert!(!top_file.layers().iter().any(|layer| layer.name() == "HIDDEN"));
    let hidden = dashed_file.entities().iter().filter(|entity| dashed_file.layer(entity.layer()).unwrap().name() == "HIDDEN").collect::<Vec<_>>();
    assert!(!hidden.is_empty());
    assert!(hidden.iter().all(|entity| matches!(entity.geometry(), solidcraft_io::DxfShape::Polyline { points } if points.len() >= 2)));
    assert!(hidden.iter().all(|entity| dashed_file.layer(entity.layer()).unwrap().linetype() == solidcraft_io::DxfLinetype::Hidden));
    let bottom = export(&mut s, camera(StandardView::Bottom), "omit");
    assert!(bottom["visible"].as_u64().unwrap() > top["visible"].as_u64().unwrap());
    assert_eq!(s.undo.len(), undo);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
}

#[test]
fn view_dxf_respects_hidden_bodies_mm_and_camera_roll() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"}));
    run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[100,0,0],"body_name":"Other"}));
    run(&mut s, "browser.visibility", json!({"items":["Other"],"visible":false}));
    let mut c = camera(StandardView::Top);
    let drawing = export(&mut s, c, "omit");
    let size = |v: &Value| {
        let mut points = Vec::new();
        for g in solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap() {
            if let solidcraft_io::Geom2::Line(a, b) = g {
                points.extend([a, b]);
            }
        }
        let min = points.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |a, p| Vec2::new(a.x.min(p.x), a.y.min(p.y)));
        let max = points.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| Vec2::new(a.x.max(p.x), a.y.max(p.y)));
        max - min
    };
    let d = size(&drawing);
    assert!((d.x - 40.).abs() < 0.001 && (d.y - 30.).abs() < 0.001, "{d:?}");
    c.roll = std::f64::consts::FRAC_PI_2;
    let d = size(&export(&mut s, c, "omit"));
    assert!((d.x - 30.).abs() < 0.001 && (d.y - 40.).abs() < 0.001, "{d:?}");
    run(&mut s, "browser.visibility", json!({"items":["Plate"],"visible":false}));
    assert!(s.execute(CMD, &json!({"camera":c})).is_err());
}

#[test]
fn view_dxf_cylinders_have_side_silhouettes_and_perspective_is_supported() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20}));
    let mut c = camera(StandardView::Front);
    c.target = Vec3::new(0., 0., 10.);
    let v = export(&mut s, c, "omit");
    let lines = solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap();
    assert!(lines.iter().filter(|g| matches!(g,solidcraft_io::Geom2::Line(a,b) if (a.y-b.y).abs()>19.9)).count() >= 2);
    c.fov = 0.6;
    let v = export(&mut s, c, "dashed");
    assert_eq!(v["projection"], "perspective");
    assert!(v["visible"].as_u64().unwrap() > 0);
}

#[test]
fn view_dxf_camera_without_fov_is_orthographic() {
    // docs/mcp.md: `{target, yaw, pitch, distance, fov?}`; no `fov` is an orthographic view (#13112).
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":10,"height":5}));
    let v = run(&mut s, CMD, json!({"camera":{"target":[0,0,0],"yaw":0,"pitch":0,"distance":100}}));
    assert_eq!(v["projection"], "orthographic", "{v}");
    assert!(v["visible"].as_u64().unwrap() > 0, "{v}");
    // The input schema takes it too.
    let mut strict = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
    run(&mut strict, "solid.box", json!({"length":20,"width":10,"height":5}));
    assert_eq!(run(&mut strict, CMD, json!({"camera":{"target":[0,0,0],"yaw":0,"pitch":0,"distance":100}}))["projection"], "orthographic");
    // `distance` stays required.
    let err = s.execute(CMD, &json!({"camera":{"target":[0,0,0],"yaw":0,"pitch":0}})).unwrap_err().to_string();
    assert!(err.contains("invalid camera"), "{err}");
}

#[test]
fn view_dxf_rejects_invalid_options_and_near_eye_geometry_without_panicking() {
    let mut s = sample();
    for extra in [json!({"hidden":"all"}), json!({"hidden":true}), json!({"tolerance":0}), json!({"tolerance":"x"}), json!({"path":false})] {
        let mut p = extra;
        p["camera"] = json!(camera(StandardView::Top));
        assert!(s.execute(CMD, &p).is_err(), "{p}");
    }
    assert!(s.execute(CMD, &json!({})).is_err());
    let mut c = camera(StandardView::Top);
    c.fov = 0.5;
    c.distance = 1.;
    assert!(s.execute(CMD, &json!({"camera":c})).is_err());
    c.fov = 4.;
    assert!(s.execute(CMD, &json!({"camera":c})).is_err());
}

#[test]
fn view_dxf_keeps_every_occurrence_while_sharing_geometry() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Repeated part"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":8,"width":6,"height":3}));
    for x in [15., 30.] {
        run(&mut s, "occurrence.copy", json!({"component":component,"translate":[x,0,0]}));
    }
    run(&mut s, "component.activate", json!({"component":0}));
    let doc = s.doc.clone();
    let result = export(&mut s, camera(StandardView::Top), "dashed");
    assert_eq!(result["stats"]["bodies"], 3);
    assert_eq!(result["stats"]["shared_meshes"], 1);
    assert_eq!(result["stats"]["worker_limit"], mesh_lines::WORKERS);
    assert!(result["hidden"].as_u64().unwrap() > 0);
    let lines = solidcraft_io::read_dxf(result["dxf"].as_str().unwrap()).unwrap();
    let mut centers = std::collections::BTreeSet::new();
    for line in lines {
        if let solidcraft_io::Geom2::Line(a, b) = line {
            // Each translated copy has a visible top perimeter, including the offscreen one.
            if (a.y - b.y).abs() > 5.9 {
                centers.insert(((a.x + 20.) / 15.).floor() as i64);
            }
        }
    }
    assert!(centers.len() >= 3, "{centers:?}");
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    // The definition's first placement hidden: the others are still drawn from the shared mesh.
    let first = s.world_state().unwrap().bodies.first().map(|b| b.name.clone()).unwrap();
    run(&mut s, "browser.visibility", json!({"items":[first],"visible":false}));
    let result = export(&mut s, camera(StandardView::Top), "dashed");
    assert_eq!((result["stats"]["bodies"].clone(), result["stats"]["shared_meshes"].clone()), (json!(2), json!(1)), "{result}");
}

/// The lines themselves, worked by hand: a plain 40 x 30 x 10 box seen from the front is its
/// 40 x 10 outline. Every line lies along one of its four sides, they add up to its 100 mm
/// perimeter, and nothing is hidden.
#[test]
fn view_dxf_front_view_of_a_box_is_its_outline() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"}));
    let v = export(&mut s, camera(StandardView::Front), "dashed");
    assert_eq!(v["hidden"], 0, "{v}");
    let lines = lines(&v);
    let lo = lines.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |m, (a, b)| Vec2::new(m.x.min(a.x).min(b.x), m.y.min(a.y).min(b.y)));
    let hi = lines.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |m, (a, b)| Vec2::new(m.x.max(a.x).max(b.x), m.y.max(a.y).max(b.y)));
    assert!(((hi - lo) - Vec2::new(40.0, 10.0)).len() < 1e-6, "{lo:?} {hi:?}");
    let near = |x: f64, y: f64| (x - y).abs() < 1e-6;
    for (a, b) in &lines {
        let along = (near(a.x, b.x) && (near(a.x, lo.x) || near(a.x, hi.x))) || (near(a.y, b.y) && (near(a.y, lo.y) || near(a.y, hi.y)));
        assert!(along, "{a:?} {b:?} isn't along the outline");
    }
    let length: f64 = lines.iter().map(|(a, b)| a.dist(*b)).sum();
    assert!((length - 100.0).abs() < 1e-6, "{length}");
}

/// A plate with a through hole, seen from the iso view, has every line drawn once: no two lines
/// lie along each other. The faces round the rim are meshed apart, a hair off each other, and the
/// mesh sides between them lie on the rim's own edge: they are no silhouettes.
#[test]
fn view_dxf_draws_a_holes_rim_once() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius":3,"height":12,"base":[10,10,-1],"operation":"cut","targets":["Plate"]}));
    let lines = lines(&export(&mut s, camera(StandardView::Iso), "omit"));
    assert!(!lines.is_empty());
    for (i, &(a, b)) in lines.iter().enumerate() {
        let dir = (b - a) / a.dist(b);
        let (off, along) = (|p: Vec2| dir.cross(p - a).abs(), |p: Vec2| (p - a).dot(dir));
        for &(c, d) in &lines[i + 1..] {
            let overlap = along(c).max(along(d)).min(a.dist(b)) - along(c).min(along(d)).max(0.0);
            assert!(off(c).max(off(d)) > 1e-5 || overlap < 1e-4, "drawn twice: {a:?} {b:?} and {c:?} {d:?}");
        }
    }
}

/// A bend seen end on draws each of its arcs once. The plate is 2 mm sheet, bent up 60° or 90° on its x = 60
/// edge with an inner radius of its thickness, and seen from the front: every triangle of the bend's surface
/// is seen edge on, whatever facing the mesh's rounding gives it, so its rows draw no chords beside the arcs.
/// At 90° the thin triangles that join the bend's edge samples to its first rows lean past 1e-3; the
/// surface's own normals still see them edge on. The lines on each bend circle add up to its arc, less only
/// what sampling takes off.
#[test]
fn view_dxf_draws_a_bends_arcs_once() {
    for degrees in [60f64, 90.] {
        let mut s = Session::default();
        run(&mut s, "sheet.manage_rules", json!({"name": "Steel 2mm", "thickness": "2 mm", "bend_radius": "Thickness", "active": true}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 40]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
        run(&mut s, "sheet.flange", json!({"edges": [[60, 20, 2]], "height": 20, "angle": format!("{degrees} deg")}));
        let mut c = Camera { target: Vec3::new(30., 20., 5.), fov: 0., distance: 200., ..Default::default() };
        c.set_view(StandardView::Front);
        let lines = lines(&export(&mut s, c, "omit"));
        // The bend's axis, 4 mm (radius and thickness) above the plate's underside, where the flange's
        // outside meets the edge flanged: on the view, less the camera's target.
        let axis = Vec2::new(60. - 4. * (degrees / 2.).to_radians().tan() - 30., 4. - 5.);
        for radius in [2., 4.] {
            let on = |p: Vec2| (p.dist(axis) - radius).abs() < 1e-3;
            let drawn: f64 = lines.iter().filter(|&&(a, b)| on(a) && on(b)).map(|(a, b)| a.dist(*b)).sum();
            let arc = radius * degrees.to_radians();
            assert!(drawn > 0.99 * arc && drawn <= arc, "{degrees}°, radius {radius}: {drawn} mm of lines on a {arc} mm arc");
        }
    }
}

#[test]
fn view_dxf_exports_a_sketch_only_design_on_the_sketch_layer() {
    let mut s = Session::default();
    assert!(s.execute(CMD, &json!({"camera":camera(StandardView::Top)})).is_err());
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,20],"radius":8}));
    for finish in [false, true] {
        if finish {
            run(&mut s, "sketch.finish", json!({}));
        }
        let v = export(&mut s, camera(StandardView::Top), "omit");
        // Four rectangle sides and one circle, each its own entity.
        assert_eq!(v["sketch"], 5, "{v}");
        let geometry = solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap();
        let (mut sides, mut circle) = (Vec::new(), 0);
        for g in geometry {
            let solidcraft_io::Geom2::Line(a, b) = g else { panic!("{g:?}") };
            // The drawing is centred on the camera target (20, 15).
            let on_circle = |p: Vec2| ((p - Vec2::new(10., 5.)).len() - 8.).abs() < 0.02;
            if on_circle(a) && on_circle(b) {
                circle += 1;
            } else {
                sides.push((a.dist(b) * 1000.).round() as i64);
            }
        }
        sides.sort_unstable();
        assert_eq!(sides, [40_000, 40_000, 60_000, 60_000], "finish={finish}");
        assert!(circle > 16, "{circle}");
    }
}

/// A visible sketch's four straight sides, one circle and one quarter arc are six source
/// curves. In arcs mode those remain four LINEs, one CIRCLE and one ARC, on SKETCH;
/// finishing the sketch changes neither their geometry nor the requested representation.
#[test]
fn view_dxf_curves_arcs_preserve_visible_sketch_circles_and_arcs() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,20],"radius":8}));
    run(&mut s, "sketch.arc.center_point", json!({"center":[45,20],"start":[51,20],"end":[45,26]}));
    for finish in [false, true] {
        if finish {
            run(&mut s, "sketch.finish", json!({}));
        }
        let doc = s.doc.clone();
        let undo = s.undo.len();
        let top = camera(StandardView::Top);
        let default = export(&mut s, top, "omit");
        let as_lines = run(&mut s, CMD, json!({"camera":top,"curves":"lines"}));
        assert_eq!(default["dxf"], as_lines["dxf"], "the default remains lines; finish={finish}");
        let line_file = solidcraft_io::read_dxf_file(as_lines["dxf"].as_str().unwrap()).unwrap();
        assert_eq!(line_file.entities().len(), 6, "finish={finish}");
        assert!(line_file.entities().iter().all(|e| !matches!(e.geometry(), DxfShape::Circle { .. } | DxfShape::Arc { .. })));

        let as_arcs = run(&mut s, CMD, json!({"camera":top,"curves":"arcs"}));
        assert_eq!(as_arcs["sketch"], 6, "finish={finish}");
        let file = solidcraft_io::read_dxf_file(as_arcs["dxf"].as_str().unwrap()).unwrap();
        assert_eq!(file.entities().len(), 6, "finish={finish}");
        assert!(file.entities().iter().all(|e| file.layer(e.layer()).unwrap().name() == "SKETCH"));
        let kinds = [
            file.entities().iter().filter(|e| matches!(e.geometry(), DxfShape::Line(..))).count(),
            file.entities().iter().filter(|e| matches!(e.geometry(), DxfShape::Circle { .. })).count(),
            file.entities().iter().filter(|e| matches!(e.geometry(), DxfShape::Arc { .. })).count(),
        ];
        assert_eq!(kinds, [4, 1, 1], "four sides, a circle and a quarter arc; finish={finish}");
        assert_eq!(s.undo.len(), undo, "export is not an edit; finish={finish}");
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    }
}

#[test]
fn view_dxf_hides_finished_sketches_behind_bodies_but_draws_the_active_sketch_on_top() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10}));
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,0,-5],"x_dir":[1,0,0],"y_dir":[0,1,0]}}));
    run(&mut s, "sketch.line", json!({"points":[[-20,15],[60,15]]}));
    // The box's edges run along y = 0 and y = 30; only the sketch line lies on y = 15 (0 in the
    // drawing, which is centred on the camera target; the top view is tilted a hair).
    let length = |v: &Value| {
        let lines = solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap();
        lines
            .iter()
            .filter_map(|g| if let solidcraft_io::Geom2::Line(a, b) = g { Some((*a, *b)) } else { None })
            .filter(|(a, b)| a.y.abs() < 1e-3 && b.y.abs() < 1e-3)
            .map(|(a, b)| a.dist(b))
            .sum::<f64>()
    };
    let active = export(&mut s, camera(StandardView::Top), "omit");
    assert_eq!(active["sketch"], 1, "{active}");
    let active_file = solidcraft_io::read_dxf_file(active["dxf"].as_str().unwrap()).unwrap();
    assert!(
        (length(&active) - 80.).abs() < 1e-3
            && active_file.entities().iter().any(|entity| active_file.layer(entity.layer()).unwrap().name() == "SKETCH")
    );
    run(&mut s, "sketch.finish", json!({}));
    let finished = export(&mut s, camera(StandardView::Top), "omit");
    assert_eq!(finished["sketch"], 2, "{finished}");
    assert!((length(&finished) - 40.).abs() < 1e-3, "{}", length(&finished));
    let dashed = export(&mut s, camera(StandardView::Top), "dashed");
    assert_eq!(dashed["sketch"], 2);
    assert!((length(&dashed) - 80.).abs() < 1e-3, "{}", length(&dashed));
}

/// A shown sketch under an extrusion's outline is drawn once, as the body's black line, not again
/// in blue on SKETCH: a cutting service reads the blue copy as a marking line (#19507).
#[test]
fn view_dxf_draws_a_shown_sketch_under_a_bodys_outline_once() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Panel"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Panel","distance":2,"body_name":"Panel"}));
    let sketch = s.doc.features[0].id;
    let sides = |v: &Value| {
        let mut sides: Vec<i64> = lines(v).iter().map(|(a, b)| (a.dist(*b) * 1000.).round() as i64).collect();
        sides.sort_unstable();
        sides
    };
    for hidden in ["omit", "dashed"] {
        // The body's own lines, with the sketch hidden. The top view is tilted a hair, so the
        // panel's bottom edge on one side can show beside the top one: that line is the body's.
        s.visibility.shown_sketches.retain(|id| *id != sketch);
        let body = sides(&export(&mut s, camera(StandardView::Top), hidden));
        assert!(
            body.iter().filter(|l| **l == 30_000).count() >= 2 && body.iter().filter(|l| **l == 40_000).count() >= 2,
            "hidden={hidden}: {body:?}"
        );
        s.visibility.shown_sketches.push(sketch);
        assert!(crate::view::sketch_shown(&s, sketch));
        let v = export(&mut s, camera(StandardView::Top), hidden);
        assert_eq!(v["sketch"], 0, "hidden={hidden}: {v}");
        let file = solidcraft_io::read_dxf_file(v["dxf"].as_str().unwrap()).unwrap();
        assert!(file.entities().iter().all(|e| file.layer(e.layer()).unwrap().name() != "SKETCH"), "hidden={hidden}");
        // Showing the sketch adds no line: the drawing is the body's lines alone.
        assert_eq!(sides(&v), body, "hidden={hidden}");
    }
    // The sketch past the body keeps its own line: a second rectangle beside the panel.
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Beside"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[50,0],"p1":[60,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let v = export(&mut s, camera(StandardView::Top), "omit");
    assert!(v["sketch"].as_u64().is_some_and(|n| n > 0), "{v}");
}

#[test]
fn view_dxf_zero_origin_puts_the_model_origin_at_the_drawing_zero() {
    let mut s = sample();
    let bounds = |v: &Value| {
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for g in solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap() {
            let solidcraft_io::Geom2::Line(a, b) = g else { panic!("{g:?}") };
            for p in [a, b] {
                (lo, hi) = (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)));
            }
        }
        (lo, hi)
    };
    let near = |a: Vec2, x: f64, y: f64| (a.x - x).abs() < 1e-3 && (a.y - y).abs() < 1e-3;
    // By default the drawing is centred on the camera target (20, 15), whatever the model origin.
    let (lo, hi) = bounds(&export(&mut s, camera(StandardView::Top), "omit"));
    assert!(near(lo, -20., -15.) && near(hi, 20., 15.), "{lo:?} {hi:?}");
    // `zero: origin` is the same drawing with the model origin, the plate's corner, at 0,0.
    for zero in [None, Some("target")] {
        let mut p = json!({"camera": camera(StandardView::Top)});
        if let Some(z) = zero {
            p["zero"] = json!(z);
        }
        let (lo, _) = bounds(&run(&mut s, CMD, p));
        assert!(near(lo, -20., -15.), "{zero:?}");
    }
    let v = run(&mut s, CMD, json!({"camera": camera(StandardView::Top), "zero": "origin"}));
    let (lo, hi) = bounds(&v);
    assert!(near(lo, 0., 0.) && near(hi, 40., 30.), "{lo:?} {hi:?}");
    let err = s.execute(CMD, &json!({"camera": camera(StandardView::Top), "zero": "corner"})).unwrap_err().to_string();
    assert!(err.contains("zero must be target or origin"), "{err}");
}

/// The lines of a regular polygon with `n` sides round `c`, from angle `from` to `to`.
/// Over a whole turn the last line returns to the first point and the ring is closed.
fn ring(c: Vec2, r: f64, n: usize, from: f64, to: f64) -> Vec<(Vec2, Vec2)> {
    let at = |i: usize| {
        let t = from + (to - from) * i as f64 / n as f64;
        Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())
    };
    (0..n).map(|i| (at(i), at(i + 1))).collect()
}

/// Top has right=X and up=(0,sin(pitch),cos(pitch)). All source curves lie at z=0,
/// so their projected Y is (worldY-15)*sin(pitch)-5*cos(pitch), including the existing
/// pole clamp. The projected radius anisotropy is at most R*(1-cos(1e-6)), below
/// 4e-12 mm for R<=8. Expectations come from this frame, not the exported output.
#[test]
fn view_dxf_sketch_arcs_keep_projected_centers_radii_angles_and_rectangle_corners() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,20],"radius":8}));
    run(&mut s, "sketch.arc.center_point", json!({"center":[45,20],"start":[51,20],"end":[45,26]}));
    let top = camera(StandardView::Top);
    let (sin, cos) = top.pitch.sin_cos();
    let projected_y = |world_y: f64| (world_y - 15.) * sin - 5. * cos;
    let center_y = projected_y(20.);
    let near = |a: Vec2, b: Vec2| a.dist(b) < 1e-9;
    let corners =
        [Vec2::new(-20., projected_y(0.)), Vec2::new(40., projected_y(0.)), Vec2::new(40., projected_y(40.)), Vec2::new(-20., projected_y(40.))];
    let sides = [(corners[0], corners[1]), (corners[1], corners[2]), (corners[2], corners[3]), (corners[3], corners[0])];
    for finish in [false, true] {
        if finish {
            run(&mut s, "sketch.finish", json!({}));
        }
        let v = run(&mut s, CMD, json!({"camera":top,"curves":"arcs"}));
        let file = solidcraft_io::read_dxf_file(v["dxf"].as_str().unwrap()).unwrap();
        let (mut found_sides, mut circles, mut arcs) = (Vec::new(), 0, 0);
        for entity in file.entities() {
            match entity.geometry() {
                DxfShape::Line(a, b) => {
                    let side = sides.iter().position(|&(p, q)| (near(*a, p) && near(*b, q)) || (near(*a, q) && near(*b, p)));
                    found_sides.push(side.expect("one original rectangle side"));
                }
                DxfShape::Circle { c, r } => {
                    circles += 1;
                    assert!(near(*c, Vec2::new(10., center_y)) && (*r - 8.).abs() < 1e-9, "finish={finish}: {c:?}, {r}");
                }
                DxfShape::Arc { c, r, a0, a1 } => {
                    arcs += 1;
                    assert!(near(*c, Vec2::new(25., center_y)) && (*r - 6.).abs() < 1e-9, "finish={finish}: {c:?}, {r}");
                    assert!(a0.abs() < 1e-6 && (*a1 - 90.).abs() < 1e-6, "finish={finish}: {a0}..{a1}");
                }
                shape => panic!("unexpected sketch representation: {shape:?}"),
            }
        }
        found_sides.sort_unstable();
        assert_eq!(found_sides, [0, 1, 2, 3], "finish={finish}");
        assert_eq!((circles, arcs), (1, 1), "finish={finish}");
    }
}

#[test]
fn view_dxf_sketch_arc_planning_keeps_original_fallback_points_and_hidden_policy() {
    let points = |segments: Vec<(Vec2, Vec2)>| {
        let mut points: Vec<_> = segments.iter().map(|(a, _)| *a).collect();
        points.push(segments.last().unwrap().1);
        points
    };
    let hidden_ring = points(ring(Vec2::new(40., 0.), 10., 64, 0., std::f64::consts::TAU));
    let mut mixed = points(ring(Vec2::ZERO, 10., 20, 0., std::f64::consts::FRAC_PI_2));
    mixed.push(Vec2::new(20., 20.));
    let sources = [
        (vec![Vec2::new(-0.0, 0.), Vec2::new(1e-14, -0.0)], false),
        (vec![Vec2::ZERO, Vec2::ZERO], false),
        (vec![Vec2::ZERO, Vec2::X, Vec2::new(1., 1.), Vec2::Y, Vec2::ZERO], false),
        (mixed, false),
        (hidden_ring, true),
    ];
    let bits = |points: &[Vec2]| points.iter().map(|p| [p.x.to_bits(), p.y.to_bits()]).collect::<Vec<_>>();
    for dashed in [false, true] {
        let paths: Vec<_> = sources.iter().map(|(p, hidden)| mesh_lines::Path { points: p.clone(), closed: false, hidden: *hidden }).collect();
        let count = paths.iter().map(|p| p.points.len()).sum();
        let plan = PlannedViewArcs::sketch(&paths, 0.01, &Stage::new("dxf_write", 1)).unwrap();
        assert!(plan.sketch_shapes.is_empty() && plan.arc_source_points.is_empty(), "no full visible path is one circle/arc");
        let mut file = DxfFile::for_current_view_arc_export_with_sketch_arcs(0, paths.len(), count, &plan.arc_source_points).unwrap();
        let sketch = file.declare_layer("SKETCH", 5, DxfLinetype::Continuous, None).unwrap();
        let hidden = dashed.then(|| file.declare_layer("HIDDEN", 8, DxfLinetype::Hidden, None).unwrap());
        assert_eq!(sketch_entities(paths, plan.sketch_shapes, dashed, &mut file, hidden, Some(sketch)).unwrap(), 4);
        let expected: Vec<_> = sources.iter().filter(|(_, hidden)| dashed || !hidden).collect();
        assert_eq!(file.entities().len(), expected.len());
        for (entity, (original, hidden)) in file.entities().iter().zip(expected) {
            assert_eq!(file.layer(entity.layer()).unwrap().name(), if *hidden { "HIDDEN" } else { "SKETCH" });
            let retained = match entity.geometry() {
                DxfShape::Line(a, b) => vec![*a, *b],
                DxfShape::Polyline { points } => points.clone(),
                shape => panic!("fallback changed representation: {shape:?}"),
            };
            assert_eq!(bits(&retained), bits(original), "fallback does not snap, deduplicate or reorder source points");
        }
        solidcraft_io::write_dxf_file(&file).unwrap();
    }
}

#[test]
fn view_dxf_sketch_arc_planning_does_not_join_separate_curves_into_one_circle() {
    let mut paths = Vec::new();
    for (from, to) in [(0., std::f64::consts::PI), (std::f64::consts::PI, std::f64::consts::TAU)] {
        let segments = ring(Vec2::ZERO, 10., 40, from, to);
        let mut points: Vec<_> = segments.iter().map(|(a, _)| *a).collect();
        points.push(segments.last().unwrap().1);
        paths.push(mesh_lines::Path { points, closed: false, hidden: false });
    }
    let plan = PlannedViewArcs::sketch(&paths, 0.01, &Stage::new("dxf_write", 1)).unwrap();
    assert_eq!(plan.arc_source_points, [41, 41]);
    assert_eq!(plan.sketch_shapes.len(), 2);
    assert!(plan.sketch_shapes.iter().all(|(_, shape)| matches!(shape, DxfShape::Arc { .. })));
    let mut file = DxfFile::for_current_view_arc_export_with_sketch_arcs(0, 2, 82, &plan.arc_source_points).unwrap();
    let layer = file.declare_layer("SKETCH", 5, DxfLinetype::Continuous, None).unwrap();
    assert_eq!(sketch_entities(paths, plan.sketch_shapes, false, &mut file, None, Some(layer)).unwrap(), 2);
    solidcraft_io::write_dxf_file(&file).unwrap();
}

#[test]
fn view_dxf_open_sketch_arc_far_from_origin_does_not_close_its_gap() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.arc.center_point", json!({"center":[1_000_000,1_000_000],"start":[1_000_008,1_000_000],"sweep":359.995}));
    let segments = s.doc.sketch(s.active_sketch.unwrap()).unwrap().segs(0);
    let [solidcore_record::Seg2::Arc { sweep, .. }] = segments.as_slice() else { panic!("{segments:?}") };
    assert!((sweep.to_degrees() - 359.995).abs() < 1e-6, "the actual command must retain the open arc: {segments:?}");
    let mut top = camera(StandardView::Top);
    top.target = Vec3::ZERO;
    let result = run(&mut s, CMD, json!({"camera":top,"curves":"arcs","tolerance":0.0001}));
    let file = solidcraft_io::read_dxf_file(result["dxf"].as_str().unwrap()).unwrap();
    assert_eq!(file.entities().len(), 1);
    let entity = &file.entities()[0];
    assert_eq!(file.layer(entity.layer()).unwrap().name(), "SKETCH");
    let DxfShape::Arc { a0, a1, .. } = entity.geometry() else {
        panic!("an open sketch arc must not become a closed circle: {:?}", entity.geometry())
    };
    // The omitted0.005 degree arc spans0.000698 mm atR8, larger than the requested0.0001 mm.
    assert!(((a1 - a0).rem_euclid(360.) - 359.995).abs() < 1e-6, "{a0}..{a1}");
}

#[test]
fn view_dxf_true_sketch_circle_at_origin_stays_closed_with_zero_camera_target() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":8}));
    let mut top = camera(StandardView::Top);
    top.target = Vec3::ZERO;
    let result = run(&mut s, CMD, json!({"camera":top,"curves":"arcs","tolerance":0.0001}));
    let file = solidcraft_io::read_dxf_file(result["dxf"].as_str().unwrap()).unwrap();
    assert_eq!(file.entities().len(), 1);
    let entity = &file.entities()[0];
    assert_eq!(file.layer(entity.layer()).unwrap().name(), "SKETCH");
    let DxfShape::Circle { c, r } = entity.geometry() else { panic!("a true source circle must stay closed: {:?}", entity.geometry()) };
    // The source is centered on the camera target; Top's pole clamp changes R8 by less than4e-12 mm.
    assert!(c.len() < 1e-9 && (r - 8.).abs() < 1e-9, "center={c:?}, radius={r}");
}

#[test]
fn view_dxf_clockwise_open_arc_preserves_sweep_when_camera_target_changes() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.arc.center_point", json!({"center":[1_000_000,1_000_000],"start":[1_000_008,1_000_000],"sweep":-359.995}));
    for target in [Vec3::ZERO, Vec3::new(1_000_000., 1_000_000., 0.)] {
        let mut top = camera(StandardView::Top);
        top.target = target;
        let result = run(&mut s, CMD, json!({"camera":top,"curves":"arcs","tolerance":0.0001}));
        let file = solidcraft_io::read_dxf_file(result["dxf"].as_str().unwrap()).unwrap();
        assert_eq!(file.entities().len(), 1, "target={target:?}");
        let entity = &file.entities()[0];
        assert_eq!(file.layer(entity.layer()).unwrap().name(), "SKETCH");
        let DxfShape::Arc { r, a0, a1, .. } = entity.geometry() else {
            panic!("the open clockwise source must remain an ARC: {:?}; target={target:?}", entity.geometry())
        };
        // DXF writes arcs counter-clockwise, exchanging the clockwise source's endpoints.
        // Moving the camera target changes coordinates, not radius or the359.995 degree sweep.
        assert!((r - 8.).abs() < 1e-9, "radius={r}; target={target:?}");
        assert!(((a1 - a0).rem_euclid(360.) - 359.995).abs() < 1e-6, "{a0}..{a1}; target={target:?}");
    }
}

#[test]
fn view_dxf_tilted_sketch_circle_keeps_original_polyline_within_requested_tolerance() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,20],"radius":8}));
    let mut cam = camera(StandardView::Top);
    cam.pitch = 89.0_f64.to_radians();
    // The projected ellipse has radii 8 and 8*sin(89 degrees). Its approximately
    // 0.000609 mm circle-fit error exceeds this 0.0001 mm export tolerance.
    let as_lines = run(&mut s, CMD, json!({"camera":cam,"curves":"lines","tolerance":0.0001}));
    let as_arcs = run(&mut s, CMD, json!({"camera":cam,"curves":"arcs","tolerance":0.0001}));
    let file = solidcraft_io::read_dxf_file(as_arcs["dxf"].as_str().unwrap()).unwrap();
    assert_eq!(file.entities().len(), 1);
    let entity = &file.entities()[0];
    assert_eq!(file.layer(entity.layer()).unwrap().name(), "SKETCH");
    let DxfShape::Polyline { points } = entity.geometry() else { panic!("{:?}", entity.geometry()) };
    assert!(points.len() > 2);
    // Falling back must retain all original projected coordinates and their serialized bytes.
    assert_eq!(as_arcs["dxf"].as_str().unwrap().as_bytes(), as_lines["dxf"].as_str().unwrap().as_bytes());
}

#[test]
fn view_dxf_arcs_turn_a_tessellated_circle_into_a_circle_and_keep_polygons_as_lines() {
    let tau = std::f64::consts::TAU;
    let c = Vec2::new(3., -2.);
    // A fine ring is a circle; coarse rings (an octagon, even a 32-gon) sag too far from the
    // circle for the 0.01 mm the drawing was made with, so they are polygons and stay lines.
    let fine = arcs::recognise(ring(c, 10., 64, 0., tau), 0.01);
    assert_eq!(fine.len(), 1, "{fine:?}");
    let DxfShape::Circle { c: found, r } = &fine[0] else { panic!("{fine:?}") };
    assert!((*found - c).len() < 1e-9 && (r - 10.).abs() < 1e-9, "{fine:?}");
    for sides in [6, 8, 32] {
        let polygon = arcs::recognise(ring(c, 10., sides, 0., tau), 0.01);
        assert_eq!(polygon.len(), sides, "{sides} sides");
        assert!(polygon.iter().all(|shape| matches!(shape, DxfShape::Line(..))));
    }
}

#[test]
fn view_dxf_arcs_turn_a_run_of_lines_into_an_arc_that_runs_counter_clockwise() {
    let c = Vec2::new(0., 0.);
    let quarter = std::f64::consts::FRAC_PI_2;
    // The run goes round clockwise, from 90 to 0 degrees: the DXF arc names 0 first, then 90.
    let run = arcs::recognise(ring(c, 10., 40, quarter, 0.), 0.01);
    assert_eq!(run.len(), 1, "{run:?}");
    let DxfShape::Arc { r, a0, a1, .. } = &run[0] else { panic!("{run:?}") };
    assert!((r - 10.).abs() < 1e-9 && a0.abs() < 1e-6 && (a1 - 90.).abs() < 1e-6, "{run:?}");
    // A straight run, and a few lines, are never an arc.
    let straight: Vec<(Vec2, Vec2)> = (0..10).map(|i| (Vec2::new(i as f64, 0.), Vec2::new(i as f64 + 1., 0.))).collect();
    assert_eq!(arcs::recognise(straight, 0.01).len(), 10);
    assert_eq!(arcs::recognise(ring(c, 10., 3, 0., quarter), 0.01).len(), 3);
}

#[test]
fn view_dxf_curves_arcs_write_a_round_hole_as_one_circle_and_the_default_still_writes_lines() {
    let mut s = sample();
    // The sample's blind hole is hidden from above; this one goes right through.
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":10,"base":[20,15,0],"operation":"cut","targets":["Plate"]}));
    let top = camera(StandardView::Top);
    let geometry = |v: &Value| solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap();
    let as_lines = geometry(&run(&mut s, CMD, json!({"camera": top})));
    assert!(as_lines.iter().all(|g| matches!(g, solidcraft_io::Geom2::Line(..))), "the default is unchanged");
    let as_arcs = geometry(&run(&mut s, CMD, json!({"camera": top, "curves": "arcs"})));
    let circles: Vec<_> = as_arcs.iter().filter_map(|g| if let solidcraft_io::Geom2::Circle(c, r) = g { Some((*c, *r)) } else { None }).collect();
    // The hole is centred on the camera target (20, 15), so it is the circle at 0,0.
    assert_eq!(circles.len(), 1, "{as_arcs:?}");
    assert!(circles[0].0.len() < 1e-3 && (circles[0].1 - 5.).abs() < 1e-4, "{circles:?}");
    // The plate's four sides stay four lines, and the hole's ring no longer adds dozens of lines.
    let lines = as_arcs.iter().filter(|g| matches!(g, solidcraft_io::Geom2::Line(..))).count();
    assert_eq!(lines, 4, "{as_arcs:?}");
    assert!(as_lines.len() > 40);
    let err = s.execute(CMD, &json!({"camera": top, "curves": "splines"})).unwrap_err().to_string();
    assert!(err.contains("curves must be lines or arcs"), "{err}");
}

/// With `curves: "arcs"` and dashed hidden lines, each kind of entity sits on its own layer: the
/// circle and the plate's sides on VISIBLE, the blind hole's dashed runs on HIDDEN, and the hidden
/// runs come first in the file, as they always have (the order is part of the file's bytes).
#[test]
fn view_dxf_curves_arcs_with_dashed_hidden_lines_keep_their_layers_and_order() {
    let mut s = sample();
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":10,"base":[20,15,0],"operation":"cut","targets":["Plate"]}));
    let v = run(&mut s, CMD, json!({"camera": camera(StandardView::Top), "curves": "arcs", "hidden": "dashed"}));
    let file = solidcraft_io::read_dxf_file(v["dxf"].as_str().unwrap()).unwrap();
    let on = |entity: &solidcraft_io::DxfFileEntity<DxfShape>| file.layer(entity.layer()).unwrap().name().to_string();
    let layers: Vec<String> = file.entities().iter().map(on).collect();
    let first_visible = layers.iter().position(|name| name == "VISIBLE").unwrap();
    assert!(first_visible > 0 && layers[..first_visible].iter().all(|name| name == "HIDDEN"), "{layers:?}");
    assert!(layers[first_visible..].iter().all(|name| name == "VISIBLE"), "{layers:?}");
    let kinds = |layer: &str| -> Vec<&DxfShape> { file.entities().iter().filter(|e| on(e) == layer).map(|e| e.geometry()).collect() };
    assert!(kinds("HIDDEN").iter().all(|shape| matches!(shape, DxfShape::Polyline { .. })));
    assert_eq!(kinds("VISIBLE").iter().filter(|shape| matches!(shape, DxfShape::Circle { .. })).count(), 1);
    assert_eq!(kinds("VISIBLE").iter().filter(|shape| matches!(shape, DxfShape::Line(..))).count(), 4);
}

// Frozen pre-X2 assembly recipe, with its real shared projection and legacy
// byte-baseline seam. This is not another production writer or parser.
fn legacy_view_projection_bytes(s: &Session, cam: &Camera, dashed: bool) -> String {
    use solidcraft_io::test_support::{DxfEntity, DxfLayer, legacy_dxf_baseline};
    let meshes =
        mesh_lines::prepare(s, &s.world_state().unwrap(), CMD, 0.01, mesh_lines::Ask::default(), |b| s.body_visible(&b.name), refused).unwrap();
    let parts = sketch::parts(s, 0.01).unwrap();
    let mut drawing = mesh_lines::drawing(&meshes, &parts, &View::from_camera(cam)).unwrap();
    drawing.sketch = mesh_lines::sketch_off_lines(std::mem::take(&mut drawing.sketch), &drawing.lines, 0.02 + drawing.eps);
    let mut entities = Vec::new();
    let mut hidden = Vec::new();
    for line in drawing.lines {
        if line.hidden {
            if dashed {
                hidden.push((line.a, line.b));
            }
        } else {
            entities.push(DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(line.a, line.b) });
        }
    }
    for path in connect(hidden) {
        entities.push(DxfEntity { layer: "HIDDEN".into(), shape: DxfShape::Polyline { points: path.points } });
    }
    let mut shown = 0;
    for path in drawing.sketch.into_iter().filter(|path| dashed || !path.hidden) {
        let layer = if path.hidden { "HIDDEN" } else { "SKETCH" };
        shown += usize::from(!path.hidden);
        let shape = match <[Vec2; 2]>::try_from(path.points) {
            Ok([a, b]) => DxfShape::Line(a, b),
            Err(points) => DxfShape::Polyline { points },
        };
        entities.push(DxfEntity { layer: layer.into(), shape });
    }
    let mut layers = vec![DxfLayer::new("VISIBLE", 7)];
    if dashed {
        layers.push(DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", 8) });
    }
    if shown > 0 {
        layers.push(DxfLayer::new("SKETCH", 5));
    }
    legacy_dxf_baseline(&layers, &entities).unwrap()
}

#[test]
fn view_dxf_owned_file_keeps_current_header_tables_and_legacy_projection_bytes() {
    let mut s = sample();
    let cam = camera(StandardView::Top);
    for hidden in ["omit", "dashed"] {
        let expected = legacy_view_projection_bytes(&s, &cam, hidden == "dashed");
        let doc = s.doc.clone();
        let actual = export(&mut s, cam, hidden);
        assert_eq!(actual["dxf"].as_str().unwrap().as_bytes(), expected.as_bytes());
        assert!(expected.contains("$DWGCODEPAGE\n") && expected.contains("ACAD_GROUP\n"));
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    }
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,0,-5],"x_dir":[1,0,0],"y_dir":[0,1,0]}}));
    run(&mut s, "sketch.line", json!({"points":[[-20,15],[60,15]]}));
    for finish in [false, true] {
        if finish {
            run(&mut s, "sketch.finish", json!({}));
        }
        for hidden in ["omit", "dashed"] {
            let expected = legacy_view_projection_bytes(&s, &cam, hidden == "dashed");
            let actual = export(&mut s, cam, hidden);
            assert_eq!(actual["dxf"].as_str().unwrap().as_bytes(), expected.as_bytes(), "finish={finish}, hidden={hidden}");
        }
    }
}

#[test]
fn hidden_line_runs_keep_endpoint_order_and_disconnected_runs() {
    let (a, b, c) = (Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(2., 0.));
    let (d, e) = (Vec2::new(10., 0.), Vec2::new(11., 0.));
    let paths = connect(vec![(a, b), (c, b), (d, e)]);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].points, [a, b, c]);
    assert_eq!(paths[1].points, [d, e]);
    assert!(paths.iter().all(|path| path.hidden));
}

#[test]
fn hidden_line_junctions_stop_runs_and_cycles_keep_the_repeated_endpoint() {
    let (a, b, c, d) = (Vec2::new(0., 0.), Vec2::new(1., 0.), Vec2::new(1., 1.), Vec2::new(0., 1.));
    let junction = connect(vec![(d, a), (a, b), (a, c)]);
    assert_eq!(junction.len(), 3);
    assert_eq!(junction[0].points, [d, a]);
    assert_eq!(junction[1].points, [a, b]);
    assert_eq!(junction[2].points, [a, c]);
    let cycle = connect(vec![(a, b), (b, c), (c, d), (d, a)]);
    assert_eq!(cycle.len(), 1);
    assert_eq!(cycle[0].points, [a, b, c, d, a]);
    assert!(junction.iter().chain(&cycle).all(|path| path.hidden));
}

#[test]
fn view_dxf_front_view_of_a_tray_with_round_corner_reliefs_is_exported_not_refused() {
    // #16814: the faces beside round corner reliefs are swept along the front view, and their
    // mesh triangles tilt either way. Each of their mesh edges counted as a silhouette, and
    // the hidden-line work budget ran out on the exact front view (yaw 0).
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name":"Steel 1.2mm","thickness":"1.2 mm","bend_radius":"1.2 mm","k_factor":"0.42","active":true}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base"}));
    run(&mut s, "sketch.rectangle.center", json!({"center":[0,0],"corner":[100,90]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch":"Base","body_name":"Chassis"}));
    run(&mut s, "sheet.flange", json!({"edges":[[0,-90,1.2],[0,90,1.2]],"height":70}));
    run(
        &mut s,
        "sheet.flange",
        json!({"edges":[[-100,0,1.2],[100,0,1.2],[-100,-88.8,35],[100,-88.8,35],[-100,88.8,35],[100,88.8,35]],"height":12,"corner_relief_shape":"round","corner_relief":3}),
    );
    let v = run(&mut s, CMD, json!({"hidden":"dashed","camera":{"target":[0,0,0],"yaw":0,"pitch":0,"distance":2000,"fov":0.0}}));
    // The tray's outline: the 200 mm base, the 70 mm walls (from z = 0 to 70).
    let mut points = Vec::new();
    for g in solidcraft_io::read_dxf(v["dxf"].as_str().unwrap()).unwrap() {
        if let solidcraft_io::Geom2::Line(a, b) = g {
            points.extend([a, b]);
        }
    }
    let min = points.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |a, p| Vec2::new(a.x.min(p.x), a.y.min(p.y)));
    let max = points.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| Vec2::new(a.x.max(p.x), a.y.max(p.y)));
    assert!(min.dist(Vec2::new(-100., 0.)) < 1e-6 && max.dist(Vec2::new(100., 70.)) < 1e-6, "{min:?} {max:?}");
    // Only the B-rep edges and real silhouettes are drawn: 1134 segments, 0.5 million of the
    // 200 million work steps. Before, 76680 segments ran the budget out.
    let stats = &v["stats"];
    assert!(stats["segments"].as_u64().unwrap() < 2_000, "{stats}");
    assert!(stats["occlusion_work"].as_u64().unwrap() < 2_000_000, "{stats}");
    assert!(v["visible"].as_u64().unwrap() > 0 && v["hidden"].as_u64().unwrap() > 0, "{v}");
}

/// #22913: with Section Analysis on, the view is exported as the section cuts it, not refused: the
/// side each plane's normal points to is left out, and the cut faces' outlines are drawn. A
/// `section` of its own stands in for the session's; `[]` draws the bodies whole.
#[test]
fn view_dxf_exports_the_view_as_the_section_cuts_it() {
    let mut s = sample();
    let top = camera(StandardView::Top);
    let width = |v: &Value| {
        let xs: Vec<f64> = lines(v).iter().flat_map(|(a, b)| [a.x, b.x]).collect();
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - xs.iter().copied().fold(f64::INFINITY, f64::min)
    };
    assert!((width(&export(&mut s, top, "omit")) - 40.).abs() < 1e-3);
    // Section Analysis on the YZ plane at x = 20: half the plate is left.
    run(&mut s, "inspect.section", json!({"plane":"YZ","at":20}));
    let cut = export(&mut s, top, "omit");
    assert!((width(&cut) - 20.).abs() < 1e-3, "{}", width(&cut));
    assert!((width(&run(&mut s, CMD, json!({"camera":top,"section":[]}))) - 40.).abs() < 1e-3);
    run(&mut s, "inspect.section", json!({"clear":true}));
    // Cut across y at 12, the side toward -y left out: from above, 40 by 18 of the plate is left.
    let height = |v: &Value| {
        let ys: Vec<f64> = lines(v).iter().flat_map(|(a, b)| [a.y, b.y]).collect();
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - ys.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let across_y = run(&mut s, CMD, json!({"camera":top,"section":[{"origin":[0,12,0],"normal":[0,-1,0]}]}));
    assert!((width(&across_y) - 40.).abs() < 1e-3 && (height(&across_y) - 18.).abs() < 1e-3, "{} by {}", width(&across_y), height(&across_y));
    // From the front, the blind hole is hidden in the plate; cut through its axis, its section shows:
    // a line across the hole's top, 4 mm up, from x = 7 to 13 (the view is centred on (20, 15, 5)).
    let front = camera(StandardView::Front);
    let across_the_hole = |v: &Value| {
        lines(v)
            .iter()
            .any(|(a, b)| (a.y + 1.).abs() < 1e-3 && (b.y + 1.).abs() < 1e-3 && (a.x.min(b.x) + 13.).abs() < 1e-3 && (a.x.max(b.x) + 7.).abs() < 1e-3)
    };
    assert!(!across_the_hole(&export(&mut s, front, "omit")));
    let through_the_axis = json!([{"origin":[0,10,0],"normal":[0,-1,0]}]);
    let section = run(&mut s, CMD, json!({"camera":front,"section":through_the_axis}));
    assert!(across_the_hole(&section), "{:?}", lines(&section));
    // Bad planes are refused, and so is a section that leaves nothing.
    let refused = |s: &mut Session, section: Value| s.execute(CMD, &json!({"camera":front,"section":section})).unwrap_err().to_string();
    assert!(refused(&mut s, json!([{"origin":[0,0,0],"normal":[0,0,0]}])).contains("normal that isn't zero"));
    assert!(refused(&mut s, json!([{"origin":[0,0,-1],"normal":[0,0,1]}])).contains("cuts away every visible body"));
    assert!(refused(&mut s, json!([{"origin":[0,0,0]}])).contains("each with an origin and a normal"));
}
