//! Headless pictures drawn with the viewport's pipelines: what they show (appearances, guides,
//! silhouettes, the section and its cap, surface analysis) and that bad requests are errors.
//! GPU output differs a little between drivers, so the checks count pixels of a kind rather than
//! compare bytes. Skipped, with the reason printed, on a machine without a GPU adapter
//! (`SOLIDCRAFT_REQUIRE_GPU=1` makes that a failure).

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::{Camera, Rgb, Scene, SceneLine};
use solidcraft_engine::view::{self, Guides, colors};
use solidcraft_gpu::GpuError;
use solidcraft_gpu::capture::{self, Capturer};

fn capturer() -> Option<&'static Capturer> {
    match capture::shared() {
        Ok(c) => {
            eprintln!("GPU: {} ({}x MSAA)", c.adapter_name(), c.samples());
            Some(c)
        }
        Err(e @ GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// RGBA pixels as `[r, g, b, a]`.
fn pixels(img: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    img.as_chunks::<4>().0.iter().copied()
}

fn session(script: &[(&str, serde_json::Value)]) -> Session {
    let mut s = Session::default();
    for (c, p) in script {
        s.execute(c, p).unwrap();
    }
    s
}

/// #2888: compiling the viewport on GL used to reject AO's depth textureLoad, even
/// with AO disabled. Select GL explicitly so a working Vulkan adapter cannot hide it.
#[test]
fn opengl_viewport_pipelines_render_a_model() {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::GL;
    // NVIDIA's EGL answers wgpu's request for GL 3.3 with exactly 3.3 yet lists
    // GL_ARB_compute_shader, so wgpu builds its indirect-call validation pipeline, naga cannot
    // emit compute GLSL below 4.30, and request_device fails as Device(Lost). The test draws
    // nothing indirect, so the validation is not needed here.
    desc.flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    let instance = wgpu::Instance::new(desc);
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(e) => {
            let required = std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_some() && wgpu::Backends::from_env() == Some(wgpu::Backends::GL);
            assert!(!required, "OpenGL is required here: {e}");
            eprintln!("skipping the OpenGL regression: {e}");
            return;
        }
    };
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Gl);
    eprintln!("OpenGL regression: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: solidcraft_gpu::depth_features(&adapter),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .unwrap();
    let c = Capturer::new(&adapter, device, queue).unwrap();
    let s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]);
    let cam = view::home_camera(&s).unwrap();
    let img = c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid: false, axes: false }).unwrap(), &cam, 160, 100).unwrap();
    let empty = c.rgba(&Default::default(), &cam, 160, 100).unwrap();
    let changed = pixels(&img).zip(pixels(&empty)).filter(|(a, b)| a != b).count();
    assert!(changed > 500, "the model covers only {changed} pixels");
}

/// Appearances show as the viewport shows them: a body's colour, a face's own look.
#[test]
fn pictures_show_appearances() {
    let Some(c) = capturer() else { return };
    let s = session(&[
        ("solid.box", json!({"length": 40, "width": 30, "height": 20})),
        ("solid.cylinder", json!({"diameter": 14, "height": 30, "base": [50, 15, 0], "operation": "new"})),
        ("appearance.assign", json!({"bodies": ["Body2"], "color": [220, 60, 40]})),
        ("appearance.assign", json!({"faces": [[20, 15, 20]], "body": "Body1", "color": [40, 170, 80]})),
    ]);
    let cam = view::home_camera(&s).unwrap();
    let img = c.rgba(&view::scene(&s, &cam).unwrap(), &cam, 200, 140).unwrap();
    assert_eq!(img.len(), 200 * 140 * 4);
    let green = pixels(&img).filter(|p| p[1] > 120 && p[0] < 90 && p[2] < 110).count();
    let red = pixels(&img).filter(|p| p[0] > 150 && p[1] < 90 && p[2] < 80).count();
    assert!(green > 100 && red > 100, "green top {green}, red cylinder {red} pixels");
    assert!(pixels(&img).all(|p| p[3] == 255), "the picture is opaque");
}

/// The renderer counts what it draws (the performance overlay, #2548): every mesh triangle and
/// edge segment of the scene, more with more geometry, nothing with nothing to draw.
#[test]
fn the_renderer_counts_what_it_draws() {
    let Some(c) = capturer() else { return };
    let drawn = |s: &Session| {
        let cam = view::home_camera(s).unwrap();
        let scene = view::scene_with_guides(s, &cam, None, Guides { grid: false, axes: false }).unwrap();
        let triangles: usize = scene.meshes.iter().map(|m| m.mesh.triangles.len()).sum();
        let (_, stats) = c.rgba_with_stats(&scene, &cam, 160, 100).unwrap();
        (stats, triangles as u64)
    };
    let (empty, _) = drawn(&Session::default());
    assert_eq!((empty.triangles, empty.line_segments), (0, 0), "{empty:?}");
    let (one, one_mesh) = drawn(&session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]));
    assert!(one_mesh >= 12 && one.triangles >= one_mesh, "{one:?} for {one_mesh} mesh triangles");
    assert!(one.line_segments >= 12 && one.draw_calls > empty.draw_calls, "{one:?}");
    let (two, two_mesh) = drawn(&session(&[
        ("solid.box", json!({"length": 40, "width": 30, "height": 20})),
        ("solid.cylinder", json!({"diameter": 14, "height": 30, "base": [50, 15, 0], "operation": "new"})),
    ]));
    assert!(two_mesh > one_mesh && two.triangles >= two_mesh && two.triangles > one.triangles, "{two:?} for {two_mesh}");
    assert!(two.line_segments > one.line_segments, "{two:?}");
}

/// Each picture's counts are its own while other threads draw on the same capturer: an empty
/// design never reports another thread's box, and a box never reports an empty design's
/// nothing. The counts were once read in a second call after the picture, and another thread's
/// picture could land in between.
#[test]
fn pictures_drawn_at_once_each_count_their_own() {
    let Some(c) = capturer() else { return };
    let scene = |s: &Session| {
        let cam = view::home_camera(s);
        (view::scene_with_guides(s, &cam, None, Guides { grid: false, axes: false }), cam)
    };
    let empty = scene(&Session::default());
    let solid = scene(&session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]));
    std::thread::scope(|t| {
        for k in 0..8 {
            let (name, (sc, cam), drawn) = if k % 2 == 0 { ("empty", &empty, false) } else { ("box", &solid, true) };
            t.spawn(move || {
                for i in 0..50 {
                    let (_, stats) = c.rgba_with_stats(sc, cam, 64, 48).unwrap();
                    assert_eq!(stats.triangles > 0, drawn, "{name}, picture {i}: {stats:?}");
                }
            });
        }
    });
}

/// The ground grid and the origin's axes each change the picture.
#[test]
fn guides_change_the_picture() {
    let Some(c) = capturer() else { return };
    let s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [10, -15, -10]}))]);
    let cam = view::home_camera(&s).unwrap();
    let picture = |grid, axes| c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid, axes }).unwrap(), &cam, 160, 100).unwrap();
    let (all, no_grid, no_axes, bare) = (picture(true, true), picture(false, true), picture(true, false), picture(false, false));
    assert!(all != no_grid && all != no_axes && no_grid != bare && no_axes != bare);
}

/// Curved surfaces get a dark silhouette where they turn away (the viewport's outline).
#[test]
fn pictures_draw_silhouettes_on_curved_surfaces() {
    let Some(c) = capturer() else { return };
    let s = session(&[("solid.sphere", json!({"diameter": 20}))]);
    let cam = view::home_camera(&s).unwrap();
    let img = c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid: false, axes: false }).unwrap(), &cam, 400, 400).unwrap();
    let dark = pixels(&img).filter(|p| p[0] < 90 && p[1] < 90 && p[2] < 95).count();
    assert!(dark > 40, "silhouette pixels: {dark}");
}

/// A section shows: the cut part is gone, the cut face hides what lies inside the body behind
/// it and shows in the cap colour.
#[test]
fn pictures_show_the_section_and_its_cap() {
    let Some(c) = capturer() else { return };
    let mut s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]);
    let cam = view::home_camera(&s).unwrap();
    let whole = c.rgba(&view::scene(&s, &cam).unwrap(), &cam, 200, 140).unwrap();
    // Cut away the front half (y < 15), so the iso camera looks into the cut.
    s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    let cut = c.rgba(&view::scene(&s, &cam).unwrap(), &cam, 200, 140).unwrap();
    let changed = pixels(&whole).zip(pixels(&cut)).filter(|(a, b)| a != b).count();
    assert!(changed > 500, "{changed} pixels changed");
    for fov in [0.0, 0.6] {
        let mut cam = view::home_camera(&s).unwrap();
        cam.fov = fov;
        let mut sc = view::scene(&s, &cam).unwrap();
        // The cap in the cap colour: a part's hatch colour is the one its name picks (#8529).
        sc.part_hatch = None;
        // A marker inside the body, behind the cut face.
        let red = Rgb(255, 0, 0);
        sc.lines.push(SceneLine {
            points: vec![Vec3::new(5.0, 25.0, 10.0), Vec3::new(35.0, 25.0, 10.0)],
            color: red,
            alpha: 1.0,
            width: 3.0,
            on_top: false,
        });
        let img = c.rgba(&sc, &cam, 300, 220).unwrap();
        let marker = pixels(&img).filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60).count();
        assert_eq!(marker, 0, "fov {fov}: the inside shows through the cut face");
        // The cap: warm (red over blue), unlike the grey-blue body and background.
        let cap = pixels(&img).filter(|p| i32::from(p[0]) - i32::from(p[2]) > 20).count();
        assert!(cap > 500, "fov {fov}: cap pixels {cap}");
    }
}

/// With the section's hatch off, the cut shows plain in its body's own colour, as the CPU
/// rasterizer draws it; hatched, it is tinted and striped.
#[test]
fn pictures_show_a_plain_cut_without_the_hatch() {
    let Some(c) = capturer() else { return };
    let mut s = session(&[
        ("solid.box", json!({"length": 40, "width": 30, "height": 20})),
        ("appearance.assign", json!({"bodies": ["Body1"], "color": [40, 160, 90]})),
    ]);
    let cam = view::home_camera(&s).unwrap();
    let mut picture = |hatch: bool| {
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}, "hatch": hatch})).unwrap();
        c.rgba(&view::scene(&s, &cam).unwrap(), &cam, 300, 220).unwrap()
    };
    let solid = |img: &[u8]| pixels(img).filter(|p| p[..3].iter().zip([40u8, 160, 90]).all(|(a, b)| a.abs_diff(b) <= 3)).count();
    let (hatched, plain) = (picture(true), picture(false));
    assert!(solid(&plain) > 500, "plain cut: {} pixels in the body's colour", solid(&plain));
    assert!(solid(&hatched) < 50, "the hatched cut is tinted: {}", solid(&hatched));
}

/// A body inside another's hollow, behind the cut, leaves no trace on the cap (#6328): the cap
/// used to be painted by every face behind it, last drawn last, so the inner body's colour and
/// outline showed through the cut like an X-ray. Hatched and plain, orthographic and
/// perspective, the picture is the same with the inner body as without it.
#[test]
fn a_body_behind_the_cut_does_not_colour_the_cap() {
    let Some(c) = capturer() else { return };
    let outer = [
        ("solid.box", json!({"length": 40, "width": 40, "height": 30, "body_name": "outer"})),
        ("solid.box", json!({"length": 20, "width": 20, "height": 16, "corner": [10, 10, 7], "operation": "cut"})),
        ("appearance.assign", json!({"bodies": ["outer"], "color": [40, 160, 90]})),
    ];
    let inner = [
        ("solid.box", json!({"length": 8, "width": 8, "height": 6, "corner": [16, 16, 12], "operation": "new_body", "body_name": "inner"})),
        ("appearance.assign", json!({"bodies": ["inner"], "color": [220, 30, 30]})),
    ];
    let mut alone = session(&outer);
    let mut with = session(&[outer[0].clone(), outer[1].clone(), outer[2].clone(), inner[0].clone(), inner[1].clone()]);
    for hatch in [true, false] {
        // Cut away y < 6: the hollow (and the inner body) lie 4 mm behind the cut face.
        for s in [&mut alone, &mut with] {
            s.execute("inspect.section", &json!({"plane": {"origin": [0, 6, 0], "normal": [0, -1, 0]}, "hatch": hatch})).unwrap();
        }
        for fov in [0.0, 0.6] {
            let mut cam = view::home_camera(&alone).unwrap();
            cam.fov = fov;
            let picture = |s: &Session| c.rgba(&view::scene(s, &cam).unwrap(), &cam, 300, 220).unwrap();
            let (a, b) = (picture(&alone), picture(&with));
            let differ = pixels(&a).zip(pixels(&b)).filter(|(p, q)| p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > 8)).count();
            assert_eq!(differ, 0, "hatch {hatch}, fov {fov}: the inner body shows through the cap in {differ} pixels");
        }
    }
}

/// #4214: a line that is not opaque is blended with what is under it, so the grid's fade reaches
/// the picture.
#[test]
fn pictures_blend_a_line_that_is_not_opaque() {
    let Some(c) = capturer() else { return };
    let cam = Camera { distance: 50.0, fov: 0.0, ..Default::default() };
    let darkest = |alpha: f32| {
        let mut sc = Scene { radius: 30.0, ..Default::default() };
        sc.lines.push(SceneLine {
            points: vec![Vec3::new(-8.0, 2.0, 0.0), Vec3::new(8.0, 2.0, 0.0)],
            color: Rgb(0, 0, 0),
            alpha,
            width: 1.4,
            on_top: false,
        });
        let img = c.rgba(&sc, &cam, 64, 64).unwrap();
        pixels(&img).map(|p| i32::from(p[0])).min().unwrap_or(255)
    };
    let solid = darkest(1.0);
    let half = darkest(0.5);
    assert!(solid < 40, "the opaque line is black: {solid}");
    assert!(half > 60 && half < 170, "the half-opaque line is between the background and the line: {half} against {solid}");
}

/// Surface analysis shades headless pictures as it shades the viewport: a box's flat faces in
/// the curvature map's flat colour, zebra stripes in black and white, the accessibility analysis
/// in reachable and blocked colours.
#[test]
fn pictures_show_the_surface_analysis() {
    let Some(c) = capturer() else { return };
    let mut s = session(&[
        ("solid.box", json!({"length": 60, "width": 40, "height": 20})),
        ("solid.hole", json!({"position": [30, 20, 20], "diameter": 10, "depth": 20})),
    ]);
    let cam = view::home_camera(&s).unwrap();
    let picture =
        |s: &Session| c.rgba(&view::scene_with_guides(s, &cam, None, Guides { grid: false, axes: false }).unwrap(), &cam, 320, 200).unwrap();
    let plain = picture(&s);
    s.execute("inspect.curvature_map", &json!({})).unwrap();
    let curvature = picture(&s);
    let near = |p: [u8; 4], c: [u8; 3]| (0..3).all(|k| (i32::from(p[k]) - i32::from(c[k])).abs() < 60);
    let flat = solidcraft_engine::render::AnalysisColors::DEFAULT.curvature[0].map(|v| (v * 255.0).round() as u8);
    let flats = |img: &[u8]| pixels(img).filter(|p| near(*p, flat)).count();
    // The flat colour is dark, so the plain picture's edge lines come near it in places.
    assert!(
        flats(&curvature) > 3000 && flats(&plain) * 10 < flats(&curvature),
        "flat faces in the flat colour: {} (plain {})",
        flats(&curvature),
        flats(&plain)
    );
    s.execute("inspect.zebra", &json!({})).unwrap();
    let zebra = picture(&s);
    let black = pixels(&zebra).filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40).count();
    let white = pixels(&zebra).filter(|p| p[0] > 225 && p[1] > 225 && p[2] > 225).count();
    assert!(black > 500 && white > 500, "zebra: {black} black, {white} white pixels");
    s.execute("inspect.accessibility", &json!({})).unwrap();
    let access = picture(&s);
    let [ok, blocked] = colors_of(solidcraft_engine::render::AnalysisColors::DEFAULT.access);
    let (reachable, under) = (pixels(&access).filter(|p| near(*p, ok)).count(), pixels(&access).filter(|p| near(*p, blocked)).count());
    assert!(reachable > 1000 && under > 1000, "accessibility: {reachable} reachable, {under} blocked pixels");
}

/// A flat face of a box keeps its zebra colour while the view turns a little at a time (#2153):
/// a horizontal face reflected straight ahead, on the edge between a black and a white stripe,
/// and rounding flipped it from one step to the next.
#[test]
fn zebra_on_flat_faces_holds_still_while_the_view_turns() {
    let Some(c) = capturer() else { return };
    let mut s = session(&[("solid.box", json!({"length": 60, "width": 40, "height": 20}))]);
    s.execute("inspect.zebra", &json!({})).unwrap();
    let home = view::home_camera(&s);
    let whites: Vec<usize> = (0..24)
        .map(|step| {
            let mut cam = home;
            cam.yaw += f64::from(step) * 0.11_f64.to_radians();
            let img = c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid: false, axes: false }), &cam, 320, 200).unwrap();
            pixels(&img).filter(|p| p[0] > 225 && p[1] > 225 && p[2] > 225).count()
        })
        .collect();
    let (least, most) = (whites.iter().min().copied().unwrap_or(0), whites.iter().max().copied().unwrap_or(0));
    assert!(most > 1000, "white stripes: {whites:?}");
    assert!((most - least) * 10 < most, "white pixels jump while the view turns by 0.11 degree steps: {whites:?}");
}

fn colors_of(c: [[f32; 3]; 2]) -> [[u8; 3]; 2] {
    c.map(|rgb| rgb.map(|x| (x * 255.0).round() as u8))
}

/// An empty scene is the background gradient: the top colour at the top, the bottom one at the
/// bottom.
#[test]
fn an_empty_picture_is_the_background() {
    let Some(c) = capturer() else { return };
    let sc = solidcraft_engine::render::Scene { background: Some((colors::BG_TOP, colors::BG_BOTTOM)), ..Default::default() };
    let img = c.rgba(&sc, &Camera::default(), 64, 64).unwrap();
    let at = |row: usize| pixels(&img).nth(row * 64 + 32).unwrap_or_default();
    let near = |p: [u8; 4], c: Rgb| [p[0], p[1], p[2]].iter().zip([c.0, c.1, c.2]).all(|(a, b)| a.abs_diff(b) <= 3);
    assert!(near(at(0), colors::BG_TOP) && near(at(63), colors::BG_BOTTOM), "{:?} {:?}", at(0), at(63));
}

/// Sizes outside 1 to the device's limit are errors, not crashes.
#[test]
fn bad_picture_sizes_are_errors() {
    let Some(c) = capturer() else { return };
    let sc = solidcraft_engine::render::Scene::default();
    for (w, h) in [(0, 10), (10, 0), (capture::MAX_SIDE + 1, 10), (u32::MAX, u32::MAX)] {
        assert!(c.rgba(&sc, &Camera::default(), w, h).is_err(), "{w} x {h}");
    }
    // And the device still draws.
    let drawn = c.rgba(&sc, &Camera::default(), 8, 8);
    assert!(drawn.is_ok(), "{:?}", drawn.err());
}

/// Pictures read back while another thread uses their device: the app's window submits its
/// frames on the device its thumbnails and `ui.render` pictures are drawn on, and on the shared
/// capturer every picture checks its device while another one reads back. Any submit or poll can
/// take a picture's finished read-back and answer it on its own thread a moment after the
/// picture's wait is over; the picture used to give up then ("reading a picture back without
/// waiting for the browser", now and then in this binary's other tests).
#[test]
fn pictures_read_back_while_another_thread_uses_the_device() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let policy = solidcraft_gpu::select::Policy::from_env().unwrap();
    let Some((adapter, _)) = solidcraft_gpu::select::pick(policy) else {
        assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "no GPU adapter under the {} policy", policy.name());
        eprintln!("skipping: no GPU adapter under the {} policy", policy.name());
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: solidcraft_gpu::depth_features(&adapter),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .unwrap();
    let c = Capturer::new(&adapter, device.clone(), queue).unwrap();
    let sc = solidcraft_engine::render::Scene::default();
    const PICTURES: usize = 2000;
    let done = AtomicBool::new(false);
    let failed: Vec<String> = std::thread::scope(|t| {
        t.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                let _ = device.poll(wgpu::PollType::Poll);
            }
        });
        let failed = (0..PICTURES).filter_map(|i| c.rgba(&sc, &Camera::default(), 8, 8).err().map(|e| format!("picture {i}: {e}"))).collect();
        done.store(true, Ordering::Relaxed);
        failed
    });
    assert!(failed.is_empty(), "{} of {PICTURES} pictures failed, the first: {:?}", failed.len(), failed.first());
}
