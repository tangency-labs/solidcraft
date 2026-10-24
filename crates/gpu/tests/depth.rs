//! Depth in headless pictures, drawn with the viewport's pipelines (#1434, #1739): an edge behind
//! a face stays hidden however large the design and however far the camera, and whatever lies on
//! a face (its edges, a thin label) draws over it at any angle. Each check runs with the depth the
//! device draws with (a 32-bit float buffer, reversed, where it has one) and with the 24-bit
//! fallback. Skipped, with the reason printed, without a GPU adapter (`SOLIDCRAFT_REQUIRE_GPU=1`
//! makes that a failure).

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::{Mesh, Vec3};
use solidcraft_engine::render::{Camera, Rgb, Scene, SceneLine, SceneMesh, StandardView};
use solidcraft_engine::view::{self, Guides};
use solidcraft_gpu::GpuError;
use solidcraft_gpu::capture::{self, Capturer};
use std::sync::{Arc, OnceLock};

fn or_skip(c: Result<&'static Capturer, &'static GpuError>) -> Option<&'static Capturer> {
    match c {
        Ok(c) => Some(c),
        Err(e @ GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// The device's own depth, then the 24-bit fallback (the same device kind, its own instance).
fn capturers() -> Vec<(&'static str, &'static Capturer)> {
    static FALLBACK: OnceLock<solidcraft_gpu::Result<Capturer>> = OnceLock::new();
    let Some(own) = or_skip(capture::shared()) else { return Vec::new() };
    let Some(fallback) = or_skip(FALLBACK.get_or_init(|| Capturer::headless_with_depth(wgpu::TextureFormat::Depth24PlusStencil8)).as_ref()) else {
        return Vec::new();
    };
    eprintln!("GPU: {}, depth {:?} and {:?}", own.adapter_name(), own.depth_format(), fallback.depth_format());
    vec![("own", own), ("24-bit", fallback)]
}

fn reversed(c: &Capturer) -> bool {
    solidcraft_gpu::view::reversed_depth(Some(c.depth_format()))
}

/// RGBA pixels as `[r, g, b, a]`.
fn pixels(img: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    img.as_chunks::<4>().0.iter().copied()
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 150 && p[1] < 90 && p[2] < 90
}

fn session(script: &[(&str, serde_json::Value)]) -> Session {
    let mut s = Session::default();
    for (c, p) in script {
        s.execute(c, p).unwrap();
    }
    s
}

fn picture(c: &Capturer, s: &Session, cam: &Camera, w: u32, h: u32) -> Vec<u8> {
    c.rgba(&view::scene_with_guides(s, cam, None, Guides { grid: false, axes: false }), cam, w, h).unwrap()
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    pixels(a).zip(pixels(b)).filter(|(p, q)| p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > 24)).count()
}

/// The reproduction of #1434: a 20 mm plate with a washer 22–25 mm behind its front face, and a
/// 30 m slab far away that makes the design 34 m across. The washer never shows: the picture is
/// the same without it. (Lines were pulled toward the eye by 2e-4 of the depth range, about
/// 27 mm here in orthographic.)
#[test]
fn a_washer_behind_a_plate_stays_hidden_on_a_large_design() {
    let plate = [
        ("solid.box", json!({"length": 30000, "width": 8000, "height": 100, "corner": [-15000, 3000, -3000], "body_name": "slab"})),
        ("solid.box", json!({"length": 200, "width": 20, "height": 200, "corner": [0, 0, 0], "body_name": "plate"})),
        ("solid.cylinder", json!({"radius": 8, "height": 60, "base": [100, -20, 100], "axis": "Y", "body_name": "bolt"})),
    ];
    let washer = ("solid.cylinder", json!({"radius": 15, "height": 3, "base": [100, 22, 100], "axis": "Y", "body_name": "washer"}));
    let without = session(&plate);
    let with = session(&[plate[0].clone(), plate[1].clone(), plate[2].clone(), washer]);
    for (mode, c) in capturers() {
        for fov in [0.0, 0.6] {
            let cam = Camera { target: Vec3::new(100.0, 10.0, 100.0), yaw: 0.35, pitch: 0.25, roll: 0.0, distance: 450.0, fov, ..Default::default() };
            let n = differing(&picture(c, &with, &cam, 400, 300), &picture(c, &without, &cam, 400, 300));
            assert_eq!(n, 0, "{mode} depth, fov {fov}: the washer shows through the plate in {n} pixels");
        }
    }
}

/// #1434's second reproduction, in perspective at 10 m: a 2 m plate, a 600 mm washer 12–15 mm
/// behind its front face, and a red label 0.3 mm thick on it. The washer stays hidden in both
/// depths; with reversed float depth the label is red all over (in 24 bits one depth step there
/// is about 0.6 mm, so the label speckled).
#[test]
fn at_ten_metres_a_thin_label_is_whole_and_a_washer_behind_stays_hidden() {
    let base = [
        ("solid.box", json!({"length": 2000, "width": 10, "height": 2000, "corner": [0, 0, 0], "body_name": "plate"})),
        ("solid.box", json!({"length": 1000, "width": 0.3, "height": 400, "corner": [500, -0.3, 1400], "body_name": "label"})),
        ("appearance.assign", json!({"bodies": ["label"], "color": [204, 34, 34]})),
    ];
    let washer = ("solid.cylinder", json!({"radius": 300, "height": 3, "base": [1000, 12, 1000], "axis": "Y", "body_name": "washer"}));
    let without = session(&base);
    let with = session(&[base[0].clone(), base[1].clone(), base[2].clone(), washer]);
    let cam =
        Camera { target: Vec3::new(1000.0, 5.0, 1000.0), yaw: 0.35, pitch: 0.25, roll: 0.0, distance: 10_000.0, fov: 0.6, ..Default::default() };
    let (w, h) = (800, 600);
    for (mode, c) in capturers() {
        let img = picture(c, &with, &cam, w, h);
        let n = differing(&img, &picture(c, &without, &cam, w, h));
        assert_eq!(n, 0, "{mode} depth: the washer shows through the plate in {n} pixels");
        if !reversed(c) {
            continue;
        }
        // The label's front face (y = -0.3), sampled inside its outline.
        let radius = view::scene(&with, &cam).radius;
        let mut grey = Vec::new();
        for i in 1..20 {
            for j in 1..20 {
                let p = Vec3::new(500.0 + 1000.0 * f64::from(i) / 20.0, -0.3, 1400.0 + 400.0 * f64::from(j) / 20.0);
                let Some((x, y, _)) = cam.to_screen(p, f64::from(w), f64::from(h), radius) else { continue };
                let at = (y as usize * w as usize + x as usize) * 4;
                if let Some(px) = img.get(at..at + 4).and_then(|p| <[u8; 4]>::try_from(p).ok())
                    && !red(px)
                {
                    grey.push((x as u32, y as u32, px));
                }
            }
        }
        assert!(grey.is_empty(), "{mode} depth: the label speckles at {} of 361 points: {:?}", grey.len(), &grey[..grey.len().min(5)]);
    }
}

/// A 30 × 40 mm sheet at y = z·tan(`tilt`) (facing the front camera when `tilt` is 0).
fn sheet(tilt: f64) -> Mesh {
    let t = tilt.tan();
    let c = [(-15.0, -20.0), (15.0, -20.0), (15.0, 20.0), (-15.0, 20.0)].map(|(x, z)| Vec3::new(x, z * t, z));
    Mesh {
        positions: c.to_vec(),
        normals: vec![Vec3::new(0.0, -tilt.cos(), tilt.sin()); 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    }
}

/// A front view of `mesh` with `lines`, as part of a design of `radius`, 40 mm high on 200 px.
fn front(mesh: Mesh, lines: Vec<SceneLine>, radius: f64, fov: f64) -> (Scene, Camera) {
    let scene = Scene { meshes: vec![SceneMesh::solid(Arc::new(mesh), Rgb(160, 165, 175))], lines, radius, ..Default::default() };
    let mut cam = Camera { distance: if fov > 0.0 { 25.0 / (fov * 0.5).tan() } else { 62.5 }, fov, ..Default::default() };
    cam.set_view(StandardView::Front);
    (scene, cam)
}

fn red_line(a: Vec3, b: Vec3, on_top: bool) -> SceneLine {
    SceneLine { points: vec![a, b], color: Rgb(255, 0, 0), width: 1.2, on_top }
}

fn red_pixels(c: &Capturer, scene: &Scene, cam: &Camera) -> usize {
    pixels(&c.rgba(scene, cam, 200, 200).unwrap()).filter(|p| red(*p)).count()
}

/// An edge lying on a face draws its whole width over it, on a face seen square and on one turned
/// to 80°, in orthographic and perspective, in a small design and a large one.
#[test]
fn an_edge_on_a_face_draws_whole_at_any_angle() {
    for (mode, c) in capturers() {
        for tilt in [0.0_f64, 60.0, 80.0] {
            let t = tilt.to_radians().tan();
            // Across the sheet, at z = 5: y = 5·tan(tilt).
            let (a, b) = (Vec3::new(-12.0, 5.0 * t, 5.0), Vec3::new(12.0, 5.0 * t, 5.0));
            for radius in [30.0, 17_000.0] {
                for fov in [0.0, 0.6] {
                    let (on, cam) = front(sheet(tilt.to_radians()), vec![red_line(a, b, false)], radius, fov);
                    let (top, _) = front(sheet(tilt.to_radians()), vec![red_line(a, b, true)], radius, fov);
                    let (shown, all) = (red_pixels(c, &on, &cam), red_pixels(c, &top, &cam));
                    assert!(all > 50, "{mode}: the line itself ({all} px)");
                    assert!(shown * 10 >= all * 9, "{mode} depth, tilt {tilt}°, radius {radius}, fov {fov}: {shown} of {all} px drawn");
                }
            }
        }
    }
}

/// An edge 0.5 mm behind a face seen square is hidden, however large the design: lines were pulled
/// toward the eye by a share of the depth range, about 27 mm in a design 34 m across.
#[test]
fn an_edge_just_behind_a_face_stays_hidden_on_a_large_design() {
    for (mode, c) in capturers() {
        for radius in [30.0, 17_000.0] {
            for fov in [0.0, 0.6] {
                // The sheet is at y = 0; the camera looks along +y, so behind is +y.
                let (a, b) = (Vec3::new(-12.0, 0.5, 5.0), Vec3::new(12.0, 0.5, 5.0));
                let (scene, cam) = front(sheet(0.0), vec![red_line(a, b, false)], radius, fov);
                let n = red_pixels(c, &scene, &cam);
                assert_eq!(n, 0, "{mode} depth, radius {radius}, fov {fov}: the edge behind shows in {n} px");
            }
        }
    }
}

/// The application's device draws with reversed float depth where it can: every desktop backend
/// this runs on (D3D12, Vulkan, Metal) has `Depth32FloatStencil8`.
#[test]
fn the_device_draws_with_float_depth_where_it_has_it() {
    let Some(c) = or_skip(capture::shared()) else { return };
    eprintln!("GPU: {}, depth {:?}", c.adapter_name(), c.depth_format());
    let float = c.depth_format() == wgpu::TextureFormat::Depth32FloatStencil8;
    assert_eq!(float, reversed(c));
    if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_some() {
        assert!(float, "a desktop GPU without Depth32FloatStencil8: {:?}", c.depth_format());
    }
}

/// What lies on a section's cut face draws over the cap, which writes its own depth (so the GPU's
/// push does not apply to it; the shader pushes it the same way).
#[test]
fn an_edge_on_the_cut_face_draws_over_the_cap() {
    let mut s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]);
    // Cut away the front (y < 15), so the iso camera looks at the cut face at y = 15.
    s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    for (mode, c) in capturers() {
        for fov in [0.0, 0.6] {
            let mut cam = view::home_camera(&s);
            cam.fov = fov;
            let count = |on_top: bool| {
                // The session's section (`scene_with_guides` takes its own). The line is 2 µm on
                // the kept side: exactly on the plane, rounding cuts parts of it away with the rest.
                let mut sc = view::scene(&s, &cam);
                sc.lines.push(red_line(Vec3::new(5.0, 15.002, 10.0), Vec3::new(35.0, 15.002, 10.0), on_top));
                pixels(&c.rgba(&sc, &cam, 300, 220).unwrap()).filter(|p| red(*p)).count()
            };
            let (shown, all) = (count(false), count(true));
            assert!(all > 50, "{mode}: the line itself ({all} px)");
            assert!(shown * 10 >= all * 9, "{mode} depth, fov {fov}: {shown} of {all} px drawn over the cap");
        }
    }
}

/// A post standing on a plate, cut through by a section: the post's cut is its own (blue) cap
/// down to the plate, whichever body came first. The post's bottom, seen through the cut, and the
/// plate's top are the same plane; the plate's top showed through in patches, and with the post
/// made first its cap took the plate's colour (the plate's bottom lies behind it).
#[test]
fn a_body_standing_on_another_keeps_its_own_cap() {
    let plate = ("solid.box", json!({"length": 80, "width": 50, "height": 10, "body_name": "Plate"}));
    let post = ("solid.cylinder", json!({"radius": 12, "height": 40, "base": [20, 25, 10], "body_name": "Post"}));
    let colours = [
        ("appearance.assign", json!({"bodies": ["Plate"], "appearance": "ABS - Red"})),
        ("appearance.assign", json!({"bodies": ["Post"], "appearance": "ABS - Blue"})),
        ("inspect.section", json!({"plane": {"origin": [0, 25, 0], "normal": [0, -1, 0]}})),
    ];
    for post_first in [false, true] {
        let mut script = if post_first { vec![post.clone(), plate.clone()] } else { vec![plate.clone(), post.clone()] };
        script.extend(colours.iter().cloned());
        let s = session(&script);
        for (mode, c) in capturers() {
            for fov in [0.0, 0.6] {
                let mut cam = view::home_camera(&s);
                cam.fov = fov;
                // The session's section (`scene_with_guides` takes its own).
                let scene = view::scene(&s, &cam);
                let img = c.rgba(&scene, &cam, 400, 400).unwrap();
                let vp = cam.view_proj(1.0, scene.radius);
                // The post's cut, 1–4 mm above the plate (a pixel is about 0.4 mm).
                let mut wrong = Vec::new();
                for x in (10..=30).map(f64::from) {
                    for z in [11.0, 12.0, 13.0, 14.0] {
                        let q = vp.apply(Vec3::new(x, 25.0, z));
                        let (px, py) = (((q[0] / q[3] + 1.0) * 200.0) as usize, ((1.0 - q[1] / q[3]) * 200.0) as usize);
                        let p = pixels(&img).nth(py * 400 + px).unwrap();
                        if i16::from(p[2]) <= i16::from(p[0]) + 25 {
                            wrong.push(((x, z), p));
                        }
                    }
                }
                assert!(wrong.is_empty(), "{mode} depth, fov {fov}, post first {post_first}: not the post's cap at {wrong:?}");
            }
        }
    }
}
