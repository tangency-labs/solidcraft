//! Canvas to Sketch: traces a canvas image into a sketch lying exactly on it.
//!
//! The image is reduced to at most `WORK_RES` pixels, the drawing is told from the background
//! automatically (Otsu threshold; inverted when the border is dark), its outlines are found with
//! interpolated marching squares, simplified, and written as lines (straight runs, with sharp
//! corners rebuilt) and cubic Bézier splines (curved runs). Holes stay holes.
//!
//! A canvas is capped by file size, not by pixel count, so the image's declared width × height
//! is checked before anything is decoded (a small, highly compressible PNG can declare a huge
//! size), and the decoder itself runs under the same limit.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::canvas::{Canvas, image_info};
use solidcraft_geom::prelude::*;
use solidcraft_io::Geom2;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::canvas::{CanvasToSketchParams, CanvasTraced};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const TO_SKETCH_CONTRACT: TypedContract<CanvasToSketchParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("canvas.to_sketch", "Canvas to Sketch", crate::checked!("canvas.to_sketch", to_sketch))
    .begins_edit()
    .icon("canvas")
    .contract(&TO_SKETCH_CONTRACT)
    .output(output_schema_for::<CanvasTraced>)
    .params(concat!(
        "canvas?: id or name (default: the last canvas); name?: the new sketch's name. Traces the canvas image (dark drawing on a light background, ",
        "or the reverse) into a new sketch on the canvas plane, lying exactly on the image: lines, splines and holes. ",
        "at?: [x, y] a point on the canvas plane (mm); given, only the region joined to that point whose colour is within tolerance of the colour ",
        "there is traced; tolerance?: colour difference 0-100 (default 20), with at"
    ))];

const WORK_RES: u32 = 512;
/// The colour tolerance (percent) when `at` is given without one.
const DEFAULT_TOLERANCE: f64 = 20.0;
const MAX_LOOPS: usize = 2000;
/// The largest image traced, in pixels (decoded as RGBA: 160 MB).
const MAX_PIXELS: u64 = 40_000_000;

fn to_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "canvas.to_sketch";
    if s.active_sketch.is_some() {
        return Err(bad(cmd, "finish the sketch being edited first"));
    }
    let canvas = pick(s, p, cmd)?;
    let bytes = canvas.bytes();
    let (_, [iw, ih]) = image_info(bytes).ok_or_else(|| bad(cmd, "only PNG and JPEG canvases can be traced"))?;
    if u64::from(iw) * u64::from(ih) > MAX_PIXELS {
        return Err(bad(
            cmd,
            format!("the canvas image is {iw} x {ih} pixels; tracing reads at most 40 megapixels, so scale the picture down first"),
        ));
    }
    let region = region_params(p, cmd)?;
    let Loaded { w, h, ink, rgb } = load(bytes).ok_or_else(|| bad(cmd, "the canvas image cannot be read"))?;
    // An anchored canvas is traced where its frame shows it: into its sketch's plane, each pixel
    // placed through the frame's corners (stretched, turned and mirrored as shown).
    let anchored = canvas.anchor.as_ref().and_then(|a| {
        let frame = s.doc.canvas_anchor_frame(&canvas, a, &s.model.state()).ok()?;
        match s.doc.feature(a.sketch).map(|f| &f.kind) {
            Some(solidcraft_doc::FeatureKind::Sketch { plane, .. }) => Some((plane.clone(), frame)),
            _ => None,
        }
    });
    // Image pixels -> canvas plane: width as the canvas, centred, turned and mirrored like it.
    let sc = match &anchored {
        Some((_, [bl, br, _, _])) => (*br - *bl).len() / w as f64,
        None => canvas.width / w as f64,
    };
    let (half_w, half_h) = (w as f64 * sc / 2.0, h as f64 * sc / 2.0);
    let (sin, cos) = canvas.angle.sin_cos();
    let (center, flip) = (canvas.center, canvas.flip);
    let frame = anchored.as_ref().map(|(_, k)| *k);
    let (wf, hf) = (w as f64, h as f64);
    // The inverse of `tf`: a point on the canvas plane -> a pixel position of the reduced image.
    let inv = move |q: Vec2| -> Option<(f64, f64)> {
        let (x, y) = if let Some([bl, br, _, tl]) = frame {
            let (e1, e2, d) = (br - bl, tl - bl, q - bl);
            let det = e1.x * e2.y - e1.y * e2.x;
            if det.abs() < 1e-12 {
                return None;
            }
            let (u, v) = ((d.x * e2.y - d.y * e2.x) / det, (e1.x * d.y - e1.y * d.x) / det);
            (u * wf, (1.0 - v) * hf)
        } else {
            if sc <= 0.0 {
                return None;
            }
            let d = q - center;
            let (mut u, v) = (d.x * cos + d.y * sin, -d.x * sin + d.y * cos);
            if flip {
                u = -u;
            }
            ((u + half_w) / sc, (half_h - v) / sc)
        };
        (x.is_finite() && y.is_finite()).then_some((x, y))
    };
    let tf = move |q: Vec2| -> Vec2 {
        if let Some([bl, br, _, tl]) = frame {
            // Pixel (0, h) is the image's bottom-left corner, (w, h) its bottom-right, (0, 0) its top-left.
            let xyz = |p: Vec2| [p.x, p.y, 0.0];
            let [x, y, _] = solidcraft_kernel::affine::parallelogram_point(xyz(bl), xyz(br - bl), xyz(tl - bl), q.x / wf, 1.0 - q.y / hf);
            return Vec2::new(x, y);
        }
        let mut u = q.x * sc - half_w;
        let v = half_h - q.y * sc;
        if flip {
            u = -u;
        }
        Vec2::new(center.x + u * cos - v * sin, center.y + u * sin + v * cos)
    };
    let (ink, threshold) = match region {
        Some((at, tolerance)) => {
            let (px, py) = inv(at).ok_or_else(|| bad(cmd, "`at` is not on the canvas plane's image"))?;
            let (x, y) = (px.floor(), py.floor());
            if !(x >= 0.0 && y >= 0.0 && x < wf && y < hf) {
                return Err(bad(cmd, "`at` is outside the canvas image"));
            }
            let mask = flood(&rgb, w, h, (x as usize, y as usize), tolerance);
            let count = mask.iter().filter(|v| **v > 0.5).count();
            if count == w * h {
                return Err(bad(cmd, "that region is the whole picture; lower the tolerance or pick a spot inside the shape"));
            }
            (mask, 0.5)
        }
        None => {
            let t = otsu(&ink);
            (ink, t)
        }
    };
    let opts = Opts { threshold, ..Opts::default() };
    let loops = trace(&ink, w, h, &opts);
    if loops.is_empty() {
        return Err(bad(
            cmd,
            if p.get("at").is_some_and(|v| !v.is_null()) {
                "no outline found for that region (it is too small; raise the tolerance or pick a larger shape)"
            } else {
                "no outline found in the image (it needs a drawing that stands out from its background)"
            },
        ));
    }
    let straight = opts.straight_min * w.max(h) as f64;
    let mut geom = Vec::new();
    for l in &loops {
        let mut cur = l.points.first().copied().unwrap_or_default();
        for seg in segments(l, &opts, straight) {
            match seg {
                Seg::Line(b) => geom.push(Geom2::Line(tf(cur), tf(b))),
                Seg::Cubic(c1, c2, b) => geom.push(Geom2::Spline { pts: vec![tf(cur), tf(c1), tf(c2), tf(b)], control: true, degree: 3 }),
            }
            cur = seg.end();
        }
    }
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| s.doc.free_sketch_name(&format!("{} sketch", canvas.name)));
    let plane = anchored.map_or_else(|| canvas.plane.clone(), |(plane, _)| plane);
    let r = super::sketch::create_sketch_with_plane(s, &json!({ "name": name }), plane, None)?;
    // `sketch.create` always reports the id of the sketch it made.
    let id = r.get("sketch").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "could not start a sketch on the canvas"))?;
    let tol = (sc * 0.01).max(1e-6);
    let made = super::sketch::edit_solve(s, &json!({}), cmd, false, |sk, _| {
        let (cs, _) = super::sketch_import::add_geometry(sk, &geom, &|q| q, 1.0, tol)?;
        Ok(cs.len())
    });
    let finish = super::find_command("sketch.finish").ok_or_else(|| bad(cmd, "sketch.finish"))?.run;
    let (curves, info) = match made {
        Ok(v) => v,
        Err(e) => {
            let _ = finish(s, &json!({}));
            return Err(e);
        }
    };
    finish(s, &json!({}))?;
    let holes = loops.iter().filter(|l| l.depth % 2 == 1).count();
    super::sketch_canvas::canvas_reply(
        cmd,
        CanvasTraced { sketch: id, name, canvas: canvas.id, curves, outlines: loops.len() - holes, holes, info, param_warnings: Vec::new() },
    )
}

fn pick(s: &Session, p: &Value, cmd: &str) -> Result<Canvas> {
    let all = &s.doc.canvases;
    let found = match p.get("canvas") {
        None | Some(Value::Null) => all.last(),
        Some(Value::Number(n)) => all.iter().find(|c| c.id.to_string() == n.to_string()),
        Some(Value::String(x)) => all.iter().find(|c| c.id.to_string() == *x || c.name == *x),
        _ => return Err(bad(cmd, "`canvas` must be an id or a name")),
    };
    found.cloned().ok_or_else(|| {
        bad(cmd, if all.is_empty() { "the design has no canvas (Insert > Canvas first)".to_string() } else { "no such canvas".to_string() })
    })
}

/// The picture at working size: its size, the ink (below) and each pixel's colour.
struct Loaded {
    w: usize,
    h: usize,
    ink: Vec<f64>,
    rgb: Vec<[f64; 3]>,
}

/// `at` and `tolerance` as given: `None` without `at`; refused when either cannot be read.
fn region_params(p: &Value, cmd: &str) -> Result<Option<(Vec2, f64)>> {
    let tolerance = match p.get("tolerance") {
        None | Some(Value::Null) => DEFAULT_TOLERANCE,
        Some(v) => v.as_f64().filter(|t| (0.0..=100.0).contains(t)).ok_or_else(|| bad(cmd, "`tolerance` must be a number from 0 to 100"))?,
    };
    match p.get("at") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => crate::params::vec2(v).map(|at| Some((at, tolerance))).ok_or_else(|| bad(cmd, "`at` must be a point [x, y] of numbers")),
    }
}

/// 1.0 for the pixels joined (4-way) to `seed` whose colour is within `tolerance` percent of the
/// seed's, 0.0 elsewhere. Iterative, one visit per pixel.
fn flood(rgb: &[[f64; 3]], w: usize, h: usize, seed: (usize, usize), tolerance: f64) -> Vec<f64> {
    let mut mask = vec![0.0; w * h];
    let Some(&base) = rgb.get(seed.1 * w + seed.0) else { return mask };
    // The largest colour distance is white to black: sqrt(3) * 255.
    let limit = tolerance / 100.0 * 3f64.sqrt() * 255.0;
    let near =
        |i: usize| rgb.get(i).is_some_and(|c| ((c[0] - base[0]).powi(2) + (c[1] - base[1]).powi(2) + (c[2] - base[2]).powi(2)).sqrt() <= limit);
    let mut stack = vec![(seed.0, seed.1)];
    if let Some(m) = mask.get_mut(seed.1 * w + seed.0) {
        *m = 1.0;
    }
    while let Some((x, y)) = stack.pop() {
        let around = [
            x.checked_sub(1).map(|a| (a, y)),
            (x + 1 < w).then_some((x + 1, y)),
            y.checked_sub(1).map(|b| (x, b)),
            (y + 1 < h).then_some((x, y + 1)),
        ];
        for (nx, ny) in around.into_iter().flatten() {
            let i = ny * w + nx;
            if mask.get(i).is_some_and(|m| *m == 0.0) && near(i) {
                if let Some(m) = mask.get_mut(i) {
                    *m = 1.0;
                }
                stack.push((nx, ny));
            }
        }
    }
    mask
}

/// Ink (0 background … 1 drawing) at working resolution, row-major. The decoder is limited to
/// `MAX_PIXELS` too, whatever the header said.
fn load(bytes: &[u8]) -> Option<Loaded> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4 + (1 << 20));
    reader.limits(limits);
    let img = reader.decode().ok()?.to_rgba8();
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return None;
    }
    let k = (WORK_RES as f64 / iw.max(ih) as f64).min(1.0);
    let (w, h) = (((iw as f64 * k).round() as u32).max(1), ((ih as f64 * k).round() as u32).max(1));
    let img = if k < 1.0 { image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle) } else { img };
    // Colour on white paper (transparent counts as white), 0-255 per channel.
    let rgb: Vec<[f64; 3]> = img
        .pixels()
        .map(|px| {
            let [r, g, b, a] = px.0;
            let a = a as f64 / 255.0;
            let over = |c: u8| c as f64 * a + 255.0 * (1.0 - a);
            [over(r), over(g), over(b)]
        })
        .collect();
    let mut ink: Vec<f64> = img
        .pixels()
        .map(|px| {
            let [r, g, b, a] = px.0;
            let a = a as f64 / 255.0;
            // Transparent counts as white paper.
            let lum = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0 * a + (1.0 - a);
            1.0 - lum
        })
        .collect();
    let (w, h) = (w as usize, h as usize);
    // A dark border means a light drawing on a dark background.
    let mut border = Vec::new();
    for x in 0..w {
        border.extend([ink.get(x), ink.get((h - 1) * w + x)]);
    }
    for y in 0..h {
        border.extend([ink.get(y * w), ink.get(y * w + w - 1)]);
    }
    let border: Vec<f64> = border.into_iter().flatten().copied().collect();
    if !border.is_empty() && border.iter().sum::<f64>() / border.len() as f64 > 0.5 {
        ink.iter_mut().for_each(|v| *v = 1.0 - *v);
    }
    Some(Loaded { w, h, ink, rgb })
}

/// The threshold that best splits drawing from background (Otsu), taken halfway between both.
fn otsu(ink: &[f64]) -> f64 {
    let mut hist = [0u64; 256];
    for v in ink {
        if let Some(c) = hist.get_mut(((v * 255.0) as usize).min(255)) {
            *c += 1;
        }
    }
    let total: u64 = hist.iter().sum();
    let sum_all: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum();
    let (mut w0, mut s0, mut best, mut best_t) = (0u64, 0.0, -1.0, 0.5);
    for (t, c) in hist.iter().enumerate() {
        w0 += c;
        if w0 == 0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0 {
            break;
        }
        s0 += t as f64 * *c as f64;
        let (m0, m1) = (s0 / w0 as f64, (sum_all - s0) / w1 as f64);
        let var = w0 as f64 * w1 as f64 * (m0 - m1).powi(2);
        if var > best {
            best = var;
            best_t = (m0 + m1) / 2.0 / 255.0;
        }
    }
    best_t.clamp(0.15, 0.85)
}

struct Opts {
    threshold: f64,
    blur: usize,
    tolerance: f64,
    min_area: f64,
    corner_angle: f64,
    straight_min: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { threshold: 0.5, blur: 1, tolerance: 1.0, min_area: 0.0004, corner_angle: 50.0, straight_min: 0.04 }
    }
}

struct Loop {
    points: Vec<Vec2>,
    /// How far the original outline strays from each simplified span (px), and the line
    /// through the span's inner samples, used to keep straight spans straight.
    dev: Vec<f64>,
    area: f64,
    depth: usize,
    bbox: [f64; 4],
}

fn blur(g: &[f64], w: usize, h: usize, r: usize) -> Vec<f64> {
    if r == 0 {
        return g.to_vec();
    }
    let pass = |src: &[f64], n: usize, stride: usize, count: usize, step: usize| -> Vec<f64> {
        let mut out = vec![0.0; src.len()];
        let k = (2 * r + 1) as f64;
        for line in 0..count {
            let at = |i: isize| -> f64 {
                let i = i.clamp(0, n as isize - 1) as usize;
                src.get(line * step + i * stride).copied().unwrap_or(0.0)
            };
            let mut sum: f64 = (-(r as isize)..=r as isize).map(&at).sum();
            for i in 0..n {
                if let Some(o) = out.get_mut(line * step + i * stride) {
                    *o = sum / k;
                }
                sum += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
            }
        }
        out
    };
    let rows = pass(g, w, 1, h, w);
    pass(&rows, h, w, w, 1)
}

fn trace(ink: &[f64], w: usize, h: usize, o: &Opts) -> Vec<Loop> {
    let g = blur(ink, w, h, o.blur);
    // A one-pixel empty frame closes every outline.
    let (pw, ph) = (w + 2, h + 2);
    let mut v = vec![0.0; pw * ph];
    for y in 0..h {
        for x in 0..w {
            if let (Some(d), Some(s)) = (v.get_mut((y + 1) * pw + x + 1), g.get(y * w + x)) {
                *d = *s;
            }
        }
    }
    let raw = contours(&v, pw, ph, o.threshold);
    let min_area = (o.min_area * (w * h) as f64).max(1.0);
    let mut loops = Vec::new();
    for pts in raw {
        let pts: Vec<Vec2> = pts.into_iter().map(|q| Vec2::new(q.x - 0.5, q.y - 0.5)).collect();
        if area(&pts).abs() < min_area {
            continue;
        }
        let (simp, dev, lines) = simplify_closed(&pts, o.tolerance.max(0.05), o.blur as f64 + 2.0);
        let simp = sharpen(&simp, &dev, &lines, flat(o), o.blur as f64 + 3.0);
        let a = area(&simp);
        if simp.len() < 3 || a.abs() < 1.0 {
            continue;
        }
        let xs = simp.iter().map(|q| q.x);
        let ys = simp.iter().map(|q| q.y);
        let bbox =
            [xs.clone().fold(f64::MAX, f64::min), ys.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max), ys.fold(f64::MIN, f64::max)];
        loops.push(Loop { points: simp, dev, area: a, depth: 0, bbox });
        if loops.len() >= MAX_LOOPS {
            break;
        }
    }
    // Nesting depth: how many larger outlines contain it (even: filled, odd: a hole).
    let depths: Vec<usize> = loops
        .iter()
        .enumerate()
        .map(|(i, li)| {
            let q = li.points.first().copied().unwrap_or_default();
            loops
                .iter()
                .enumerate()
                .filter(|(j, lj)| {
                    *j != i
                        && lj.area.abs() > li.area.abs()
                        && q.x >= lj.bbox[0]
                        && q.x <= lj.bbox[2]
                        && q.y >= lj.bbox[1]
                        && q.y <= lj.bbox[3]
                        && inside(q, &lj.points)
                })
                .count()
        })
        .collect();
    for (l, d) in loops.iter_mut().zip(depths) {
        l.depth = d;
    }
    loops
}

/// Interpolated marching squares: closed outlines at `t`, in grid coordinates.
fn contours(v: &[f64], w: usize, h: usize, t: f64) -> Vec<Vec<Vec2>> {
    use std::collections::HashMap;
    let at = |x: usize, y: usize| v.get(y * w + x).copied().unwrap_or(0.0);
    let frac = |a: f64, b: f64| if (b - a).abs() < 1e-12 { 0.5 } else { (t - a) / (b - a) };
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut pos: HashMap<usize, Vec2> = HashMap::new();
    for y in 0..h.saturating_sub(1) {
        for x in 0..w.saturating_sub(1) {
            let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1));
            let idx = (usize::from(a > t) << 3) | (usize::from(b > t) << 2) | (usize::from(c > t) << 1) | usize::from(d > t);
            let centre = (a + b + c + d) / 4.0 > t;
            // Edges: 0 top, 1 right, 2 bottom, 3 left.
            let segs: &[(u8, u8)] = match idx {
                1 | 14 => &[(3, 2)],
                2 | 13 => &[(2, 1)],
                3 | 12 => &[(3, 1)],
                4 | 11 => &[(0, 1)],
                6 | 9 => &[(0, 2)],
                7 | 8 => &[(0, 3)],
                5 if centre => &[(0, 3), (2, 1)],
                5 => &[(0, 1), (3, 2)],
                10 if centre => &[(0, 1), (3, 2)],
                10 => &[(0, 3), (2, 1)],
                _ => &[],
            };
            for &(e1, e2) in segs {
                let mut keys = [0usize; 2];
                for (slot, e) in keys.iter_mut().zip([e1, e2]) {
                    let (k, p) = match e {
                        0 => ((y * w + x) * 2, Vec2::new(x as f64 + frac(a, b), y as f64)),
                        2 => (((y + 1) * w + x) * 2, Vec2::new(x as f64 + frac(d, c), (y + 1) as f64)),
                        3 => ((y * w + x) * 2 + 1, Vec2::new(x as f64, y as f64 + frac(a, d))),
                        _ => ((y * w + x + 1) * 2 + 1, Vec2::new((x + 1) as f64, y as f64 + frac(b, c))),
                    };
                    pos.entry(k).or_insert(p);
                    *slot = k;
                }
                adj.entry(keys[0]).or_default().push(keys[1]);
                adj.entry(keys[1]).or_default().push(keys[0]);
            }
        }
    }
    let mut keys: Vec<usize> = adj.keys().copied().collect();
    keys.sort_unstable();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for start in keys {
        if !seen.insert(start) {
            continue;
        }
        let mut lp = vec![start];
        let (mut prev, mut cur) = (usize::MAX, start);
        for _ in 0..adj.len() + 1 {
            let Some(n) = adj.get(&cur) else { break };
            let next = match (n.first(), n.get(1)) {
                (Some(&a), Some(&b)) => {
                    if a != prev {
                        a
                    } else {
                        b
                    }
                }
                (Some(&a), None) => a,
                _ => break,
            };
            if next == start || !seen.insert(next) {
                break;
            }
            lp.push(next);
            (prev, cur) = (cur, next);
        }
        if lp.len() >= 3 {
            out.push(lp.iter().filter_map(|k| pos.get(k).copied()).collect());
        }
    }
    out
}

fn area(p: &[Vec2]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

fn inside(q: Vec2, p: &[Vec2]) -> bool {
    let n = p.len();
    let mut c = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (p[i], p[j % n]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

fn seg_dist(q: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l2 = d.x * d.x + d.y * d.y;
    if l2 == 0.0 {
        return q.dist(a);
    }
    let u = (((q.x - a.x) * d.x + (q.y - a.y) * d.y) / l2).clamp(0.0, 1.0);
    q.dist(a + d * u)
}

/// Douglas–Peucker on an open chain: the indices kept.
fn dp_open(p: &[Vec2], eps: f64) -> Vec<usize> {
    let n = p.len();
    if n < 3 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    while let Some((i, j)) = stack.pop() {
        let (mut best, mut bi) = (-1.0, 0);
        for k in i + 1..j {
            let d = seg_dist(p[k], p[i], p[j]);
            if d > best {
                best = d;
                bi = k;
            }
        }
        if best > eps && bi > 0 {
            keep[bi] = true;
            stack.push((i, bi));
            stack.push((bi, j));
        }
    }
    keep.iter().enumerate().filter(|(_, k)| **k).map(|(i, _)| i).collect()
}

type Line2 = Option<(Vec2, Vec2)>;

/// Simplified closed outline, with each span's straightness (the original samples away from
/// its ends against the line through the first and last of them) and that line.
fn simplify_closed(p: &[Vec2], eps: f64, margin: f64) -> (Vec<Vec2>, Vec<f64>, Vec<Line2>) {
    let n = p.len();
    if n < 5 {
        return (p.to_vec(), vec![f64::INFINITY; n], vec![None; n]);
    }
    let p0 = p[0];
    let far = (0..n).max_by(|a, b| p[*a].dist(p0).total_cmp(&p[*b].dist(p0))).unwrap_or(0);
    let a = dp_open(&p[..=far], eps);
    let mut tail = p[far..].to_vec();
    tail.push(p0);
    let b: Vec<usize> = dp_open(&tail, eps).into_iter().map(|i| far + i).collect();
    let mut idx: Vec<usize> = a[..a.len().saturating_sub(1)].iter().chain(&b[..b.len().saturating_sub(1)]).copied().collect();
    idx.dedup_by(|x, y| p[*x % n].dist(p[*y % n]) < 1e-6);
    if idx.len() > 1 && p[idx[0] % n].dist(p[idx[idx.len() - 1] % n]) < 1e-6 {
        idx.pop();
    }
    let m = idx.len();
    let mut dev = Vec::with_capacity(m);
    let mut lines = Vec::with_capacity(m);
    for k in 0..m {
        let (i, j) = (idx[k] % n, idx[(k + 1) % m] % n);
        let (a, b) = (p[i], p[j]);
        let span: Vec<usize> = if j > i { (i + 1..j).collect() } else { (i + 1..n).chain(0..j).collect() };
        let inner: Vec<Vec2> = span.iter().map(|q| p[*q]).filter(|q| q.dist(a).min(q.dist(b)) > margin).collect();
        match (inner.first(), inner.last()) {
            (Some(&s), Some(&e)) if inner.len() >= 2 && s.dist(e) > 1.0 => {
                let d = e - s;
                let len = (d.x * d.x + d.y * d.y).sqrt();
                dev.push(inner.iter().map(|r| ((r.x - s.x) * d.y - (r.y - s.y) * d.x).abs() / len).fold(0.0, f64::max));
                lines.push(Some((s, e)));
            }
            _ => {
                dev.push(f64::INFINITY);
                lines.push(None);
            }
        }
    }
    (idx.iter().map(|i| p[*i % n]).collect(), dev, lines)
}

fn intersect((p1, p2): (Vec2, Vec2), (p3, p4): (Vec2, Vec2)) -> Option<Vec2> {
    let d = (p1.x - p2.x) * (p3.y - p4.y) - (p1.y - p2.y) * (p3.x - p4.x);
    if d.abs() < 1e-9 {
        return None;
    }
    let a = p1.x * p2.y - p1.y * p2.x;
    let b = p3.x * p4.y - p3.y * p4.x;
    Some(Vec2::new((a * (p3.x - p4.x) - (p1.x - p2.x) * b) / d, (a * (p3.y - p4.y) - (p1.y - p2.y) * b) / d))
}

/// A corner between two straight spans moves to where their lines meet (blur rounds it).
fn sharpen(p: &[Vec2], dev: &[f64], lines: &[Line2], flat: f64, reach: f64) -> Vec<Vec2> {
    let n = p.len();
    let mut out = p.to_vec();
    for i in 0..n {
        let a = (i + n - 1) % n;
        if let (Some(&da), Some(&di), Some(Some(la)), Some(Some(li))) = (dev.get(a), dev.get(i), lines.get(a), lines.get(i))
            && da <= flat
            && di <= flat
            && let Some(q) = intersect(*la, *li)
            && q.dist(p[i]) <= reach
        {
            out[i] = q;
        }
    }
    out
}

fn flat(o: &Opts) -> f64 {
    (o.tolerance * 0.35).max(0.3)
}

#[derive(Clone, Copy)]
enum Seg {
    Line(Vec2),
    Cubic(Vec2, Vec2, Vec2),
}

impl Seg {
    fn end(&self) -> Vec2 {
        match *self {
            Seg::Line(b) | Seg::Cubic(_, _, b) => b,
        }
    }
}

/// The outline as lines (long straight spans) and cubic Béziers through the other vertices,
/// G1 where they meet a line or each other, kinked at sharp corners.
fn segments(l: &Loop, o: &Opts, straight: f64) -> Vec<Seg> {
    let p = &l.points;
    let n = p.len();
    let len: Vec<f64> = (0..n).map(|i| p[(i + 1) % n].dist(p[i])).collect();
    let fl = flat(o);
    let line: Vec<bool> = (0..n).map(|i| len[i] >= straight && l.dev.get(i).is_none_or(|d| *d <= fl)).collect();
    let sharp: Vec<bool> = (0..n)
        .map(|i| {
            let (u, v) = (p[i] - p[(i + n - 1) % n], p[(i + 1) % n] - p[i]);
            (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y).abs().to_degrees() > o.corner_angle
        })
        .collect();
    let unit = |v: Vec2| {
        let l = (v.x * v.x + v.y * v.y).sqrt();
        if l > 0.0 { v * (1.0 / l) } else { Vec2::ZERO }
    };
    // The direction at vertex i for the curve on span `seg` (i or i - 1).
    let tangent = |i: usize, seg: usize| -> Vec2 {
        if sharp[i] {
            return p[(seg + 1) % n] - p[seg];
        }
        let other = if seg == i { (i + n - 1) % n } else { i };
        if line[other] {
            return p[(other + 1) % n] - p[other];
        }
        p[(i + 1) % n] - p[(i + n - 1) % n]
    };
    (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            if line[i] {
                return Seg::Line(p[j]);
            }
            // Handles a third of the span long: no overshoot or loops between unequal spans.
            let k = len[i] / 3.0;
            Seg::Cubic(p[i] + unit(tangent(i, i)) * k, p[j] - unit(tangent(j, i)) * k, p[j])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use crate::tests::image_file;

    /// A white 200x100 PNG with a black ring (outer r 40, inner r 15) and a black square.
    fn picture() -> String {
        let mut img = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 255, 255, 255]));
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f64 - 50.0).powi(2) + (y as f64 - 50.0).powi(2)).sqrt();
            if (15.0..40.0).contains(&d) || ((120..180).contains(&x) && (20..80).contains(&y)) {
                *px = image::Rgba([0, 0, 0, 255]);
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        image_file(&out.into_inner())
    }

    #[test]
    fn canvas_becomes_a_sketch_on_it() {
        let mut s = Session::default();
        s.execute("canvas.insert", &json!({"path": picture(), "plane": "XY", "width": 100, "center": [10, 5], "name": "Pic"})).unwrap();
        let r = s.execute("canvas.to_sketch", &json!({})).unwrap();
        assert_eq!(r["outlines"], 2, "{r}");
        assert_eq!(r["holes"], 1, "{r}");
        assert!(s.active_sketch.is_none());
        let id = r["sketch"].as_u64().unwrap();
        let ss = s.model.state().sketch(id).unwrap().clone();
        // Ring (40, 15 px at 0.5 mm/px) and the 60x60 px square (30x30 mm, from x 60 to 90).
        let mut areas: Vec<f64> = ss.profiles.iter().map(|p| p.area).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        let ring = std::f64::consts::PI * (20.0f64.powi(2) - 7.5f64.powi(2));
        assert!(areas.iter().any(|a| (a - ring).abs() < ring * 0.03), "{areas:?}");
        assert!(areas.iter().any(|a| (a - 900.0).abs() < 900.0 * 0.03), "{areas:?}");
        // One undo step removes it again.
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.find_feature("Pic sketch").is_none());
        assert!(s.execute("canvas.to_sketch", &json!({"canvas": "nope"})).is_err());
    }

    /// A white 200x100 PNG with a red disc (centre 50, 50; radius 30) holding a darker red disc
    /// (radius 12), and a blue square (x 120 to 180, y 20 to 80).
    fn shapes() -> String {
        let mut img = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 255, 255, 255]));
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f64 - 50.0).powi(2) + (y as f64 - 50.0).powi(2)).sqrt();
            if d < 12.0 {
                *px = image::Rgba([200, 0, 0, 255]);
            } else if d < 30.0 {
                *px = image::Rgba([255, 0, 0, 255]);
            } else if (120..180).contains(&x) && (20..80).contains(&y) {
                *px = image::Rgba([0, 0, 255, 255]);
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        image_file(&out.into_inner())
    }

    /// Where image pixel (x, y) of `shapes()` lies on the plane, for the canvas placed below
    /// (100 mm wide, centred at [10, 5]: half a millimetre a pixel).
    fn at(x: f64, y: f64) -> serde_json::Value {
        json!([10.0 + x * 0.5 - 50.0, 5.0 + 25.0 - y * 0.5])
    }

    fn placed() -> Session {
        let mut s = Session::default();
        s.execute("canvas.insert", &json!({"path": shapes(), "plane": "XY", "width": 100, "center": [10, 5], "name": "Pic"})).unwrap();
        s
    }

    fn areas(s: &Session, r: &serde_json::Value) -> Vec<f64> {
        let ss = s.model.state().sketch(r["sketch"].as_u64().unwrap()).unwrap().clone();
        let mut a: Vec<f64> = ss.profiles.iter().map(|p| p.area).collect();
        a.sort_by(|a, b| a.total_cmp(b));
        a
    }

    #[test]
    fn a_picked_colour_region_is_traced_alone() {
        let mut s = placed();
        // The red disc, both reds (tolerance 20): one loop, the blue square left out.
        let r = s.execute("canvas.to_sketch", &json!({"at": at(75.0, 50.0), "tolerance": 20})).unwrap();
        assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(1), Some(0)), "{r}");
        let disc = std::f64::consts::PI * 15.0f64.powi(2);
        let a = areas(&s, &r);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!((a[0] - disc).abs() < disc * 0.05, "{a:?} vs {disc}");
        // The default tolerance reads the same.
        let r = s.execute("canvas.to_sketch", &json!({"at": at(75.0, 50.0)})).unwrap();
        assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(1), Some(0)), "{r}");
        // The blue square alone.
        let r = s.execute("canvas.to_sketch", &json!({"at": at(150.0, 50.0)})).unwrap();
        let a = areas(&s, &r);
        assert!(a.len() == 1 && (a[0] - 900.0).abs() < 900.0 * 0.05, "{a:?}");
    }

    #[test]
    fn tolerance_decides_whether_a_darker_shade_joins() {
        let mut s = placed();
        // Tolerance 0: only the bright ring, with the darker centre a hole.
        let r = s.execute("canvas.to_sketch", &json!({"at": at(75.0, 50.0), "tolerance": 0})).unwrap();
        assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(1), Some(1)), "{r}");
        // Tolerance 20: both shades, a solid disc.
        let r = s.execute("canvas.to_sketch", &json!({"at": at(75.0, 50.0), "tolerance": 20})).unwrap();
        assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(1), Some(0)), "{r}");
    }

    #[test]
    fn picking_the_background_keeps_the_shapes_as_holes() {
        let mut s = placed();
        let r = s.execute("canvas.to_sketch", &json!({"at": at(5.0, 5.0), "tolerance": 5})).unwrap();
        assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(1), Some(2)), "{r}");
    }

    #[test]
    fn a_pick_that_selects_nothing_useful_is_refused() {
        let mut s = placed();
        let e = s.execute("canvas.to_sketch", &json!({"at": [1000, 1000]})).unwrap_err().to_string();
        assert!(e.contains("outside the canvas image"), "{e}");
        let e = s.execute("canvas.to_sketch", &json!({"at": at(75.0, 50.0), "tolerance": 100})).unwrap_err().to_string();
        assert!(e.contains("whole picture"), "{e}");
        assert!(s.doc.find_feature("Pic sketch").is_none(), "nothing is added");
    }

    #[test]
    fn without_a_pick_the_whole_picture_is_traced_as_before() {
        let mut s = Session::default();
        s.execute("canvas.insert", &json!({"path": picture(), "plane": "XY", "width": 100, "center": [10, 5], "name": "Pic"})).unwrap();
        let plain = s.execute("canvas.to_sketch", &json!({})).unwrap();
        let null = s.execute("canvas.to_sketch", &json!({"at": null, "tolerance": null})).unwrap();
        for r in [&plain, &null] {
            assert_eq!((r["outlines"].as_u64(), r["holes"].as_u64()), (Some(2), Some(1)), "{r}");
        }
        assert_eq!(areas(&s, &plain), areas(&s, &null));
    }

    #[test]
    fn hostile_region_params_never_panic() {
        let mut s = placed();
        let bad = [
            json!({"at": "here"}),
            json!({"at": [1]}),
            json!({"at": {"x": 1}}),
            json!({"at": at(75.0, 50.0), "tolerance": -1}),
            json!({"at": at(75.0, 50.0), "tolerance": 101}),
            json!({"at": at(75.0, 50.0), "tolerance": "high"}),
            json!({"tolerance": [1]}),
        ];
        for p in bad {
            assert!(s.execute("canvas.to_sketch", &p).is_err(), "{p}");
        }
        for p in [
            json!({"at": [1e308, -1e308]}),
            json!({"at": [-1e-320, 1e-320]}),
            json!({"at": [0, 0], "tolerance": 0}),
            json!({"at": [-40, 30], "tolerance": 1e-300}),
        ] {
            let _ = s.execute("canvas.to_sketch", &p);
        }
        assert!(s.active_sketch.is_none());
    }

    /// A PNG header declaring a 100 000 x 100 000 image (no pixel data) is refused from its
    /// declared size, before any decoding.
    #[test]
    fn huge_declared_images_are_refused_before_decoding() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(100_000u32.to_be_bytes());
        b.extend(100_000u32.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let mut s = Session::default();
        s.execute("canvas.insert", &json!({"path": image_file(&b), "width": 100})).unwrap();
        let e = s.execute("canvas.to_sketch", &json!({})).unwrap_err().to_string();
        assert!(e.contains("40 megapixels"), "{e}");
        assert!(s.doc.features.is_empty(), "nothing is added");
    }
}
