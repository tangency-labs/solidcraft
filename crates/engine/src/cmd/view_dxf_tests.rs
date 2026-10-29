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
    let first = s.world_state().bodies.first().map(|b| b.name.clone()).unwrap();
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
