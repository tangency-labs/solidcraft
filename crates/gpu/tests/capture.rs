//! Headless pictures drawn with the viewport's pipelines: what they show (appearances, guides,
//! silhouettes, the section and its cap, surface analysis) and that bad requests are errors.
//! GPU output differs a little between drivers, so the checks count pixels of a kind rather than
//! compare bytes. Skipped, with the reason printed, on a machine without a GPU adapter
//! (`SOLIDCRAFT_REQUIRE_GPU=1` makes that a failure).

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::{Camera, Rgb, SceneLine};
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
    let cam = view::home_camera(&s);
    let img = c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid: false, axes: false }), &cam, 160, 100).unwrap();
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
    let cam = view::home_camera(&s);
    let img = c.rgba(&view::scene(&s, &cam), &cam, 200, 140).unwrap();
    assert_eq!(img.len(), 200 * 140 * 4);
    let green = pixels(&img).filter(|p| p[1] > 120 && p[0] < 90 && p[2] < 110).count();
    let red = pixels(&img).filter(|p| p[0] > 150 && p[1] < 90 && p[2] < 80).count();
    assert!(green > 100 && red > 100, "green top {green}, red cylinder {red} pixels");
    assert!(pixels(&img).all(|p| p[3] == 255), "the picture is opaque");
}

/// The ground grid and the origin's axes each change the picture.
#[test]
fn guides_change_the_picture() {
    let Some(c) = capturer() else { return };
    let s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [10, -15, -10]}))]);
    let cam = view::home_camera(&s);
    let picture = |grid, axes| c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid, axes }), &cam, 160, 100).unwrap();
    let (all, no_grid, no_axes, bare) = (picture(true, true), picture(false, true), picture(true, false), picture(false, false));
    assert!(all != no_grid && all != no_axes && no_grid != bare && no_axes != bare);
}

/// Curved surfaces get a dark silhouette where they turn away (the viewport's outline).
#[test]
fn pictures_draw_silhouettes_on_curved_surfaces() {
    let Some(c) = capturer() else { return };
    let s = session(&[("solid.sphere", json!({"diameter": 20}))]);
    let cam = view::home_camera(&s);
    let img = c.rgba(&view::scene_with_guides(&s, &cam, None, Guides { grid: false, axes: false }), &cam, 400, 400).unwrap();
    let dark = pixels(&img).filter(|p| p[0] < 90 && p[1] < 90 && p[2] < 95).count();
    assert!(dark > 40, "silhouette pixels: {dark}");
}

/// A section shows: the cut part is gone, the cut face hides what lies inside the body behind
/// it and shows in the cap colour.
#[test]
fn pictures_show_the_section_and_its_cap() {
    let Some(c) = capturer() else { return };
    let mut s = session(&[("solid.box", json!({"length": 40, "width": 30, "height": 20}))]);
    let cam = view::home_camera(&s);
    let whole = c.rgba(&view::scene(&s, &cam), &cam, 200, 140).unwrap();
    // Cut away the front half (y < 15), so the iso camera looks into the cut.
    s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}})).unwrap();
    let cut = c.rgba(&view::scene(&s, &cam), &cam, 200, 140).unwrap();
    let changed = pixels(&whole).zip(pixels(&cut)).filter(|(a, b)| a != b).count();
    assert!(changed > 500, "{changed} pixels changed");
    for fov in [0.0, 0.6] {
        let mut cam = view::home_camera(&s);
        cam.fov = fov;
        let mut sc = view::scene(&s, &cam);
        // A marker inside the body, behind the cut face.
        let red = Rgb(255, 0, 0);
        sc.lines.push(SceneLine { points: vec![Vec3::new(5.0, 25.0, 10.0), Vec3::new(35.0, 25.0, 10.0)], color: red, width: 3.0, on_top: false });
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
    let cam = view::home_camera(&s);
    let mut picture = |hatch: bool| {
        s.execute("inspect.section", &json!({"plane": {"origin": [0, 15, 0], "normal": [0, -1, 0]}, "hatch": hatch})).unwrap();
        c.rgba(&view::scene(&s, &cam), &cam, 300, 220).unwrap()
    };
    let solid = |img: &[u8]| pixels(img).filter(|p| p[..3].iter().zip([40u8, 160, 90]).all(|(a, b)| a.abs_diff(b) <= 3)).count();
    let (hatched, plain) = (picture(true), picture(false));
    assert!(solid(&plain) > 500, "plain cut: {} pixels in the body's colour", solid(&plain));
    assert!(solid(&hatched) < 50, "the hatched cut is tinted: {}", solid(&hatched));
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
    let cam = view::home_camera(&s);
    let picture = |s: &Session| c.rgba(&view::scene_with_guides(s, &cam, None, Guides { grid: false, axes: false }), &cam, 320, 200).unwrap();
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
    assert!(c.rgba(&sc, &Camera::default(), 8, 8).is_ok());
}
