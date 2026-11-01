//! Motion animations (`motion.export_animation`): a study played, one joint driven, or an exploded
//! view coming apart (and back together), drawn frame by frame by the headless renderer the host
//! installed ([`crate::capture`]) and written as an animated GIF or a folder of PNG frames. Sampled
//! on a copy, as a trace is: the design is not changed, nor its placements.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use solidcraft_doc::Mat;
use solidcraft_geom::Vec3;

use super::{Plan, drive_with_contact, explode_moves, exploded_at, motion_plan, sample_count, view_index};
use crate::capture::{self, Renderer};
use crate::params::{bad, str_};
use crate::render::Camera;
use crate::{Result, Session};

/// What the frames show.
enum Motion {
    /// A study played or a joint driven: the values driven, and what they are at each frame.
    Drive(BTreeSet<(u64, usize)>, Plan),
    /// An exploded view's moves coming apart, and back together when true.
    Explode(Vec<(u64, Mat, Vec3)>, bool),
}

/// Most frames of one animation.
const MAX_FRAMES: usize = 1000;
/// Largest frame side, in pixels.
const MAX_SIDE: u32 = 4096;
/// Most pixels of a GIF's frames together: they are held in memory (4 bytes each) until the
/// palette is chosen, so 256 MB at most.
const MAX_GIF_PIXELS: usize = 1 << 26;
/// Most pixels of a PNG folder's frames together (each frame is written as it is drawn): about
/// 500 frames of 1920 × 1080.
const MAX_PNG_PIXELS: usize = 1 << 30;
/// The camera fields `camera` may set: those of `ui.camera`'s reply.
const FIELDS: [&str; 7] = ["target", "yaw", "pitch", "roll", "distance", "fov", "up"];

pub(super) fn export(s: &mut Session, p: &Value) -> Result<Value> {
    export_with(s, p, capture::installed().as_deref())
}

/// [`export`] drawn by `renderer`.
pub(super) fn export_with(s: &Session, p: &Value, renderer: Option<&Renderer>) -> Result<Value> {
    let cmd = "motion.export_animation";
    let path = str_(p, "path")
        .filter(|x| !x.trim().is_empty() && x.len() <= 4096 && !x.chars().any(char::is_control))
        .ok_or_else(|| bad(cmd, "`path` must name a .gif file, or a folder for PNG frames"))?;
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        return Err(bad(cmd, "`path` is a .gif file or a folder for the PNG frames (frame-0001.png, …), not one .png"));
    }
    let gif = lower.ends_with(".gif");
    // In the browser written files stay in memory until the host takes them, and the host is not
    // told of removed ones: a folder of frames would pile up there.
    if !gif && cfg!(target_arch = "wasm32") {
        return Err(bad(cmd, "a folder of PNG frames is not available in the browser: give a .gif path"));
    }
    let side = |key: &str, default: u32| match p.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite() && (16.0..=f64::from(MAX_SIDE)).contains(n))
            .map(|n| n.round() as u32)
            .ok_or_else(|| bad(cmd, format!("`{key}` must be 16…{MAX_SIDE} pixels"))),
    };
    let (width, height) = (side("width", 800)?, side("height", 500)?);
    let fps = match p.get("fps") {
        None => 20.0,
        Some(v) => v.as_f64().filter(|n| n.is_finite() && (1.0..=50.0).contains(n)).ok_or_else(|| bad(cmd, "`fps` must be 1…50"))?,
    };
    let flag = |key: &str, default: bool| match p.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    };
    let guides = crate::view::Guides { grid: flag("grid", true)?, axes: flag("axes", true)? };
    let (default, motion) = match (p.get("view"), p.get("study").or(p.get("joint"))) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give an exploded `view`, a `study` or a `joint`: one of them")),
        (Some(_), None) => {
            let back = flag("back", false)?;
            let v = s.doc.assembly.exploded.get(view_index(s, p, cmd)?).ok_or_else(|| bad(cmd, "view"))?;
            (if back { 81 } else { 41 }, Motion::Explode(explode_moves(s, v), back))
        }
        (None, driven) => {
            if driven.is_none() {
                return Err(bad(cmd, "give a `study`, a `joint` with `from` and `to`, or an exploded `view`"));
            }
            if p.get("back").is_some() {
                return Err(bad(cmd, "`back` goes with an exploded `view`"));
            }
            let (default, given, plan) = motion_plan(s, p, cmd)?;
            (default, Motion::Drive(given, plan))
        }
    };
    let home = || match &motion {
        // Framing both ends: every frame lies between the parts assembled and exploded.
        Motion::Explode(moves, _) => {
            let mut t = s.scratch();
            t.pending_moves.clear();
            let assembled = crate::view::bounds(&t);
            t.pending_moves = exploded_at(moves, 1.0);
            let mut c = crate::view::home_camera(&t);
            c.fit(&assembled.union(&crate::view::bounds(&t)));
            c
        }
        Motion::Drive(..) => crate::view::home_camera(s),
    };
    let cam = camera(s, home, p.get("camera"), cmd)?;
    // At least 4 frames fit even at the largest size.
    let max = MAX_FRAMES.min(if gif { MAX_GIF_PIXELS } else { MAX_PNG_PIXELS } / (width as usize * height as usize));
    let n = sample_count(p, default.min(max), max, cmd).map_err(|_| bad(cmd, format!("`samples` must be 2…{max} for {width} × {height} frames")))?;
    let renderer = renderer.ok_or_else(|| bad(cmd, "no renderer is installed to draw pictures in this process"))?;
    // A destination outside an agent's folders is refused before anything is drawn.
    let io = |e: std::io::Error| bad(cmd, format!("{path}: {e}"));
    solidcraft_io::vfs::check(path).map_err(io)?;
    let frame_path = |k: usize| std::path::Path::new(path).join(format!("frame-{k:04}.png")).to_string_lossy().to_string();
    if !gif {
        solidcraft_io::vfs::create_dir_all(path).map_err(io)?;
        // The frames of an earlier export here go first, so the folder never mixes two
        // animations: not after a shorter one, nor when this one fails part way.
        for stale in (1..=MAX_FRAMES).map(frame_path).filter(|f| solidcraft_io::vfs::exists(f)) {
            solidcraft_io::vfs::remove(&stale).map_err(|e| bad(cmd, format!("{stale}: {e}")))?;
        }
    }
    let mut t = s.scratch();
    // From where the parts are placed: moves only shown (an exploded view, the pose an open
    // Motion Study dialog shows) are not where the joints are solved, nor the assembled placement.
    t.pending_moves.clear();
    let (mut frames, mut bytes, mut drawn_by) = (Vec::new(), 0, "");
    let mut stop = (Value::Null, Value::Null, Value::Null);
    for k in 0..n {
        match &motion {
            Motion::Drive(given, plan) => {
                let want = plan(&t, k, n);
                let (r, _) = drive_with_contact(&mut t, &want, given)?;
                if stop.0.is_null() && (r["stopped_by_loop"].is_object() || r["stopped_by_contact"].is_object()) {
                    stop = (json!(k), r["stopped_by_loop"].clone(), r["stopped_by_contact"].clone());
                }
            }
            // Shown as explode.show shows it, the way out at a steady pace (and back).
            Motion::Explode(moves, back) => {
                let u = k as f64 / (n - 1) as f64;
                t.pending_moves = exploded_at(moves, if *back { 1.0 - (2.0 * u - 1.0).abs() } else { u });
            }
        }
        let scene = crate::view::scene_with_guides(&t, &cam, &s.section, guides);
        let picture = capture::rgba_with(Some(renderer), &scene, &cam, width, height).map_err(|e| bad(cmd, e))?;
        drawn_by = picture.renderer;
        if gif {
            frames.push(picture.rgba);
        } else {
            let png = crate::render::png(width as usize, height as usize, picture.rgba).ok_or_else(|| bad(cmd, "PNG encoding failed"))?;
            let file = frame_path(k + 1);
            solidcraft_io::vfs::write(&file, &png).map_err(|e| bad(cmd, format!("{file}: {e}")))?;
            bytes += png.len();
        }
    }
    let mut out = json!({"path": path, "format": if gif { "gif" } else { "png" }, "frames": n, "width": width, "height": height});
    if gif {
        // GIF counts in hundredths of a second, and viewers slow anything under 2 down.
        let delay = (100.0 / fps).round().clamp(2.0, 100.0) as u16;
        let gif = crate::render::gif(width as usize, height as usize, &frames, delay).ok_or_else(|| bad(cmd, "GIF encoding failed"))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), &gif, false).map_err(io)?;
        bytes = gif.len();
        out["fps"] = json!(100.0 / f64::from(delay));
    }
    out["bytes"] = json!(bytes);
    out["renderer"] = json!(drawn_by);
    if let Some(w) = capture::warning(drawn_by) {
        out["warning"] = json!(w);
    }
    out["stopped_at"] = stop.0;
    out["stopped_by_loop"] = stop.1;
    out["stopped_by_contact"] = stop.2;
    Ok(out)
}

/// The camera the frames are drawn from: the session's (`view.apply`), or `home` framing the
/// model, with the fields `given` sets (`ui.camera`'s reply sets them all).
fn camera(s: &Session, home: impl FnOnce() -> Camera, given: Option<&Value>, cmd: &str) -> Result<Camera> {
    let base = s.camera.unwrap_or_else(home);
    let Some(given) = given else { return Ok(base) };
    let fields = given.as_object().ok_or_else(|| bad(cmd, "`camera` must be an object, such as ui.camera's reply"))?;
    let mut v = serde_json::to_value(base).map_err(|e| bad(cmd, e.to_string()))?;
    for (k, x) in fields {
        if !FIELDS.contains(&k.as_str()) {
            return Err(bad(cmd, format!("unknown camera field `{k}` (it takes {})", FIELDS.join(", "))));
        }
        if let Some(o) = v.as_object_mut() {
            o.insert(k.clone(), x.clone());
        }
    }
    let mut cam: Camera = serde_json::from_value(v).map_err(|e| bad(cmd, format!("bad camera: {e}")))?;
    if !cam.is_valid() || cam.distance > 1e15 || cam.pitch.abs() > std::f64::consts::FRAC_PI_2 || !(0.0..3.0).contains(&cam.fov) {
        return Err(bad(
            cmd,
            "the camera must be finite, with a distance up to 1e15 mm, a pitch within ±π/2 and a fov of 0 (orthographic) to 3 radians",
        ));
    }
    // Straight up or down keeps a heading, as ui.camera does.
    let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
    cam.pitch = cam.pitch.clamp(-lim, lim);
    Ok(cam)
}
