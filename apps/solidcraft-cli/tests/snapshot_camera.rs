//! `snapshot` frames its picture like the MCP `screenshot`: from a camera, a view saved in the
//! design or one the script applies itself, and without the ground grid and the origin's axes
//! when asked (the grid lies on Z 0 and cuts through a part that straddles it). Pictures are drawn
//! on the GPU and compared with a tolerance; without a GPU adapter the tests that draw are skipped
//! with the reason printed (`SOLIDCRAFT_REQUIRE_GPU=1` makes that a failure).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_mcp::{Headless, Server};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Whether pictures can be drawn here; installs the renderer the CLI installs, so the MCP
/// `screenshot` in this process draws as `solidcraft-cli mcp` does.
fn gpu() -> bool {
    match solidcraft_gpu::capture::shared() {
        Ok(c) => {
            solidcraft_engine::capture::install(std::sync::Arc::new(move |scene, cam, w, h| {
                c.rgba(scene, cam, w, h).map(|rgba| solidcraft_engine::capture::Picture { rgba, renderer: c.renderer() }).map_err(|e| e.to_string())
            }));
            true
        }
        Err(e @ solidcraft_gpu::GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            false
        }
        Err(e) => panic!("{e}"),
    }
}

/// A PNG's size and RGBA pixels.
fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap().to_rgba8();
    (img.width(), img.height(), img.into_raw())
}

/// Pixels of two same-sized pictures that differ by more than a little in some channel.
fn differing(a: &[u8], b: &[u8]) -> usize {
    let ((wa, ha, a), (wb, hb, b)) = (decode(a), decode(b));
    assert_eq!((wa, ha), (wb, hb), "pictures of different sizes");
    a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 16)).count()
}

/// The same picture: GPU output may differ by a few levels or a stray edge pixel between drivers.
#[track_caller]
fn assert_same(a: &[u8], b: &[u8], what: &str) {
    let (n, total) = (differing(a, b), (SIZE.0 * SIZE.1) as usize);
    assert!(n * 500 <= total, "{what}: {n} of {total} pixels differ");
}

/// Different pictures: more than stray pixels differ.
#[track_caller]
fn assert_differs(a: &[u8], b: &[u8], what: &str) {
    let n = differing(a, b);
    assert!(n >= 20, "{what}: only {n} pixels differ");
}

const SIZE: (u32, u32) = (160, 100);

fn camera() -> Value {
    json!({"target": [20, 0, 0], "yaw": 0.6, "pitch": 0.25, "distance": 100})
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-snapshot-camera-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    /// A box that straddles Z 0 and keeps clear of the origin (its axes show beside it), with a view
    /// saved in the design, plus `more` steps.
    fn script(&self, name: &str, more: &[Value]) -> PathBuf {
        let mut steps = vec![
            json!({"command": "solid.box", "params": {"length": 60, "width": 40, "height": 30, "corner": [10, -20, -15]}}),
            json!({"command": "view.save", "params": {"name": "Detail", "camera": camera()}}),
        ];
        steps.extend(more.iter().cloned());
        let path = self.0.join(name);
        std::fs::write(&path, json!({"commands": steps}).to_string()).unwrap();
        path
    }

    fn run(&self, script: &Path, out: &str, extra: &[&str]) -> Output {
        let (w, h) = (SIZE.0.to_string(), SIZE.1.to_string());
        Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
            .arg("snapshot")
            .arg(script)
            .arg("--out")
            .arg(self.0.join(out))
            .args(["--width", &w, "--height", &h])
            .args(extra)
            .output()
            .unwrap()
    }

    /// The PNG `snapshot` writes.
    fn snapshot(&self, script: &Path, out: &str, extra: &[&str]) -> Vec<u8> {
        let output = self.run(script, out, extra);
        assert!(output.status.success(), "{extra:?}: {}", String::from_utf8_lossy(&output.stderr));
        let png = std::fs::read(self.0.join(out)).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{extra:?}");
        png
    }

    /// The PNG the MCP `screenshot` tool writes for the same design.
    fn mcp(&self, script: &Path, out: &str, arguments: Value) -> Vec<u8> {
        let mut session = Session::default();
        session.run_script(&serde_json::from_slice(&std::fs::read(script).unwrap()).unwrap()).unwrap();
        let mut server = Server::new(Box::new(Headless::new(session)));
        let mut arguments = arguments;
        arguments["path"] = json!(self.0.join(out).to_string_lossy());
        arguments["width"] = json!(SIZE.0);
        arguments["height"] = json!(SIZE.1);
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "screenshot", "arguments": arguments}});
        let reply: Value = serde_json::from_str(&server.handle_line(&call.to_string()).unwrap()).unwrap();
        assert!(reply.get("error").is_none() && reply["result"]["isError"] != json!(true), "{reply}");
        std::fs::read(self.0.join(out)).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The picture of `--camera` is the picture of the MCP `screenshot` with the same camera, and
/// not the home view the option used to be ignored for.
#[test]
fn a_camera_frames_the_picture_as_the_mcp_screenshot_does() {
    if !gpu() {
        return;
    }
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let home = dir.snapshot(&script, "home.png", &[]);
    let json = camera().to_string();
    let shot = dir.snapshot(&script, "camera.png", &["--camera", &json]);
    assert_differs(&shot, &home, "--camera was ignored");
    assert_same(&shot, &dir.mcp(&script, "mcp-camera.png", json!({"camera": camera()})), "the MCP screenshot");
    // A perspective camera too, and a different eye gives a different picture.
    let mut near = camera();
    near["fov"] = json!(0.6);
    let perspective = dir.snapshot(&script, "perspective.png", &["--camera", &near.to_string()]);
    assert_differs(&perspective, &shot, "perspective");
    assert_same(&perspective, &dir.mcp(&script, "mcp-perspective.png", json!({"camera": near})), "the MCP perspective screenshot");
}

/// `--view` also takes a view saved in the design, which is the camera it was saved with; a
/// standard view still looks as it did.
#[test]
fn a_saved_view_is_its_camera_and_a_standard_view_is_unchanged() {
    if !gpu() {
        return;
    }
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let json = camera().to_string();
    let by_camera = dir.snapshot(&script, "camera.png", &["--camera", &json]);
    let saved = dir.snapshot(&script, "saved.png", &["--view", "Detail"]);
    assert_same(&saved, &by_camera, "the saved view");
    assert_same(&saved, &dir.mcp(&script, "mcp-saved.png", json!({"view": "Detail"})), "the MCP saved view");
    // A standard view frames the model from its direction, as before: the home camera turned.
    let top = dir.snapshot(&script, "top.png", &["--view", "top"]);
    let mut s = Session::default();
    s.run_script(&serde_json::from_slice(&std::fs::read(&script).unwrap()).unwrap()).unwrap();
    let mut cam = solidcraft_engine::view::home_camera(&s).unwrap();
    cam.set_view(solidcraft_engine::render::StandardView::Top);
    let scene = solidcraft_engine::view::scene(&s, &cam).unwrap();
    assert_same(&top, &solidcraft_engine::capture::png(&scene, &cam, SIZE.0, SIZE.1).unwrap().bytes, "the home camera turned to the top");
    assert_same(&top, &dir.mcp(&script, "mcp-top.png", json!({"view": "top"})), "the MCP top view");
    assert_differs(&top, &saved, "top and the saved view");
    let iso = dir.snapshot(&script, "iso.png", &["--view", "iso"]);
    assert_same(&iso, &dir.snapshot(&script, "home.png", &[]), "iso is home");
    assert_differs(&iso, &top, "iso and top");
}

/// A script that applies a view itself (as an MCP `execute` of `view.apply` does) sets the camera
/// of its snapshot; `--view` and `--camera` still decide when given.
#[test]
fn a_script_can_carry_its_camera() {
    if !gpu() {
        return;
    }
    let dir = Scratch::new();
    let script = dir.script("carried.json", &[json!({"command": "view.apply", "params": {"camera": camera()}})]);
    let plain = dir.script("plain.json", &[]);
    let carried = dir.snapshot(&script, "carried.png", &[]);
    assert_same(&carried, &dir.snapshot(&plain, "by-camera.png", &["--camera", &camera().to_string()]), "the carried camera");
    assert_differs(&carried, &dir.snapshot(&plain, "home.png", &[]), "the carried camera and home");
    let top = dir.snapshot(&script, "top.png", &["--view", "top"]);
    assert_same(&top, &dir.snapshot(&plain, "plain-top.png", &["--view", "top"]), "--view decides");
}

/// The grid and the axes each go when asked, and a render without both from any side shows the
/// model alone: an empty design is then the same blank picture from every camera.
#[test]
fn the_grid_and_the_axes_can_be_left_out() {
    if !gpu() {
        return;
    }
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let json = camera().to_string();
    for view in [&["--view", "front"][..], &["--camera", &json][..]] {
        let with = |extra: &[&str], out: &str| dir.snapshot(&script, out, &[view, extra].concat());
        let (all, no_grid, no_axes, bare) = (
            with(&[], "all.png"),
            with(&["--no-grid"], "no-grid.png"),
            with(&["--no-axes"], "no-axes.png"),
            with(&["--no-grid", "--no-axes"], "bare.png"),
        );
        let different = [&all, &no_grid, &no_axes, &bare];
        for (i, a) in different.iter().enumerate() {
            for b in &different[i + 1..] {
                assert_differs(a, b, &format!("{view:?}: two of the pictures"));
            }
        }
        // The same as the MCP's `grid` and `axes`.
        let mcp = |extra: Value, out: &str| {
            let mut a = match view[0] {
                "--view" => json!({"view": "front"}),
                _ => json!({"camera": camera()}),
            };
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            dir.mcp(&script, out, a)
        };
        assert_same(&no_grid, &mcp(json!({"grid": false}), "mcp-no-grid.png"), "the MCP's grid: false");
        assert_same(&no_axes, &mcp(json!({"axes": false}), "mcp-no-axes.png"), "the MCP's axes: false");
        assert_same(&bare, &mcp(json!({"grid": false, "axes": false}), "mcp-bare.png"), "the MCP's bare picture");
    }
    let empty = dir.0.join("empty.json");
    std::fs::write(&empty, json!({"commands": []}).to_string()).unwrap();
    let blank = |view: &str, out: &str| dir.snapshot(&empty, out, &["--view", view, "--no-grid", "--no-axes"]);
    assert_same(&blank("top", "blank-top.png"), &blank("front", "blank-front.png"), "blank from every side");
    assert_differs(&blank("top", "blank-top.png"), &dir.snapshot(&empty, "top.png", &["--view", "top"]), "blank and the grid");
}

/// A snapshot of a box shows its silhouette where the camera puts it: the pixels that differ
/// from an empty design's picture (same camera, no grid or axes) span the box's corners projected
/// to pixels, within the edge and silhouette lines' width.
#[test]
fn a_snapshot_shows_the_part_where_the_camera_puts_it() {
    if !gpu() {
        return;
    }
    let dir = Scratch::new();
    let part = dir.0.join("box.json");
    let empty = dir.0.join("empty.json");
    let corner = [10.0, -20.0, -15.0];
    let size = [60.0, 40.0, 30.0];
    std::fs::write(
        &part,
        json!({"commands": [{"command": "solid.box", "params": {"length": size[0], "width": size[1], "height": size[2], "corner": corner}}]})
            .to_string(),
    )
    .unwrap();
    std::fs::write(&empty, json!({"commands": []}).to_string()).unwrap();
    for cam in [
        camera(),
        json!({"target": [40, 0, 0], "yaw": 0.0, "pitch": 0.0, "distance": 160}),
        json!({"target": [40, 0, 0], "yaw": -0.4, "pitch": 0.5, "distance": 150, "fov": 0.6}),
    ] {
        let args = ["--camera", &cam.to_string(), "--no-grid", "--no-axes"];
        let (_, _, with) = decode(&dir.snapshot(&part, "part.png", &args));
        let (_, _, without) = decode(&dir.snapshot(&empty, "empty.png", &args));
        // The part's pixels: where the picture differs from the empty one.
        let (mut lo, mut hi) = ([u32::MAX; 2], [0u32; 2]);
        for (i, (p, q)) in with.as_chunks::<4>().0.iter().zip(without.as_chunks::<4>().0).enumerate() {
            if p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 24) {
                let (x, y) = (i as u32 % SIZE.0, i as u32 / SIZE.0);
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x), hi[1].max(y)];
            }
        }
        assert!(lo[0] <= hi[0], "{cam}: the part is not in the picture");
        // Where the camera puts the corners.
        let mut s = solidcraft_engine::Session::default();
        s.execute("view.apply", &json!({"camera": cam})).unwrap();
        let c = s.camera.unwrap();
        let vp = c.view_proj(f64::from(SIZE.0) / f64::from(SIZE.1), 1000.0);
        let (mut elo, mut ehi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for k in 0..8 {
            let p = solidcraft_engine::geom::Vec3::new(
                corner[0] + size[0] * f64::from(k & 1),
                corner[1] + size[1] * f64::from((k >> 1) & 1),
                corner[2] + size[2] * f64::from((k >> 2) & 1),
            );
            let q = vp.apply(p);
            let (x, y) = ((q[0] / q[3] + 1.0) * 0.5 * f64::from(SIZE.0), (1.0 - q[1] / q[3]) * 0.5 * f64::from(SIZE.1));
            elo = [elo[0].min(x), elo[1].min(y)];
            ehi = [ehi[0].max(x), ehi[1].max(y)];
        }
        let near = |got: u32, want: f64| (f64::from(got) - want).abs() <= 2.5;
        assert!(
            near(lo[0], elo[0]) && near(lo[1], elo[1]) && near(hi[0] + 1, ehi[0]) && near(hi[1] + 1, ehi[1]),
            "{cam}: the part spans pixels {lo:?}..={hi:?}, its corners project to {elo:?}..{ehi:?}"
        );
    }
}

/// `snapshot` under a `SOLIDCRAFT_GPU` policy: its reply and stderr.
fn snapshot_under(dir: &Scratch, script: &Path, out: &str, policy: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .env("SOLIDCRAFT_GPU", policy)
        .arg("snapshot")
        .arg(script)
        .arg("--out")
        .arg(dir.0.join(out))
        .args(["--width", &SIZE.0.to_string(), "--height", &SIZE.1.to_string(), "--view", "top"])
        .output()
        .unwrap()
}

/// The last tier: `SOLIDCRAFT_GPU=cpu` draws with the CPU rasterizer whatever adapters there are,
/// says so (`renderer: cpu` and a warning that overlays are not drawn), and the picture is the
/// rasterizer's own.
#[test]
fn a_forced_cpu_snapshot_is_the_cpu_rasterizers_and_says_so() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let output = snapshot_under(&dir, &script, "cpu.png", "cpu");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["renderer"], "cpu", "{reply}");
    assert!(reply["warning"].as_str().is_some_and(|w| w.contains("not drawn")), "{reply}");
    assert!(stderr.contains("CPU rasterizer"), "{stderr}");
    let mut s = Session::default();
    s.run_script(&serde_json::from_slice(&std::fs::read(&script).unwrap()).unwrap()).unwrap();
    let mut cam = solidcraft_engine::view::home_camera(&s).unwrap();
    cam.set_view(solidcraft_engine::render::StandardView::Top);
    let scene = solidcraft_engine::view::scene(&s, &cam).unwrap();
    let cpu = solidcraft_engine::render::render_png(&scene, &cam, SIZE.0 as usize, SIZE.1 as usize).unwrap();
    assert_eq!(std::fs::read(dir.0.join("cpu.png")).unwrap(), cpu);
}

/// `hardware` (and `software`) never fall to the CPU rasterizer: without such an adapter the
/// snapshot fails, naming the policy, and writes nothing; with one it says it was drawn on the GPU.
/// `auto` lands where `solidcraft-cli gpu` says it does.
#[test]
fn a_strict_policy_never_falls_to_the_cpu() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let survey = |policy: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["gpu", "--policy", policy]).output().unwrap();
        (o.status.success(), serde_json::from_slice::<Value>(&o.stdout).unwrap_or(Value::Null))
    };
    for policy in ["hardware", "software"] {
        let (found, report) = survey(policy);
        let output = snapshot_under(&dir, &script, &format!("{policy}.png"), policy);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if found {
            assert!(output.status.success(), "{policy}: {stderr}");
            let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(reply["renderer"], report["renderer"], "{policy}: {reply}");
            assert_ne!(reply["renderer"], "cpu");
        } else {
            assert_eq!(report["lands_on"], Value::Null, "{policy}: {report}");
            assert_eq!(output.status.code(), Some(1), "{policy}: {stderr}");
            assert!(stderr.contains(&format!("`{policy}` policy")) && stderr.contains("solidcraft-cli gpu"), "{policy}: {stderr}");
            assert!(!dir.0.join(format!("{policy}.png")).exists());
        }
    }
    let (found, report) = survey("auto");
    assert!(found, "auto always lands somewhere: {report}");
    let output = snapshot_under(&dir, &script, "auto.png", "auto");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["renderer"], report["renderer"], "{reply} vs {report}");
    let (_, cpu) = survey("cpu");
    assert_eq!((cpu["lands_on"].clone(), cpu["renderer"].clone()), (json!("cpu"), json!("cpu")), "{cpu}");
}

#[test]
fn a_view_or_a_camera_that_cannot_be_used_is_an_error_and_writes_nothing() {
    let dir = Scratch::new();
    let script = dir.script("part.json", &[]);
    let cases: [(&[&str], &str); 7] = [
        (&["--view", "Nowhere"], "Nowhere"),
        (&["--view", "iso", "--camera", "{}"], "not both"),
        (&["--camera", "not json"], "--camera"),
        (&["--camera", "{\"target\": [0, 0, 0]}"], "yaw"),
        (&["--camera", "{\"target\": [0, 0, 0], \"yaw\": 0, \"pitch\": 0, \"distance\": 0}"], "distance"),
        (&["--camera", "{\"target\": [0, 0, 0], \"yaw\": 0, \"pitch\": 0, \"distance\": 50, \"fov\": -1}"], "camera"),
        (&["--camera", "[1, 2, 3]"], "camera"),
    ];
    for (extra, expect) in cases {
        let output = dir.run(&script, "never.png", extra);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{extra:?}: {stderr}");
        assert!(stderr.contains(expect), "{extra:?}: {stderr}");
        assert!(output.stdout.is_empty() && !dir.0.join("never.png").exists(), "{extra:?}");
    }
}

/// The usage text says what the new options do.
#[test]
fn the_usage_names_the_camera_and_the_guides() {
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("snapshot").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    for word in ["--camera", "--view", "view.save", "--no-grid", "--no-axes"] {
        assert!(stderr.contains(word), "usage lacks {word}: {stderr}");
    }
}
