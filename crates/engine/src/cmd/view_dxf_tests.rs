use super::*;
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_render::StandardView;

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
    let undo = s.history.len();
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
        assert_eq!(s.history.len(), undo);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn view_dxf_blind_back_hole_is_omitted_or_dashed_and_front_hole_is_visible() {
    let mut s = sample();
    let undo = s.history.len();
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
    assert_eq!(s.history.len(), undo);
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
    assert_eq!(result["stats"]["worker_limit"], meshes::WORKERS);
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
