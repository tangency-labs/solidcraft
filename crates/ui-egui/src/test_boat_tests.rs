use super::*;
use crate::space_mouse::Motion;
use egui::vec2;

fn boat() -> &'static Model {
    model().expect("the built-in model reads")
}

#[test]
fn test_boat_asset_reads_with_its_counts_and_bounds() {
    let m = boat();
    assert!(m.triangles.len() >= 500 && m.triangles.len() <= 2_000, "{} triangles", m.triangles.len());
    // Smooth shading: vertices are shared where the surface is smooth, and split at creases.
    assert!(m.vertices.len() > m.triangles.len() / 3 && m.vertices.len() <= m.triangles.len() * 3, "{} vertices", m.vertices.len());
    assert!(DATA.len() < 100_000, "{} bytes", DATA.len());
    // About 60 mm long, centred on the origin.
    let (low, high) = m.vertices.iter().fold((Vec3::new(1e9, 1e9, 1e9), Vec3::new(-1e9, -1e9, -1e9)), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let size = high - low;
    assert!((size.x.max(size.y).max(size.z) - 60.0).abs() < 1.0, "{size:?}");
    assert!((m.radius - 30.0).abs() < 15.0 && m.radius >= 25.0, "radius {}", m.radius);
    // Every normal is a unit vector.
    assert!(m.normals.iter().all(|n| (n.len() - 1.0).abs() < 1e-6));
    assert!(m.vertex_normals.iter().all(|n| (n.len() - 1.0).abs() < 1e-6));
}

#[test]
fn test_boat_rejects_files_that_do_not_add_up() {
    assert!(parse(&[]).is_none() && parse(&DATA[..40]).is_none());
    let mut long = DATA.to_vec();
    long.push(0);
    assert!(parse(&long).is_none());
    // An index past the vertices.
    let mut bad = DATA.to_vec();
    let at = bad.len() - 2;
    bad[at..].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(parse(&bad).is_none());
    // A half size that is not a number.
    let mut nan = DATA.to_vec();
    nan[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(parse(&nan).is_none());
}

fn motion(translation: [f64; 3], rotation: [f64; 3]) -> Motion {
    Motion { translation, rotation }
}

#[test]
fn test_boat_step_orbits_pans_zooms_rolls_and_holds_the_boat_in_its_box() {
    let radius = boat().radius;
    let start = Boat::new(radius).cam;
    let aspect = 1.4;
    let run = |m: Motion, seconds: f64| {
        let mut cam = start;
        step(&mut cam, m, seconds, 150.0, radius, aspect, false, false);
        cam
    };
    // Nothing moves nothing.
    assert_eq!(run(motion([0.0; 3], [0.0; 3]), 0.05), start);
    // Orbit turns the view about the boat; roll rolls it; both keep the target.
    let orbit = run(motion([0.0; 3], [0.5, 0.5, 0.0]), 0.05);
    assert!(orbit.yaw != start.yaw || orbit.pitch != start.pitch);
    assert_eq!(orbit.target, start.target);
    assert!(run(motion([0.0; 3], [0.0, 0.0, 0.5]), 0.05).roll != start.roll);
    // Zoom changes the distance one way for push and the other for pull.
    let (push, pull) = (run(motion([0.0, 1.0, 0.0], [0.0; 3]), 0.05), run(motion([0.0, -1.0, 0.0], [0.0; 3]), 0.05));
    assert!(push.distance < start.distance && pull.distance > start.distance);
    // Pan moves the target, within the room the box leaves.
    let panned = run(motion([1.0, 0.0, 0.0], [0.0; 3]), 0.05);
    assert!(panned.target != start.target);
    // Held down for a long time, the limits hold: nearest, farthest, and the pan room.
    let mut cam = start;
    for _ in 0..400 {
        step(&mut cam, motion([1.0, 1.0, 1.0], [0.0; 3]), 0.05, 150.0, radius, aspect, false, false);
    }
    assert!(cam.distance >= radius * NEAREST / 1.0 - 1e-6 && cam.target.len() <= cam.distance * (FOV / 2.0).tan() + 1e-6);
    let mut cam = start;
    for _ in 0..400 {
        step(&mut cam, motion([-1.0, -1.0, -1.0], [0.0; 3]), 0.05, 150.0, radius, aspect, false, false);
    }
    assert!(cam.distance <= radius * FARTHEST + 1e-6);
    // Hostile numbers leave a valid camera.
    let mut cam = start;
    step(&mut cam, motion([f64::NAN, f64::INFINITY, 1e300], [f64::NAN; 3]), 0.05, 150.0, radius, aspect, false, false);
    assert!(cam.is_valid());
}

#[test]
fn test_boat_draws_inside_its_box_at_every_size_and_pose() {
    let m = boat();
    for (w, h) in [(120.0_f32, 90.0_f32), (200.0, 150.0), (260.0, 150.0), (150.0, 260.0), (400.0, 100.0)] {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(w, h));
        let aspect = f64::from(w) / f64::from(h);
        let mut cam = Boat::new(m.radius).cam;
        // Through the extremes of what the 3D mouse can do.
        for (translation, rotation) in [([0.0; 3], [0.0; 3]), ([1.0, -1.0, 1.0], [1.0, 0.0, 1.0]), ([-1.0, 1.0, -1.0], [0.0, 1.0, -1.0])] {
            for _ in 0..200 {
                step(&mut cam, motion(translation, rotation), 0.05, f64::from(h), m.radius, aspect, false, false);
            }
            let frame = project(m, &cam, rect.shrink(1.0));
            assert!(!frame.indices.is_empty(), "{w}x{h}: something is drawn");
            assert!(frame.shades.iter().all(|s| (0.0..=1.0).contains(s)));
            for &index in &frame.indices {
                let corner = frame.points[index as usize];
                assert!(rect.expand(0.5).contains(corner), "{w}x{h}: {corner:?} is outside {rect:?}");
            }
        }
    }
}

/// What a frame asks of the painter: every vertex once and the facing triangles' indices, with a
/// ceiling, and the time it takes to build one (printed; a loose limit so a slow runner is not a failure).
#[test]
fn test_boat_costs_a_bounded_amount_to_project_per_frame() {
    let m = boat();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(260.0, 150.0));
    let cam = Boat::new(m.radius).cam;
    let started = std::time::Instant::now();
    let frames = 20;
    let mut last = None;
    for _ in 0..frames {
        last = Some(project(m, &cam, rect));
    }
    let per_frame = started.elapsed() / frames;
    let frame = last.unwrap();
    eprintln!("BOAT COST {} vertices, {} indices, {per_frame:?} per frame", frame.points.len(), frame.indices.len());
    assert!(frame.points.len() <= 5_000 && frame.indices.len() <= 3 * 5_000, "{} vertices, {} indices", frame.points.len(), frame.indices.len());
    assert!(per_frame < std::time::Duration::from_millis(100), "{per_frame:?}");
}

#[test]
fn test_boat_far_faces_come_first_and_a_page_opened_again_starts_afresh() {
    let m = boat();
    let cam = Boat::new(m.radius).cam;
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 150.0));
    let drawn = project(m, &cam, rect).indices.len() / 3;
    // Only the faces turned to the eye: roughly half the triangles of a closed boat.
    assert!(drawn < m.triangles.len() * 3 / 4 && drawn > m.triangles.len() / 4, "{drawn} of {}", m.triangles.len());
    // The boat comes back with the page: not drawn for a moment, the state starts afresh.
    let ctx = crate::fonts::test_context();
    let mut state = None;
    let moved = Some(DeviceMotion { motion: motion([0.0; 3], [1.0, 0.0, 0.0]), seconds: 0.05 });
    let draw = |time: f64, motion: Option<DeviceMotion>, state: &mut Option<Boat>| {
        let mut output = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
            show(ui, state, motion, false, false, vec2(200.0, 150.0));
        });
        output.textures_delta.clear();
    };
    draw(1.0, moved, &mut state);
    draw(1.016, moved, &mut state);
    let turned = state.unwrap().cam;
    assert_ne!((turned.yaw, turned.pitch), (Boat::new(m.radius).cam.yaw, Boat::new(m.radius).cam.pitch));
    draw(5.0, None, &mut state);
    assert_eq!(state.unwrap().cam, Boat::new(m.radius).cam, "away for a while, it starts afresh");
    reset(&mut state);
    assert!(state.is_none());
}
