//! Stateless camera commands shared by the navigation cube and programmatic callers.
use serde_json::{Value, json};
use solidcraft_render::{Camera, StandardView};

use super::CommandSpec;
use crate::params::{bad, num, req_vec2, req_vec3, vec3};
use crate::{Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[
CommandSpec::new("view.layout", "Workspace layout", layout)
    .icon("settings").noundo().doc_only().params("mode: floating|compact → validated workspace layout; display only, never opens a dialog"),
CommandSpec::new("view.history_layout", "History layout", history_layout)
    .icon("settings").noundo().doc_only().params("width?: 1..1000000; dock?: bottom|left|under_browser; collapsed?: bool → validated layout request; display only, never opens a dialog"),
CommandSpec::new("view.graphics_preset", "Graphics Preset", graphics_preset)
    .icon("settings").noundo().doc_only().params("preset: performance|quality → preset, enabled, display_quality, supersampling, supersampling_adaptive; Quality: AO, Fine, adaptive 2x SSAA; Performance: no AO, Normal, no SSAA"),
CommandSpec::new("view.ambient_occlusion", "Ambient Occlusion", ambient_occlusion)
    .icon("settings").noundo().doc_only().params("enabled: bool → enabled; apply soft contact shading in the GPU viewport"),
CommandSpec::new("view.perspective", "Perspective view", perspective)
    .icon("perspective").noundo().doc_only().params("enabled: bool → enabled; draw the view in perspective, or orthographic when false; a sketch being edited is always shown orthographic"),
CommandSpec::new("view.navigate", "Navigate View", navigate)
    .icon("orbit")
    .noundo()
    .doc_only()
    .params("camera: Camera; direction?: [x,y,z] in the camera's up-axis frame; or action: left|right|up|down|clockwise|counterclockwise|home|fit, degrees?: 0..180 (how far left|right|up|down turn; default 90); or motion: pan|orbit|zoom, delta?: [dx,dy] (pixels, each ±10000), free_orbit?: bool (orbit through poles, default false), viewport?: [width,height] (1..1000000, required for pan or pivot), pivot?: [x,y,z], factor?: 0.05..20 (required for zoom), anchor?: [x,y,z] → camera, fit, continuous?"),
CommandSpec::new("view.normal_to", "Normal to Selection", normal_to)
        .icon("camera")
        .noundo()
        .enabled(|s| normal_target(s).map(|_| ()).ok_or_else(|| "select a planar face or plane".into()))
        .params("camera?: Camera (default: current session camera or home view) → camera; Look At the first selected planar face or plane, centered on the picked point; repeat while aligned to flip the side; preserves scale and projection")];

fn normal_target(s: &Session) -> Option<(solidcraft_geom::Vec3, solidcraft_geom::Vec3)> {
    let state = s.world_state();
    s.selection.iter().find_map(|selection| match selection {
        Sel::Face { body, index, point } if point.is_finite() => {
            let plane = state.body(body)?.body.planar_face(*index)?;
            Some((*point, plane.normal()))
        }
        Sel::Plane { name } => solidcraft_geom::Plane::named(name).map(|plane| (plane.origin, plane.normal())),
        _ => None,
    })
}

fn normal_to(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.normal_to";
    let (point, normal) = normal_target(s).ok_or_else(|| bad(cmd, "select a planar face or plane"))?;
    let cam: Camera = match p.get("camera") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| bad(cmd, "invalid camera"))?,
        None => s.camera.unwrap_or_else(|| crate::view::home_camera(s)),
    };
    if !cam.is_valid()
        || !cam.fov.is_finite()
        || !(0.0..std::f64::consts::PI).contains(&cam.fov)
        || cam.yaw.abs() > 1e6
        || cam.roll.abs() > 1e6
        || cam.pitch.abs() > std::f64::consts::FRAC_PI_2
    {
        return Err(bad(cmd, "invalid camera"));
    }
    // Promoted from viewport::look_at_selection: asking again from the normal side flips it.
    let direction = if cam.back().dot(normal) > 0.999 { -normal } else { normal };
    let mut to = cam.looking_from(direction);
    to.target = point;
    s.camera = Some(to);
    Ok(json!({"camera": to}))
}

fn layout(_s: &mut Session, p: &Value) -> Result<Value> {
    let mode = p
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "floating" | "compact"))
        .ok_or_else(|| bad("view.layout", "mode must be floating or compact"))?;
    Ok(json!({"mode": mode}))
}

fn history_layout(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.history_layout";
    let mut result = serde_json::Map::new();
    if let Some(value) = p.get("width") {
        let width = value
            .as_f64()
            .filter(|v| v.is_finite() && (1.0..=1_000_000.0).contains(v))
            .ok_or_else(|| bad(cmd, "width must be finite and within 1..1000000"))?;
        result.insert("width".into(), json!(width));
    }
    if let Some(value) = p.get("dock") {
        let dock = value
            .as_str()
            .filter(|v| matches!(*v, "bottom" | "left" | "under_browser"))
            .ok_or_else(|| bad(cmd, "dock must be bottom, left or under_browser"))?;
        result.insert("dock".into(), json!(dock));
    }
    if let Some(value) = p.get("collapsed") {
        let collapsed = value.as_bool().ok_or_else(|| bad(cmd, "collapsed must be a boolean"))?;
        result.insert("collapsed".into(), json!(collapsed));
    }
    Ok(Value::Object(result))
}

fn navigate(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.navigate";
    let mut cam: Camera =
        serde_json::from_value(p.get("camera").cloned().ok_or_else(|| bad(cmd, "camera is required"))?).map_err(|_| bad(cmd, "invalid camera"))?;
    if !cam.is_valid()
        || !cam.fov.is_finite()
        || !(0.0..std::f64::consts::PI).contains(&cam.fov)
        || cam.yaw.abs() > 1e6
        || cam.roll.abs() > 1e6
        || cam.pitch.abs() > std::f64::consts::FRAC_PI_2
    {
        return Err(bad(cmd, "invalid camera"));
    }
    if p.get("motion").is_some() {
        return continuous(cam, p);
    }
    let mut fit = false;
    if let Some(value) = p.get("direction") {
        let dir = vec3(value)
            .filter(|v| v.is_finite() && v.len() <= 2.0)
            .and_then(|v| v.normalized())
            .ok_or_else(|| bad(cmd, "direction must be a finite nonzero vector of length at most 2"))?;
        let heading = cam.yaw;
        cam = cam.looking_from(cam.from_frame(dir));
        if dir.x.abs() < 1e-9 && dir.y.abs() < 1e-9 {
            cam.yaw = square_heading(heading);
        }
    } else {
        use std::f64::consts::FRAC_PI_2;
        let (r, u, b) = cam.basis();
        // Left, right, up and down turn the view a quarter turn (the navigation cube's arrows),
        // or `degrees` (the arrow keys' 15° steps, #1541).
        let (sin, cos) = match p.get("degrees") {
            None => (1.0, 0.0),
            Some(d) => d
                .as_f64()
                .filter(|d| d.is_finite() && *d > 0.0 && *d <= 180.0)
                .map(|d| if d == 90.0 { (1.0, 0.0) } else { d.to_radians().sin_cos() })
                .ok_or_else(|| bad(cmd, "degrees must be a number above 0 and at most 180"))?,
        };
        let turn = |toward: solidcraft_geom::Vec3| cam.looking_from(b * cos + toward * sin);
        match p.get("action").and_then(Value::as_str) {
            Some("left") => cam = turn(-r),
            Some("right") => cam = turn(r),
            Some("up") => cam = turn(u),
            Some("down") => cam = turn(-u),
            Some("clockwise") => cam.roll = (cam.roll + FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            Some("counterclockwise") => cam.roll = (cam.roll - FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            Some("home") => {
                cam.set_view(StandardView::Iso);
                fit = true;
            }
            Some("fit") => fit = true,
            _ => return Err(bad(cmd, "unknown navigation action")),
        }
    }
    Ok(json!({"camera": cam, "fit": fit}))
}

/// Direct manipulation is stateless, bounded and independent of document evaluation.
fn continuous(mut cam: Camera, p: &Value) -> Result<Value> {
    let cmd = "view.navigate";
    let free_orbit = match p.get("free_orbit") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| bad(cmd, "free_orbit must be a boolean"))?,
    };
    let viewport = || {
        let size = req_vec2(cmd, p, "viewport")?;
        if !(1.0..=1e6).contains(&size.x) || !(1.0..=1e6).contains(&size.y) {
            return Err(bad(cmd, "viewport dimensions must be in 1..1000000"));
        }
        Ok(size)
    };
    match p.get("motion").and_then(Value::as_str) {
        Some(motion @ ("pan" | "orbit")) => {
            let delta = req_vec2(cmd, p, "delta")?;
            if delta.x.abs() > 1e4 || delta.y.abs() > 1e4 {
                return Err(bad(cmd, "motion deltas must be within ±10000 pixels"));
            }
            let orbit = |cam: &mut Camera| {
                if free_orbit {
                    cam.rotate_in_view(solidcraft_geom::Vec3::new(-delta.y * 0.008, -delta.x * 0.008, 0.0));
                } else {
                    cam.orbit(delta.x, delta.y);
                }
            };
            if motion == "pan" {
                cam.pan(delta.x, delta.y, viewport()?.y);
            } else if p.get("pivot").is_some() {
                let pivot = req_vec3(cmd, p, "pivot")?;
                viewport()?;
                let coords = cam.frame_coords(pivot);
                orbit(&mut cam);
                cam.place(pivot, coords);
            } else {
                orbit(&mut cam);
            }
        }
        Some("zoom") => {
            let factor = num(p, "factor").filter(|f| (0.05..=20.0).contains(f)).ok_or_else(|| bad(cmd, "factor must be in 0.05..20"))?;
            let anchor = p.get("anchor").map(|_| req_vec3(cmd, p, "anchor")).transpose()?;
            cam.zoom_at(factor, anchor);
        }
        _ => return Err(bad(cmd, "unknown navigation motion")),
    }
    if !cam.is_valid() {
        return Err(bad(cmd, "motion produced an invalid camera"));
    }
    Ok(json!({"camera":cam, "fit":false, "continuous":true}))
}

/// At a halfway tie prefer the heading nearer the front plan view.
pub(crate) fn square_heading(yaw: f64) -> f64 {
    if !yaw.is_finite() {
        return 0.0;
    }
    let q = std::f64::consts::FRAC_PI_2;
    let lo = (yaw / q).floor() * q;
    let hi = lo + q;
    if ((yaw - lo) - (hi - yaw)).abs() < 1e-6 {
        let off_front = |a: f64| ((a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
        if off_front(lo) <= off_front(hi) { lo } else { hi }
    } else if yaw - lo < hi - yaw {
        lo
    } else {
        hi
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    use solidcraft_render::UpAxis;

    #[test]
    fn look_at_planes_uses_the_first_supported_selection_and_flips_on_repeat() {
        let original = Camera { yaw: 0.3, pitch: 0.4, roll: 0.7, fov: 0.6, distance: 123.0, ..Default::default() };
        let mut s = Session {
            selection: vec![Sel::Plane { name: "missing".into() }, Sel::Plane { name: "XY".into() }, Sel::Plane { name: "YZ".into() }],
            camera: Some(original),
            ..Default::default()
        };
        for normal in [Vec3::Z, -Vec3::Z, Vec3::Z] {
            let result = s.execute("view.normal_to", &json!({})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(normal) < 2e-6);
            assert_eq!((to.target, to.distance, to.fov), (Vec3::ZERO, original.distance, original.fov));
        }
        assert!(s.undo.is_empty());
    }

    #[test]
    fn normal_to_selection_faces_each_planar_surface_without_editing_the_design() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.01).unwrap();
        let document = s.doc.to_json();
        let undo = s.undo.len();
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.0, 0.6] {
                let cam = Camera { up, fov, roll: 1.2, distance: 123.0, ..Default::default() };
                for face in &faces {
                    s.selection = vec![Sel::Face { body: body.name.clone(), index: face.index, point: face.centroid }];
                    let selected = s.selection.clone();
                    let result = s.execute("view.normal_to", &json!({"camera":cam})).unwrap();
                    let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                    assert!(to.back().dist(face.plane_normal.unwrap()) < 2e-6);
                    assert_eq!(to.target, face.centroid);
                    assert_eq!((to.distance, to.fov, to.up, to.roll), (cam.distance, cam.fov, cam.up, 0.0));
                    assert_eq!(s.camera, Some(to));
                    assert_eq!(s.selection, selected);
                }
            }
        }
        assert_eq!(s.doc.to_json(), document);
        assert_eq!(s.undo.len(), undo);
    }

    #[test]
    fn normal_to_selection_rejects_curved_missing_and_invalid_inputs() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"diameter":20,"height":30})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let flat = faces.iter().find(|f| f.plane_normal.is_some()).unwrap();
        let curved = faces.iter().find(|f| f.plane_normal.is_none()).unwrap();
        let selected = |index, point| Sel::Face { body: body.name.clone(), index, point };
        for selection in [vec![], vec![selected(curved.index, curved.centroid)], vec![selected(usize::MAX, Vec3::ZERO)]] {
            s.selection = selection;
            assert!(s.execute("view.normal_to", &json!({})).is_err());
            assert!(s.camera.is_none());
        }
        s.selection = vec![selected(flat.index, flat.centroid)];
        for camera in [
            json!({}),
            json!({"distance":0}),
            json!(Camera { distance: -1.0, ..Default::default() }),
            json!(Camera { fov: 4.0, ..Default::default() }),
        ] {
            assert!(s.execute("view.normal_to", &json!({"camera":camera})).is_err());
            assert!(s.camera.is_none());
        }
        // Programmatic callers can use the current session view without supplying a camera.
        assert!(s.execute("view.normal_to", &json!({})).is_ok());
    }

    #[test]
    fn normal_to_selection_uses_the_world_normal_of_a_moved_component() {
        let mut s = Session::default();
        s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
        s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence":"Part:1","translate":[50,20,10],"axis":[1,2,3],"angle":"37 deg"})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.01).unwrap();
        let cam = Camera { roll: 1.4, ..Default::default() };
        for face in faces {
            s.selection = vec![Sel::Face { body: body.name.clone(), index: face.index, point: face.centroid }];
            let result = s.execute("view.normal_to", &json!({"camera":cam})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(face.plane_normal.unwrap()) < 2e-6);
            assert!(to.target.dist(face.centroid) < 1e-9);
        }
    }

    /// #1541: left, right, up and down turn a quarter turn, or `degrees` (the arrow keys' 15°).
    #[test]
    fn turns_take_degrees_and_default_to_a_quarter_turn() {
        let mut s = Session::default();
        let cam = Camera { yaw: 0.0, pitch: 0.0, distance: 100.0, ..Default::default() };
        let mut turn = |action: &str, degrees: Option<Value>| -> Result<Camera> {
            let mut params = json!({"camera": cam, "action": action});
            if let Some(d) = degrees {
                params["degrees"] = d;
            }
            let result = s.execute("view.navigate", &params)?;
            Ok(serde_json::from_value(result["camera"].clone()).unwrap())
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // A quarter turn as before, given or not.
        let quarter = turn("left", None).unwrap();
        assert_eq!(turn("left", Some(json!(90))).unwrap(), quarter);
        assert!(close((quarter.yaw - cam.yaw).abs(), std::f64::consts::FRAC_PI_2));
        // 15° each way, about the up axis or over the top.
        for (action, yaw, pitch) in [("left", -15.0, 0.0), ("right", 15.0, 0.0), ("up", 0.0, 15.0), ("down", 0.0, -15.0)] {
            let to = turn(action, Some(json!(15))).unwrap();
            assert!(close(to.yaw.abs(), f64::to_radians(yaw).abs()), "{action}: yaw {}", to.yaw);
            assert!(close(to.pitch, f64::to_radians(pitch)), "{action}: pitch {}", to.pitch);
            assert_eq!((to.target, to.distance), (cam.target, cam.distance));
        }
        // Left and right turn opposite ways.
        assert!(close(turn("left", Some(json!(15))).unwrap().yaw, -turn("right", Some(json!(15))).unwrap().yaw));
        for bad in [json!(0), json!(-15), json!(181), json!(f64::NAN.to_string()), json!("15"), json!(null)] {
            assert!(turn("left", Some(bad.clone())).is_err(), "{bad}");
        }
    }

    #[test]
    fn continuous_navigation_moves_the_camera_without_editing_the_design() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        let cam = Camera { distance: 100.0, ..Default::default() };
        for (motion, extra) in
            [("pan", json!({"delta":[30,20],"viewport":[800,600]})), ("orbit", json!({"delta":[30,20]})), ("zoom", json!({"factor":0.5}))]
        {
            let mut params = extra;
            params["camera"] = json!(cam);
            params["motion"] = json!(motion);
            let result = s.execute("view.navigate", &params).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert_eq!(result["continuous"], true);
            match motion {
                "pan" => {
                    assert_ne!(to.target, cam.target);
                    assert_eq!((to.yaw, to.pitch, to.distance), (cam.yaw, cam.pitch, cam.distance));
                }
                "orbit" => {
                    assert!((to.yaw - cam.yaw - 0.24).abs() < 1e-9);
                    assert!((to.pitch - cam.pitch - 0.16).abs() < 1e-9);
                    assert_eq!((to.target, to.distance), (cam.target, cam.distance));
                }
                _ => assert_eq!(to.distance, 50.0),
            }
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn continuous_free_orbit_is_opt_in_and_preserves_framing_and_document() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut s = Session::default();
            let design = s.doc.to_json();
            let cam = Camera { pitch: 1.4, roll: 0.7, up, ..Default::default() };
            let mut p = json!({"camera":cam,"motion":"orbit","delta":[0,80]});
            let default = s.execute("view.navigate", &p).unwrap();
            p["free_orbit"] = json!(false);
            assert_eq!(s.execute("view.navigate", &p).unwrap(), default);
            let clamped: Camera = serde_json::from_value(default["camera"].clone()).unwrap();
            assert!(clamped.pitch > cam.pitch && clamped.pitch < std::f64::consts::FRAC_PI_2);
            p["free_orbit"] = json!(true);
            let result = s.execute("view.navigate", &p).unwrap();
            let free: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!((free.back().dot(cam.back()) - 0.64_f64.cos()).abs() < 1e-8);
            assert_eq!((free.target, free.distance, free.fov), (cam.target, cam.distance, cam.fov));
            assert_eq!(s.doc.to_json(), design);
            assert!(s.undo.is_empty() && s.selection.is_empty());
        }
    }

    #[test]
    fn continuous_free_orbit_keeps_the_pivot_in_the_camera_frame() {
        for up in [UpAxis::Y, UpAxis::Z] {
            for fov in [0.0, 0.6] {
                let mut s = Session::default();
                let cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), pitch: 1.4, roll: 0.7, fov, up, ..Default::default() };
                for pivot in [Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)] {
                    let result = s
                        .execute(
                            "view.navigate",
                            &json!({"camera":cam,"motion":"orbit","delta":[30,80],"free_orbit":true,"viewport":[800,600],"pivot":pivot}),
                        )
                        .unwrap();
                    let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                    assert!(to.frame_coords(pivot).dist(cam.frame_coords(pivot)) < 1e-8);
                    assert!(((to.eye() - pivot).len() - (cam.eye() - pivot).len()).abs() < 1e-8);
                    let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
                    let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
                    assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6);
                    assert_eq!((to.distance, to.fov), (cam.distance, cam.fov));
                }
            }
        }
    }

    #[test]
    fn continuous_perspective_orbit_keeps_near_and_far_pivots_on_screen() {
        let mut s = Session::default();
        let cam = Camera { yaw: 0.0, pitch: 0.0, distance: 100.0, fov: 0.6, ..Default::default() };
        for pivot in [Vec3::new(15.0, -50.0, 0.0), Vec3::new(15.0, 50.0, 0.0)] {
            let result =
                s.execute("view.navigate", &json!({"camera":cam,"motion":"orbit","delta":[20,10],"viewport":[800,600],"pivot":pivot})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
            let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
            assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6, "pivot {pivot:?} drifted from ({ax},{ay}) to ({bx},{by})");
        }
    }

    #[test]
    fn continuous_orbit_keeps_its_pivot_on_screen_and_rejects_hostile_motion() {
        let mut s = Session::default();
        let cam = Camera { distance: 100.0, ..Default::default() };
        let pivot = Vec3::new(12.0, 5.0, 0.0);
        let result = s.execute("view.navigate", &json!({"camera":cam,"motion":"orbit","delta":[20,10],"viewport":[800,600],"pivot":pivot})).unwrap();
        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
        let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 1.0).unwrap();
        let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 1.0).unwrap();
        assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6);
        for extra in [
            json!({"motion":"pan","delta":[1,2],"viewport":[800,0]}),
            json!({"motion":"pan","delta":[1e100,2],"viewport":[800,600]}),
            json!({"motion":"orbit","delta":[1]}),
            json!({"motion":"orbit","delta":[1,2],"pivot":[0,0,0]}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":"true"}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":1}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":null}),
            json!({"motion":"zoom","factor":0}),
            json!({"motion":"zoom","factor":1e100}),
            json!({"motion":"zoom","factor":1,"anchor":[1e100,0,0]}),
            json!({"motion":"unknown"}),
        ] {
            let mut p = extra;
            p["camera"] = json!(cam);
            assert!(s.execute("view.navigate", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn every_cube_direction_preserves_framing_without_editing_the_design() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut s = Session::default();
            let before = s.doc.to_json();
            let cam = Camera { up, target: Vec3::new(10.0, 20.0, 30.0), distance: 123.0, ..Default::default() };
            for x in [-1.0, 0.0, 1.0] {
                for y in [-1.0, 0.0, 1.0] {
                    for z in [-1.0, 0.0, 1.0] {
                        let Some(dir) = Vec3::new(x, y, z).normalized() else { continue };
                        let result = s.execute("view.navigate", &json!({"camera":cam,"direction":[x,y,z]})).unwrap();
                        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                        assert!(to.back().dist(cam.from_frame(dir)) < 2e-6);
                        assert_eq!((to.target, to.distance, to.fov, to.up), (cam.target, cam.distance, cam.fov, cam.up));
                    }
                }
            }
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }

    #[test]
    fn arrows_roll_and_home_are_programmatic_and_reject_hostile_input() {
        let mut s = Session::default();
        let cam = Camera::default();
        let (r, u, _) = cam.basis();
        for (action, dir) in [("left", -r), ("right", r), ("up", u), ("down", -u)] {
            let result = navigate(&mut s, &json!({"camera":cam,"action":action})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(dir) < 2e-6);
        }
        let result = navigate(&mut s, &json!({"camera":cam,"action":"clockwise"})).unwrap();
        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
        assert_eq!(to.back(), cam.back());
        assert!((to.basis().0 - u).len() < 1e-9);
        for action in ["home", "fit"] {
            assert_eq!(navigate(&mut s, &json!({"camera":cam,"action":action})).unwrap()["fit"], true);
        }
        for params in [
            json!({}),
            json!({"camera":cam,"direction":[0,0,0]}),
            json!({"camera":cam,"direction":[1e308,0,0]}),
            json!({"camera":cam,"action":"unknown"}),
        ] {
            assert!(s.execute("view.navigate", &params).is_err());
        }
    }
}

fn ambient_occlusion(_s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("view.ambient_occlusion", "enabled must be a boolean"))?;
    Ok(json!({"enabled": enabled}))
}

fn perspective(_s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("view.perspective", "enabled must be a boolean"))?;
    Ok(json!({"enabled": enabled}))
}

#[cfg(test)]
mod ambient_occlusion_tests {
    use super::*;
    #[test]
    fn validates_display_setting_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for enabled in [true, false] {
            assert_eq!(s.execute("view.ambient_occlusion", &json!({"enabled": enabled})).unwrap(), json!({"enabled": enabled}));
        }
        for p in [json!({}), json!({"enabled": 1}), json!({"enabled": "true"}), json!({"enabled": null})] {
            assert!(s.execute("view.ambient_occlusion", &p).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn perspective_is_a_display_setting_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for enabled in [true, false] {
            assert_eq!(s.execute("view.perspective", &json!({"enabled": enabled})).unwrap(), json!({"enabled": enabled}));
        }
        for p in [json!({}), json!({"enabled": 1}), json!({"enabled": "true"}), json!({"enabled": null})] {
            assert!(s.execute("view.perspective", &p).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }
}

/// One preset for contact shading, tessellation and supersampling. MSAA stays independent.
fn graphics_preset(_s: &mut Session, p: &Value) -> Result<Value> {
    let preset = p.get("preset").and_then(Value::as_str);
    let enabled = match preset {
        Some("quality") => true,
        Some("performance") => false,
        _ => return Err(bad("view.graphics_preset", "preset must be performance or quality")),
    };
    Ok(json!({"preset": preset, "enabled": enabled, "display_quality": if enabled { "fine" } else { "normal" },
        "supersampling": if enabled { 2.0 } else { 1.0 }, "supersampling_adaptive": true}))
}

#[cfg(test)]
mod graphics_preset_tests {
    use super::*;

    #[test]
    fn presets_validate_without_changing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for (preset, enabled) in [("quality", true), ("performance", false)] {
            assert_eq!(
                s.execute("view.graphics_preset", &json!({"preset":preset})).unwrap(),
                json!({"preset":preset,"enabled":enabled,"display_quality":if enabled { "fine" } else { "normal" },
                    "supersampling":if enabled { 2.0 } else { 1.0 },"supersampling_adaptive":true})
            );
        }
        for params in [json!({}), json!({"preset":"fast"}), json!({"preset":true}), json!({"preset":null})] {
            assert!(s.execute("view.graphics_preset", &params).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }
}

#[cfg(test)]
#[test]
fn history_layout_validates_geometry_without_changing_the_design() {
    let mut s = Session::default();
    let doc = s.doc.clone();
    let request = json!({"width":240.0,"dock":"under_browser","collapsed":true});
    assert_eq!(s.execute("view.history_layout", &request).unwrap(), request);
    for p in [json!({"width":-1}), json!({"width":1e20}), json!({"width":"wide"}), json!({"dock":"nowhere"}), json!({"collapsed":42})] {
        assert!(s.execute("view.history_layout", &p).is_err());
    }
    assert_eq!(s.doc, doc);
    assert!(s.undo.is_empty());
}

#[cfg(test)]
#[test]
fn workspace_layout_validates_modes_without_mutating_the_design() {
    let mut session = Session::default();
    let before = session.doc.to_json();
    for mode in ["floating", "compact"] {
        assert_eq!(session.execute("view.layout", &json!({"mode":mode})).unwrap(), json!({"mode":mode}));
    }
    for params in [json!({}), json!({"mode":"wide"}), json!({"mode":true}), json!({"mode":null}), json!({"mode":42})] {
        assert!(session.execute("view.layout", &params).is_err());
    }
    assert_eq!(session.doc.to_json(), before);
    assert!(session.undo.is_empty());
}
