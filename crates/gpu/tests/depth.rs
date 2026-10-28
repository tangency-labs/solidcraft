//! Depth in headless pictures, drawn with the viewport's pipelines (#1434, #1739): an edge behind
//! a face stays hidden however large the design and however far the camera, and whatever lies on
//! a face (its edges, a thin label) draws over it at any angle, with a section shown or not, and a
//! section's cap hides what lies behind it (#6328). Each check runs with the depth the
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
    c.rgba(&view::scene_with_guides(s, cam, None, Guides { grid: false, axes: false }).unwrap(), cam, w, h).unwrap()
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
        let radius = view::scene(&with, &cam).unwrap().radius;
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
/// `section`: with a section that cuts nothing shown, so the section's own passes draw the faces.
fn front(mesh: Mesh, lines: Vec<SceneLine>, radius: f64, fov: f64, section: bool) -> (Scene, Camera) {
    let clip = section.then_some((Vec3::X, 1e3));
    let scene = Scene { meshes: vec![SceneMesh::solid(Arc::new(mesh), Rgb(160, 165, 175))], lines, radius, clip, ..Default::default() };
    let mut cam = Camera { distance: if fov > 0.0 { 25.0 / (fov * 0.5).tan() } else { 62.5 }, fov, ..Default::default() };
    cam.set_view(StandardView::Front);
    (scene, cam)
}

fn red_line(a: Vec3, b: Vec3, on_top: bool) -> SceneLine {
    SceneLine { points: vec![a, b], color: Rgb(255, 0, 0), alpha: 1.0, width: 1.2, on_top }
}

fn red_pixels(c: &Capturer, scene: &Scene, cam: &Camera) -> usize {
    pixels(&c.rgba(scene, cam, 200, 200).unwrap()).filter(|p| red(*p)).count()
}

/// An edge lying on a face draws its whole width over it, on a face seen square and on one turned
/// to 80°, in orthographic and perspective, in a small design and a large one, with a section shown
/// or not (#6328: the section's marking pass draws the faces unpushed, then pushes them).
#[test]
fn an_edge_on_a_face_draws_whole_at_any_angle() {
    for (mode, c) in capturers() {
        for tilt in [0.0_f64, 60.0, 80.0] {
            let t = tilt.to_radians().tan();
            // Across the sheet, at z = 5: y = 5·tan(tilt).
            let (a, b) = (Vec3::new(-12.0, 5.0 * t, 5.0), Vec3::new(12.0, 5.0 * t, 5.0));
            for radius in [30.0, 17_000.0] {
                for (fov, section) in [0.0, 0.6].into_iter().flat_map(|fov| [(fov, false), (fov, true)]) {
                    let (on, cam) = front(sheet(tilt.to_radians()), vec![red_line(a, b, false)], radius, fov, section);
                    let (top, _) = front(sheet(tilt.to_radians()), vec![red_line(a, b, true)], radius, fov, section);
                    let (shown, all) = (red_pixels(c, &on, &cam), red_pixels(c, &top, &cam));
                    assert!(all > 50, "{mode}: the line itself ({all} px)");
                    assert!(
                        shown * 10 >= all * 9,
                        "{mode} depth, tilt {tilt}°, radius {radius}, fov {fov}, section {section}: {shown} of {all} px drawn"
                    );
                }
            }
        }
    }
}

/// A selected edge's highlight (a 3 px core in a 6 px halo, drawn by `GpuScene::lifted_line`
/// in the highlight scene) lying on a face seen almost edge on draws its whole width: the face
/// no longer buries part of the band, which showed as a thin wedge instead of a line. The same
/// band 5 mm behind a face seen square, beside faces seen square, stays hidden.
#[test]
fn a_selected_edge_on_a_grazing_face_draws_whole() {
    let highlight = |a: Vec3, b: Vec3, n: Vec3| {
        let mut sc = solidcraft_gpu::view::GpuScene::default();
        let red = sc.color([255, 0, 0, 255]);
        sc.lifted_line(a.to_f32(), b.to_f32(), red, 6.0, [n.to_f32(), n.to_f32()]);
        sc
    };
    let reference = |a: Vec3, b: Vec3| vec![SceneLine { points: vec![a, b], color: Rgb(255, 0, 0), alpha: 1.0, width: 6.0, on_top: true }];
    for (mode, c) in capturers() {
        for tilt in [60.0_f64, 80.0, 85.0, 88.0] {
            let tr = tilt.to_radians();
            let t = tr.tan();
            let n = Vec3::new(0.0, -tr.cos(), tr.sin());
            let (a, b) = (Vec3::new(-12.0, 5.0 * t, 5.0), Vec3::new(12.0, 5.0 * t, 5.0));
            for radius in [30.0, 17_000.0] {
                for fov in [0.0, 0.6] {
                    let (plain, cam) = front(sheet(tr), Vec::new(), radius, fov, false);
                    let (top, _) = front(sheet(tr), reference(a, b), radius, fov, false);
                    let shown = pixels(&c.rgba_highlighted(&plain, Some(highlight(a, b, n)), &cam, 200, 200).unwrap()).filter(|p| red(*p)).count();
                    let all = red_pixels(c, &top, &cam);
                    assert!(all > 100, "{mode}: the band itself ({all} px)");
                    assert!(shown * 10 >= all * 9, "{mode} depth, tilt {tilt}°, radius {radius}, fov {fov}: {shown} of {all} px drawn");
                }
            }
        }
        for radius in [30.0, 17_000.0] {
            for fov in [0.0, 0.6] {
                let (a, b) = (Vec3::new(-12.0, 5.0, 5.0), Vec3::new(12.0, 5.0, 5.0));
                let (plain, cam) = front(sheet(0.0), Vec::new(), radius, fov, false);
                let img = c.rgba_highlighted(&plain, Some(highlight(a, b, Vec3::new(0.0, -1.0, 0.0))), &cam, 200, 200).unwrap();
                let n = pixels(&img).filter(|p| red(*p)).count();
                assert_eq!(n, 0, "{mode} depth, radius {radius}, fov {fov}: the band behind shows in {n} px");
            }
        }
    }
}

/// An edge 0.5 mm behind a face seen square is hidden, however large the design, with a section
/// shown or not: lines were pulled toward the eye by a share of the depth range, about 27 mm in a
/// design 34 m across.
#[test]
fn an_edge_just_behind_a_face_stays_hidden_on_a_large_design() {
    for (mode, c) in capturers() {
        for radius in [30.0, 17_000.0] {
            for (fov, section) in [0.0, 0.6].into_iter().flat_map(|fov| [(fov, false), (fov, true)]) {
                // The sheet is at y = 0; the camera looks along +y, so behind is +y.
                let (a, b) = (Vec3::new(-12.0, 0.5, 5.0), Vec3::new(12.0, 0.5, 5.0));
                let (scene, cam) = front(sheet(0.0), vec![red_line(a, b, false)], radius, fov, section);
                let n = red_pixels(c, &scene, &cam);
                assert_eq!(n, 0, "{mode} depth, radius {radius}, fov {fov}, section {section}: the edge behind shows in {n} px");
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
            let mut cam = view::home_camera(&s).unwrap();
            cam.fov = fov;
            let count = |on_top: bool| {
                // The session's section (`scene_with_guides` takes its own). The line is 2 µm on
                // the kept side: exactly on the plane, rounding cuts parts of it away with the rest.
                let mut sc = view::scene(&s, &cam).unwrap();
                sc.lines.push(red_line(Vec3::new(5.0, 15.002, 10.0), Vec3::new(35.0, 15.002, 10.0), on_top));
                pixels(&c.rgba(&sc, &cam, 300, 220).unwrap()).filter(|p| red(*p)).count()
            };
            let (shown, all) = (count(false), count(true));
            assert!(all > 50, "{mode}: the line itself ({all} px)");
            assert!(shown * 10 >= all * 9, "{mode} depth, fov {fov}: {shown} of {all} px drawn over the cap");
        }
    }
}

/// A cut body's cap covers the body it touches behind the cut. A block whose far face (y = 30)
/// lies on a larger plate's front face, cut at y = 15: through the cut the block's far wall and
/// the plate's face are at one depth. When the cap went to the nearer of the two by depth, the
/// plate showed through 2721 of its 4350 px here (an engine's sleeves in their bores, pins in
/// their bosses); counted by crossings, the cap is the same with the plate as without it. An
/// open body behind it (a box without its far face, whose near face alone the ray crosses)
/// takes nothing from the cap either: only closed bodies are counted.
#[test]
fn a_cap_covers_the_body_its_cut_body_touches() {
    let block = ("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "block"}));
    let plate = ("solid.box", json!({"length": 70, "width": 10, "height": 50, "corner": [-15, 30, -15], "operation": "new", "body_name": "plate"}));
    let shell = ("solid.box", json!({"length": 70, "width": 10, "height": 50, "corner": [-15, 35, -15], "operation": "new", "body_name": "shell"}));
    let far_face = ("face.delete", json!({"faces": [[20, 45, 10]], "heal": false}));
    let cut = ("inspect.section", json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}}));
    let alone = session(&[block.clone(), cut.clone()]);
    let touching = session(&[block.clone(), plate, cut.clone()]);
    let open = session(&[block, shell, far_face, cut]);
    assert!(open.world_state().unwrap().body("shell").unwrap().body.is_surface());
    let (w, h) = (360, 280);
    for (mode, c) in capturers() {
        for (what, other) in [("the touching plate", &touching), ("an open shell behind", &open)] {
            for fov in [0.0, 0.6] {
                let mut cam = view::home_camera(&touching).unwrap();
                (cam.fov, cam.yaw, cam.pitch) = (fov, -0.3, 0.2);
                let radius = view::scene(&touching, &cam).unwrap().radius;
                let shot = |s: &Session| {
                    c.rgba(&Scene { grid: Vec::new(), lines: Vec::new(), radius, ..view::scene(s, &cam).unwrap() }, &cam, w, h).unwrap()
                };
                let (a, b) = (shot(&alone), shot(other));
                // The cap on screen: the cut's rectangle (x 0..40, z 0..20 at y = 15), 2 px in.
                let corner = |x: f64, z: f64| cam.to_screen(Vec3::new(x, 15.0, z), f64::from(w), f64::from(h), radius).map(|(x, y, _)| (x, y));
                let cap: Vec<(f64, f64)> = [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)].iter().filter_map(|(x, z)| corner(*x, *z)).collect();
                assert_eq!(cap.len(), 4, "fov {fov}: the cut is on screen");
                let turn = (0..4).map(|i| cap[i].0 * cap[(i + 1) % 4].1 - cap[(i + 1) % 4].0 * cap[i].1).sum::<f64>().signum();
                let inside = |x: f64, y: f64| {
                    (0..4).all(|i| {
                        let ((ax, ay), (bx, by)) = (cap[i], cap[(i + 1) % 4]);
                        ((bx - ax) * (y - ay) - (by - ay) * (x - ax)) / (bx - ax).hypot(by - ay) * turn >= 2.0
                    })
                };
                let (mut shown, mut all) = (0, 0);
                for (i, (p, q)) in pixels(&a).zip(pixels(&b)).enumerate() {
                    if inside((i % w as usize) as f64 + 0.5, (i / w as usize) as f64 + 0.5) {
                        all += 1;
                        shown += usize::from(p.iter().zip(q).any(|(u, v)| u.abs_diff(v) > 24));
                    }
                }
                assert!(all > 3000, "{mode}, fov {fov}: the cap ({all} px)");
                assert_eq!(shown, 0, "{mode} depth, fov {fov}: {what} shows through {shown} of {all} cap px");
            }
        }
    }
}

/// Pixels of the model's edges lying behind the section plane (recoloured red) that draw over
/// the cap: the cap's pixels are where the picture without lines shows the hatch (warm, with
/// warm pixels 3 px around, so the cut's outline doesn't count).
fn edges_through_the_cap(c: &Capturer, s: &Session, cam: &Camera, (origin, normal): (Vec3, Vec3), (w, h): (u32, u32)) -> usize {
    let mut sc = view::scene(s, cam).unwrap();
    let behind = |p: &Vec3| (*p - origin).dot(normal) < -0.5;
    let segments = sc
        .lines
        .iter()
        .flat_map(|l| l.points.windows(2).filter(|w| w.iter().all(behind)).map(|w| SceneLine { points: w.to_vec(), color: Rgb(255, 0, 0), ..*l }))
        .collect();
    sc.lines = segments;
    let with = c.rgba(&sc, cam, w, h).unwrap();
    sc.lines.clear();
    let without = c.rgba(&sc, cam, w, h).unwrap();
    let warm = |x: i64, y: i64| {
        let at = (y * i64::from(w) + x) as usize * 4;
        without.get(at..at + 3).is_some_and(|p| p[0] > 60 && p[0] > p[2].saturating_add(15))
    };
    let mut n = 0;
    for y in 3..i64::from(h) - 3 {
        for x in 3..i64::from(w) - 3 {
            let at = (y * i64::from(w) + x) as usize * 4;
            let cap = [(0, 0), (3, 0), (-3, 0), (0, 3), (0, -3)].iter().all(|(dx, dy)| warm(x + dx, y + dy));
            // Redder than the cap there: a red line, whole or blended at its sides.
            let redness = |img: &[u8]| img.get(at..at + 3).map_or(0, |p| i32::from(p[0]) - i32::from(p[1]));
            if cap && redness(&with) > redness(&without) + 30 {
                n += 1;
            }
        }
    }
    n
}

/// The cap hides the edges inside a cut body (#6328). Where a face seen through the cut meets a
/// face turned toward the eye along an edge (a pocket's front wall and its side wall or floor),
/// the faces' push left the cap out beside the edge, and the edge showed through it. A pocket
/// whose front wall is 2 mm behind the cut, in perspective and orthographic.
#[test]
fn edges_of_a_pocket_behind_the_cut_stay_under_the_cap() {
    let mut s = session(&[
        ("solid.box", json!({"length": 40, "width": 40, "height": 30, "corner": [-20, -20, 0]})),
        ("solid.box", json!({"length": 16, "width": 16, "height": 18, "corner": [-14, -14, 12], "operation": "cut"})),
    ]);
    let plane = (Vec3::new(0.0, -16.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
    s.execute("inspect.section", &json!({"plane": {"origin": [0, -16, 0], "normal": [0, -1, 0]}})).unwrap();
    for (mode, c) in capturers() {
        for fov in [0.0, 0.6] {
            let mut cam = view::home_camera(&s).unwrap();
            cam.fov = fov;
            let n = edges_through_the_cap(c, &s, &cam, plane, (360, 300));
            assert!(n <= 2, "{mode} depth, fov {fov}: the pocket's edges show through the cap in {n} px");
        }
    }
}

/// The same while the view turns: a filleted hub with a bore and a cross hole, cut through the
/// middle, seen from twelve directions. Its fillets' and holes' edges came and went through the
/// cap from one step to the next.
#[test]
fn edges_of_a_cut_hub_stay_under_the_cap_as_the_view_turns() {
    let mut s = session(&[
        ("solid.cylinder", json!({"radius": 20, "height": 40, "body_name": "hub"})),
        ("solid.fillet", json!({"edges": [[20, 0, 40], [20, 0, 0]], "radius": 4})),
        ("solid.cylinder", json!({"radius": 8, "height": 30, "base": [0, 0, 10], "operation": "cut"})),
        ("solid.cylinder", json!({"radius": 3, "height": 50, "base": [-25, 0, 22], "axis": "X", "operation": "cut"})),
        ("solid.fillet", json!({"edges": [[8, 0, 40]], "radius": 1.5})),
    ]);
    let plane = (Vec3::new(0.0, -2.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
    s.execute("inspect.section", &json!({"plane": {"origin": [0, -2, 0], "normal": [0, -1, 0]}})).unwrap();
    for (mode, c) in capturers() {
        for fov in [0.0, 0.6] {
            let mut shown = Vec::new();
            for i in 0..12 {
                let mut cam = view::home_camera(&s).unwrap();
                (cam.fov, cam.yaw, cam.pitch) = (fov, -1.2 + 0.075 * f64::from(i), 0.35);
                shown.push(edges_through_the_cap(c, &s, &cam, plane, (900, 760)));
            }
            let total: usize = shown.iter().sum();
            assert!(total <= 6, "{mode} depth, fov {fov}: edges show through the cap, px per view {shown:?}");
        }
    }
}

/// #6328, bherbruck's block: 40 × 40 × 30 mm with a hole and a pocket, cut on XZ at y = -16, 2 mm in
/// front of the pocket and 12 mm in front of the hole, seen from the home view. The cap hides them:
/// it matches the plain block's cap. With the faces' slope push on the section's marking pass
/// (#1768), the pocket's and the hole's edges showed through it as dashed lines in perspective, 84
/// of the 18,328 cap pixels here (both depths); before #1768 and with the fix, none. Away from the
/// cut the plain block's picture is a box of just its kept part's: the push also let back faces win
/// a band along the far edges, which the cap hatched (58 px here in orthographic, 534 in
/// perspective; now 0 and 1). The limits leave room for an edge's exact tie, which rounding
/// settles per sample (one pixel on WARP).
#[test]
fn a_cut_hides_the_edges_behind_it() {
    let block = [
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.line", json!({"points": [[-20, -20], [20, -20], [20, 20], [-20, 20]], "closed": true})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 30, "body_name": "Block"})),
    ];
    let behind = [
        ("sketch.create", json!({"plane": "XY", "name": "Hole"})),
        ("sketch.circle.center", json!({"center": [10, 0], "diameter": 8})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 30, "operation": "cut"})),
        ("sketch.create", json!({"plane": "XY", "name": "Pocket", "offset": 30})),
        ("sketch.line", json!({"points": [[-14, -14], [2, -14], [2, 2], [-14, 2]], "closed": true})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 18, "direction": "negative", "operation": "cut"})),
    ];
    let cut = ("inspect.section", json!({"plane": "XZ", "at": -16, "flip": true}));
    let plain = session(&[&block[..], std::slice::from_ref(&cut)].concat());
    let cut_through = session(&[&block[..], &behind[..], &[cut]].concat());
    let (w, h) = (400, 300);
    for (mode, c) in capturers() {
        for fov in [0.0, 0.6] {
            let mut cam = view::home_camera(&plain).unwrap();
            cam.fov = fov;
            let radius = view::scene(&plain, &cam).unwrap().radius;
            // The cap on screen: the cut's rectangle, its outline left out (2 px in).
            let corner = |x: f64, z: f64| cam.to_screen(Vec3::new(x, -16.0, z), f64::from(w), f64::from(h), radius).map(|(x, y, _)| (x, y));
            let cap: Vec<(f64, f64)> = [(-20.0, 0.0), (20.0, 0.0), (20.0, 30.0), (-20.0, 30.0)].iter().filter_map(|(x, z)| corner(*x, *z)).collect();
            assert_eq!(cap.len(), 4, "fov {fov}: the cut is on screen");
            // Which way the corners run on screen, so a point's distance inside each edge is positive.
            let turn = (0..4).map(|i| cap[i].0 * cap[(i + 1) % 4].1 - cap[(i + 1) % 4].0 * cap[i].1).sum::<f64>().signum();
            let inside_by = |x: f64, y: f64, by: f64| {
                let side = |i: usize| {
                    let ((ax, ay), (bx, by)) = (cap[i], cap[(i + 1) % 4]);
                    ((bx - ax) * (y - ay) - (by - ay) * (x - ax)) / (bx - ax).hypot(by - ay)
                };
                (0..4).all(|i| side(i) * turn >= by)
            };
            let inside = |x: f64, y: f64| inside_by(x, y, 2.0);
            // Without the ground grid, whose extent follows the design.
            let shot = |s: &Session| c.rgba(&Scene { grid: Vec::new(), ..view::scene(s, &cam).unwrap() }, &cam, w, h).unwrap();
            let (a, b) = (shot(&plain), shot(&cut_through));
            let (mut shown, mut all) = (0, 0);
            for (i, (p, q)) in pixels(&a).zip(pixels(&b)).enumerate() {
                if inside((i % w as usize) as f64 + 0.5, (i / w as usize) as f64 + 0.5) {
                    all += 1;
                    shown += usize::from(p.iter().zip(q).any(|(u, v)| u.abs_diff(v) > 24));
                }
            }
            assert!(all > 5000, "{mode}, fov {fov}: the cap ({all} px)");
            assert!(shown <= 8, "{mode} depth, fov {fov}: {shown} of {all} cap px show what lies behind the cut");
            // Away from the cut, the picture is a box of just the kept part's (no cap along the far
            // edges either).
            let kept = shot(&session(&[("solid.box", json!({"length": 40, "width": 36, "height": 30, "corner": [-20, -16, 0]}))]));
            let fringe = pixels(&a)
                .zip(pixels(&kept))
                .enumerate()
                .filter(|(i, (p, q))| {
                    !inside_by((i % w as usize) as f64 + 0.5, (i / w as usize) as f64 + 0.5, -8.0)
                        && p.iter().zip(q).any(|(u, v)| u.abs_diff(*v) > 24)
                })
                .count();
            assert!(fringe <= 8, "{mode} depth, fov {fov}: {fringe} px away from the cut differ from the kept part alone");
        }
    }
}
