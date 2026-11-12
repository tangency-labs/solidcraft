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
    assert!(!top["dxf"].as_str().unwrap().contains("HIDDEN"));
    assert!(dashed["dxf"].as_str().unwrap().contains("LWPOLYLINE"));
    assert!(dashed["dxf"].as_str().unwrap().contains("128\n"));
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
    assert!((length(&active) - 80.).abs() < 1e-3 && active["dxf"].as_str().unwrap().contains("SKETCH"));
    run(&mut s, "sketch.finish", json!({}));
    let finished = export(&mut s, camera(StandardView::Top), "omit");
    assert_eq!(finished["sketch"], 2, "{finished}");
    assert!((length(&finished) - 40.).abs() < 1e-3, "{}", length(&finished));
    let dashed = export(&mut s, camera(StandardView::Top), "dashed");
    assert_eq!(dashed["sketch"], 2);
    assert!((length(&dashed) - 80.).abs() < 1e-3, "{}", length(&dashed));
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
fn ring(c: Vec2, r: f64, n: usize, from: f64, to: f64) -> Vec<DxfEntity> {
    let at = |i: usize| {
        let t = from + (to - from) * i as f64 / n as f64;
        Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())
    };
    (0..n).map(|i| DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(at(i), at(i + 1)) }).collect()
}

#[test]
fn view_dxf_arcs_turn_a_tessellated_circle_into_a_circle_and_keep_polygons_as_lines() {
    let tau = std::f64::consts::TAU;
    let c = Vec2::new(3., -2.);
    // A fine ring is a circle; coarse rings (an octagon, even a 32-gon) sag too far from the
    // circle for the 0.01 mm the drawing was made with, so they are polygons and stay lines.
    let fine = arcs::recognise(ring(c, 10., 64, 0., tau), 0.01);
    assert_eq!(fine.len(), 1, "{fine:?}");
    let DxfShape::Circle { c: found, r } = &fine[0].shape else { panic!("{fine:?}") };
    assert!((*found - c).len() < 1e-9 && (r - 10.).abs() < 1e-9, "{fine:?}");
    for sides in [6, 8, 32] {
        let polygon = arcs::recognise(ring(c, 10., sides, 0., tau), 0.01);
        assert_eq!(polygon.len(), sides, "{sides} sides");
        assert!(polygon.iter().all(|e| matches!(e.shape, DxfShape::Line(..))));
    }
}

#[test]
fn view_dxf_arcs_turn_a_run_of_lines_into_an_arc_that_runs_counter_clockwise() {
    let c = Vec2::new(0., 0.);
    let quarter = std::f64::consts::FRAC_PI_2;
    // The run goes round clockwise, from 90 to 0 degrees: the DXF arc names 0 first, then 90.
    let run = arcs::recognise(ring(c, 10., 40, quarter, 0.), 0.01);
    assert_eq!(run.len(), 1, "{run:?}");
    let DxfShape::Arc { r, a0, a1, .. } = &run[0].shape else { panic!("{run:?}") };
    assert!((r - 10.).abs() < 1e-9 && a0.abs() < 1e-6 && (a1 - 90.).abs() < 1e-6, "{run:?}");
    // A straight run, and a few lines, are never an arc.
    let straight: Vec<DxfEntity> = (0..10)
        .map(|i| DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(Vec2::new(i as f64, 0.), Vec2::new(i as f64 + 1., 0.)) })
        .collect();
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
