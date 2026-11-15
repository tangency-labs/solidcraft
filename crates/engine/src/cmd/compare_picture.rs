//! Compare the model with a reference picture (#1486): agents model to a picture, and an
//! overlap score plus per-edge mm offsets tell the agent which edge to move and by how much.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;

use serde_json::{Value, json};
use solidcraft_doc::canvas::base64_encode;

use super::CommandSpec;
use super::silhouette::{self, Silhouette};
use crate::params::bad;
use crate::{Result, Session};

const CMD: &str = "inspect.compare_picture";
/// A reference picture is read through the vfs with a byte cap, like `screenshot` output.
const MAX_REFERENCE_BYTES: usize = 32 << 20;
/// Decoded pixels are capped as well, so a small file cannot unfold into a huge raster.
const MAX_DECODED_PIXELS: u64 = 16 << 20;
/// The comparison grid each mask is fitted into at most — the picture's and the fitted
/// model's alike; keeps the slide search and the memory it needs bounded.
const MAX_GRID: usize = 1024;
/// Background grows from the border only through steps of at most this per channel, so a
/// backdrop's gradual shading belongs to it and the step at the part's outline stops it.
const BG_STEP: u16 = 12;
/// When the first pass finds no part, the flood retries at this stricter step: a white part
/// on a (245, 245, 245) studio backdrop is a step of 10, which BG_STEP lets through.
const BG_STEP_STRICT: u16 = 4;
/// A picture whose border is mostly transparent: a pixel below this alpha is see-through.
const ALPHA_BG: u8 = 128;
/// The global retry's backdrop: a pixel within this of the border's main colour is
/// background. A light JPEG (white on (245, 245, 245) at quality 75) blurs the step at the
/// part's outline below any local limit, so that pass compares to the backdrop's colour.
const BG_NEAR: u16 = 8;
/// Stations per side for the edge offsets.
const STATIONS: usize = 9;

// For agents modelling to a picture; nothing to show a person, so no toolbar button.
pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Compare Picture", run).noundo().icon("compare").params(
    "reference: path to a PNG or JPEG the model should match; view?/camera?/bodies?/rows? as inspect.silhouette; \
     crop_px?: [x, y, w, h] in pixels, or crop_fraction?: [x, y, w, h] as fractions 0..1 of the picture \
     (to take one view off a board); part_fills_crop?: false — with true the crop box is the part itself \
     and no background search runs, for a crop that tightly frames the part; \
     fit?: \"width\" | \"height\" | \"both\" (default \"width\"): one uniform scale, taken from the named \
     dimension (\"both\" claims both and scales by width, reporting aspect_mismatch when the shapes differ); \
     overlay?: false leaves the overlay PNG out → overlap (0..1), edges: top|bottom|left|right in mm at nine stations \
     (positive where the model reaches past the picture, null where a station misses one of the two), the grid and its \
     mm_per_px, and overlay_png (red model, blue picture) as base64 PNG. The compare is outline only: \
     holes are not compared — both the picture's and the model's holes are filled",
)];

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let reference = p
        .get("reference")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| bad(CMD, "`reference` is required: a path to a PNG or JPEG"))?;
    let fit = match p.get("fit") {
        None => Fit::Width,
        Some(Value::String(v)) => match v.as_str() {
            "width" => Fit::Width,
            "height" => Fit::Height,
            "both" => Fit::Both,
            other => return Err(bad(CMD, format!("`fit` must be \"width\", \"height\" or \"both\", not `{other}`"))),
        },
        Some(_) => return Err(bad(CMD, "`fit` must be a string")),
    };
    let overlay = p.get("overlay").map(|v| v.as_bool().unwrap_or(false)).unwrap_or(true);
    let part_fills_crop = p.get("part_fills_crop").and_then(Value::as_bool).unwrap_or(false);

    // The model side: the same outline inspect.silhouette returns, as filled rows.
    let camera = silhouette::camera(p)?;
    let rows = silhouette::rows(p)?;
    let wanted = silhouette::body_names(p)?;
    let model = silhouette::outline(s, &camera, wanted.as_deref(), rows)?;

    // The picture side: decode, crop, and find the part.
    let bytes = solidcraft_io::vfs::read(reference, MAX_REFERENCE_BYTES).map_err(|e| bad(CMD, format!("{reference}: {e}")))?;
    let img = decode(&bytes)?;
    let img = apply_crop(img, p.get("crop_px"), p.get("crop_fraction"))?;
    let picture = if part_fills_crop {
        part_from_box(&img).map_err(|reason| bad(CMD, format!("the crop cannot be the part: {reason}")))?
    } else {
        find_part(&img).map_err(|reason| bad(CMD, format!("no part found in the picture: {reason}")))?
    };

    // Fit the model outline to the picture's scale, then slide it to the best overlap.
    let (mw, mh, mmpp_x, mmpp_y) = fit_model(&model, picture.width, picture.height, fit);
    let mut model_scaled = resample(&model.mask, model.width, model.height, mw, mh);
    // The same hole rule the picture side has: a through-hole in the model counts as its
    // outline, not as a hole the picture has to match.
    fill_holes(&mut model_scaled);
    let best = best_overlap(&model_scaled, &picture);
    let placed = Placed { dx: best.dx, dy: best.dy, mask: &model_scaled };
    let aspect_mismatch = (picture.width as f64 * mh as f64) / (picture.height as f64 * mw as f64);

    let mut out = json!({
        "overlap": round4(best.overlap),
        "stations": STATIONS,
        "edges": {
            "top": edge_offsets(&placed, &picture, Edge::Top, mmpp_y),
            "bottom": edge_offsets(&placed, &picture, Edge::Bottom, mmpp_y),
            "left": edge_offsets(&placed, &picture, Edge::Left, mmpp_x),
            "right": edge_offsets(&placed, &picture, Edge::Right, mmpp_x),
        },
        "fit": match fit { Fit::Width => "width", Fit::Height => "height", Fit::Both => "both" },
        "aspect_mismatch": round4(aspect_mismatch),
        "mm_per_px": {"x": round4(mmpp_x), "y": round4(mmpp_y)},
        "model": {"width": model.width, "height": model.height, "box_mm": [[round4(model.lo[0]), round4(model.lo[1])], [round4(model.hi[0]), round4(model.hi[1])]], "bodies": model.bodies},
        "picture": {"width": picture.width, "height": picture.height, "source": reference},
    });
    if overlay {
        out["overlay_png"] = json!(base64_encode(&overlay_png(&picture, &placed)));
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Fit {
    Width,
    Height,
    Both,
}

#[derive(Clone, Copy)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// A picture mask, tightly framed to the found part.
struct Part {
    width: usize,
    height: usize,
    mask: Vec<Vec<bool>>,
}

/// A model mask placed relative to the picture: its top-left sits at (dx, dy) in the
/// picture's own pixel coordinates, which may be negative.
struct Placed<'a> {
    dx: i64,
    dy: i64,
    mask: &'a [Vec<bool>],
}

/// Decode PNG or JPEG to RGBA8, refusing anything else and anything that unfolds too large.
fn decode(bytes: &[u8]) -> Result<image::RgbaImage> {
    let guess = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| bad(CMD, format!("cannot read the picture: {e}")))?;
    if !matches!(guess.format(), Some(image::ImageFormat::Png) | Some(image::ImageFormat::Jpeg)) {
        return Err(bad(CMD, "the reference must be a PNG or a JPEG"));
    }
    let (w, h) = guess.into_dimensions().map_err(|e| bad(CMD, format!("cannot read the picture: {e}")))?;
    if w as u64 * h as u64 > MAX_DECODED_PIXELS {
        return Err(bad(CMD, format!("the picture is {w}x{h}: over the {} megapixel limit", MAX_DECODED_PIXELS >> 20)));
    }
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| bad(CMD, format!("cannot read the picture: {e}")))?;
    let img = reader.decode().map_err(|e| bad(CMD, format!("cannot decode the picture: {e}")))?;
    Ok(img.to_rgba8())
}

/// Apply `crop_px: [x, y, w, h]` (pixels) or `crop_fraction: [x, y, w, h]` (fractions 0..1
/// of the picture). Explicit units: a crop of `[0, 0, 1, 1]` means one pixel, not the whole.
fn apply_crop(img: image::RgbaImage, crop_px: Option<&Value>, crop_fraction: Option<&Value>) -> Result<image::RgbaImage> {
    match (crop_px, crop_fraction) {
        (Some(_), Some(_)) => Err(bad(CMD, "give `crop_px` or `crop_fraction`, not both")),
        (None, None) => Ok(img),
        (Some(v), None) => {
            let nums = crop_nums(v, "crop_px")?;
            let (w, h) = img.dimensions();
            crop_to(img, nums[0].round() as usize, nums[1].round() as usize, nums[2].round() as usize, nums[3].round() as usize, w, h)
        }
        (None, Some(v)) => {
            let nums = crop_nums(v, "crop_fraction")?;
            if nums.iter().any(|f| *f > 1.0) {
                return Err(bad(CMD, "`crop_fraction` values must be 0..1"));
            }
            let (w, h) = img.dimensions();
            crop_to(
                img,
                (nums[0] * w as f64).round() as usize,
                (nums[1] * h as f64).round() as usize,
                (nums[2] * w as f64).round() as usize,
                (nums[3] * h as f64).round() as usize,
                w,
                h,
            )
        }
    }
}

fn crop_nums(v: &Value, name: &str) -> Result<Vec<f64>> {
    let nums: Vec<f64> = v
        .as_array()
        .filter(|a| a.len() == 4)
        .and_then(|a| a.iter().map(|v| v.as_f64()).collect())
        .ok_or_else(|| bad(CMD, format!("`{name}` must be [x, y, w, h]")))?;
    if nums.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(bad(CMD, format!("`{name}` values must be non-negative")));
    }
    Ok(nums)
}

fn crop_to(img: image::RgbaImage, x: usize, y: usize, cw: usize, ch: usize, w: u32, h: u32) -> Result<image::RgbaImage> {
    if x >= w as usize || y >= h as usize {
        return Err(bad(CMD, "the crop starts outside the picture"));
    }
    let cw = cw.max(1).min(w as usize - x);
    let ch = ch.max(1).min(h as usize - y);
    Ok(image::imageops::crop_imm(&img, x as u32, y as u32, cw as u32, ch as u32).to_image())
}

/// Find the part. Alpha decides only when the backdrop really is transparent — most border
/// pixels see-through — so one stray translucent pixel cannot switch the read; then any ink
/// is part, so a part drawn at a faint alpha survives. Otherwise grow the background in from
/// the border through small colour steps only — seeded from the border pixels close to the
/// border's main colour, so a part touching the border is not eaten by its own edge — keep
/// what the background did not reach, and keep the largest 4-connected piece. On a retry the
/// step is stricter, so a white part on a (245, 245, 245) backdrop (a step of 10) still
/// separates; on a second retry the backdrop is taken globally — within BG_NEAR of the
/// border's main colour — because a light JPEG blurs the step at the part's outline below
/// any local limit. Says which happened when no part is found.
fn find_part(img: &image::RgbaImage) -> std::result::Result<Part, String> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 2 || h < 2 {
        return Err("the picture is smaller than 2 x 2 pixels".to_string());
    }
    let px = |x: usize, y: usize| -> [u16; 3] {
        let p = img.get_pixel(x as u32, y as u32).0;
        [p[0] as u16, p[1] as u16, p[2] as u16]
    };
    let step = |a: [u16; 3], b: [u16; 3]| (a[0].abs_diff(b[0])).max(a[1].abs_diff(b[1])).max(a[2].abs_diff(b[2]));

    // Alpha decides only when the border is mostly see-through: a single backdrop pixel at
    // alpha 254 in an otherwise opaque picture must not turn the whole read into alpha.
    let mut border = 0usize;
    let mut see_through = 0usize;
    for x in 0..w {
        for y in [0, h - 1] {
            border += 1;
            if img.get_pixel(x as u32, y as u32)[3] < ALPHA_BG {
                see_through += 1;
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            border += 1;
            if img.get_pixel(x as u32, y as u32)[3] < ALPHA_BG {
                see_through += 1;
            }
        }
    }
    let uses_alpha = see_through * 2 > border;

    let (part, too_small): (Vec<bool>, String) = if uses_alpha {
        let mask: Vec<bool> = img.pixels().map(|p| p[3] > 0).collect();
        (mask, "the picture is transparent: no pixel holds any ink".to_string())
    } else {
        let bg = flood_background(&px, w, h, BG_STEP, &step);
        let mask: Vec<bool> = (0..w * h).map(|i| !bg[i]).collect();
        if largest_piece(&mask, w, h).len() >= 16 {
            (mask, "the picture holds no piece big enough to be a part".to_string())
        } else {
            let bg = flood_background(&px, w, h, BG_STEP_STRICT, &step);
            let mask: Vec<bool> = (0..w * h).map(|i| !bg[i]).collect();
            if largest_piece(&mask, w, h).len() >= 16 {
                (mask, "the picture holds no piece big enough to be a part".to_string())
            } else {
                // The local step fails on a light JPEG: the codec blurs the 10-level step
                // at the part's outline into steps of a few levels each, under any limit.
                // Take the backdrop globally instead: near the border's main colour.
                let main = border_main_colour(&px, w, h);
                let mask: Vec<bool> = (0..w * h).map(|i| step(main, px(i % w, i / w)) > BG_NEAR).collect();
                (mask, "the background grew from the border over the whole picture, even with a stricter colour step: the part's colour is too close to the backdrop's".to_string())
            }
        }
    };

    let best = largest_piece(&part, w, h);
    if best.len() < 16 {
        return Err(too_small);
    }
    // Tight frame, then downscale to the comparison grid at need.
    let (x0, y0, x1, y1) = frame(&best, w);
    let (pw, ph) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut mask = vec![vec![false; pw]; ph];
    for &i in &best {
        let (x, y) = (i % w, i / w);
        mask[y - y0][x - x0] = true;
    }
    let (dw, dh) = fit_grid(pw, ph);
    let mut mask = if (dw, dh) == (pw, ph) { mask } else { resample(&mask, pw, ph, dw, dh) };
    fill_holes(&mut mask);
    Ok(Part { width: dw, height: dh, mask })
}

/// The crop box is the part itself (`part_fills_crop`): no background search. This is how
/// one view cropped tight off a board says so — a part touching three sides leaves the
/// border's main colour the part's own, and the flood would keep the backdrop strip instead.
fn part_from_box(img: &image::RgbaImage) -> std::result::Result<Part, String> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 2 || h < 2 {
        return Err("the crop is smaller than 2 x 2 pixels".to_string());
    }
    let (dw, dh) = fit_grid(w, h);
    Ok(Part { width: dw, height: dh, mask: vec![vec![true; dw]; dh] })
}

/// The border's main colour: the border colour it has most of. A part crossing the border
/// is in the border too, but the backdrop is what most of the border shows.
fn border_main_colour(px: &impl Fn(usize, usize) -> [u16; 3], w: usize, h: usize) -> [u16; 3] {
    let mut counts: HashMap<[u16; 3], usize> = HashMap::new();
    for x in 0..w {
        for y in [0, h - 1] {
            *counts.entry(px(x, y)).or_default() += 1;
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            *counts.entry(px(x, y)).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c).unwrap_or([0; 3])
}

/// The background mask: grown from every border pixel whose colour is close to the border's
/// main colour, crossing only steps of at most `limit` per channel.
fn flood_background(px: &impl Fn(usize, usize) -> [u16; 3], w: usize, h: usize, limit: u16, step: &impl Fn([u16; 3], [u16; 3]) -> u16) -> Vec<bool> {
    let main = border_main_colour(px, w, h);

    let mut bg = vec![false; w * h];
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    let mut seed = |x: usize, y: usize| {
        if step(main, px(x, y)) <= limit && !bg[y * w + x] {
            bg[y * w + x] = true;
            queue.push_back((x, y));
        }
    };
    for x in 0..w {
        seed(x, 0);
        seed(x, h - 1);
    }
    for y in 0..h {
        seed(0, y);
        seed(w - 1, y);
    }
    while let Some((x, y)) = queue.pop_front() {
        let here = px(x, y);
        for (nx, ny) in neighbours(x, y, w, h) {
            if !bg[ny * w + nx] && step(here, px(nx, ny)) <= limit {
                bg[ny * w + nx] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    bg
}

/// The largest 4-connected piece of a mask, as indices into `w`-wide rows.
fn largest_piece(mask: &[bool], w: usize, h: usize) -> Vec<usize> {
    let mut best: Vec<usize> = Vec::new();
    let mut seen = vec![false; w * h];
    for start in 0..w * h {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut piece: Vec<usize> = Vec::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        seen[start] = true;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            piece.push(i);
            let (x, y) = (i % w, i / w);
            for (nx, ny) in neighbours(x, y, w, h) {
                let j = ny * w + nx;
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    queue.push_back(j);
                }
            }
        }
        if piece.len() > best.len() {
            best = piece;
        }
    }
    best
}

/// Fill the holes: pixels the outside never reaches become part — the rule the picture's
/// border flood gives the picture side for free, given to the model's mask as well.
fn fill_holes(mask: &mut [Vec<bool>]) {
    let (h, w) = (mask.len(), mask.first().map_or(0, Vec::len));
    if w < 2 || h < 2 {
        return;
    }
    let mut outside = vec![false; w * h];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for x in 0..w {
        for y in [0, h - 1] {
            if !mask[y][x] && !outside[y * w + x] {
                outside[y * w + x] = true;
                queue.push_back(y * w + x);
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            if !mask[y][x] && !outside[y * w + x] {
                outside[y * w + x] = true;
                queue.push_back(y * w + x);
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % w, i / w);
        for (nx, ny) in neighbours(x, y, w, h) {
            let j = ny * w + nx;
            if !mask[ny][nx] && !outside[j] {
                outside[j] = true;
                queue.push_back(j);
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if !mask[y][x] && !outside[y * w + x] {
                mask[y][x] = true;
            }
        }
    }
}

fn neighbours(x: usize, y: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].into_iter().filter(move |(nx, ny)| *nx < w && *ny < h)
}

fn frame(pixels: &[usize], w: usize) -> (usize, usize, usize, usize) {
    let mut x0 = usize::MAX;
    let mut y0 = usize::MAX;
    let mut x1 = 0;
    let mut y1 = 0;
    for &i in pixels {
        let (x, y) = (i % w, i / w);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0, x1, y1)
}

fn fit_grid(w: usize, h: usize) -> (usize, usize) {
    let scale = (MAX_GRID as f64 / w.max(h) as f64).min(1.0);
    (((w as f64 * scale).round() as usize).max(2), ((h as f64 * scale).round() as usize).max(2))
}

/// Nearest-neighbour resample of a mask to (w1, h1).
fn resample(mask: &[Vec<bool>], w0: usize, h0: usize, w1: usize, h1: usize) -> Vec<Vec<bool>> {
    (0..h1)
        .map(|y| {
            let row = &mask[((y * h0) / h1.max(1)).min(h0 - 1)];
            (0..w1).map(|x| row[((x * w0) / w1.max(1)).min(w0 - 1)]).collect()
        })
        .collect()
}

/// The model outline's size in picture pixels and the mm per pixel that follows. One
/// uniform scale, taken from the dimension the two share — the axes are never stretched
/// apart, which would hide the very mismatch the tool is for. `both` claims both dimensions
/// and scales by width; the result's `aspect_mismatch` says how far the shapes are apart.
/// The fitted size takes the same MAX_GRID cap as the picture's: a narrow model fitted by
/// width against a wide picture would otherwise ask for hundreds of megabytes of mask.
fn fit_model(model: &Silhouette, pw: usize, ph: usize, fit: Fit) -> (usize, usize, f64, f64) {
    let s = match fit {
        Fit::Width | Fit::Both => pw as f64 / model.width as f64,
        Fit::Height => ph as f64 / model.height as f64,
    };
    let mw = ((model.width as f64 * s).round() as usize).max(2);
    let mh = ((model.height as f64 * s).round() as usize).max(2);
    let (mw, mh) = fit_grid(mw, mh);
    (mw, mh, model.width as f64 * model.mm_per_px / mw as f64, model.height as f64 * model.mm_per_px / mh as f64)
}

struct Best {
    overlap: f64,
    dx: i64,
    dy: i64,
}

/// Slide the model mask over the picture mask to the best intersection-over-union. The
/// coarse pass covers the whole range of placements where the two frames can share a
/// pixel — a tenth of the picture around the centre is not enough when the heights differ —
/// and the fine pass walks every offset around its winner.
fn best_overlap(model: &[Vec<bool>], picture: &Part) -> Best {
    let pic = spans_of(&picture.mask);
    let mdl = spans_of(model);
    let model_area: i64 = mdl.iter().flatten().map(|[a, b]| (b - a) as i64).sum();
    let pic_area: i64 = pic.iter().flatten().map(|[a, b]| (b - a) as i64).sum();

    let (mw, mh) = (model[0].len() as i64, model.len() as i64);
    let (lo_x, hi_x) = (1 - mw, picture.width as i64);
    let (lo_y, hi_y) = (1 - mh, picture.height as i64);
    // About 64 coarse samples per axis, never finer than 4 px, never coarser than 32.
    let step_x = ((hi_x - lo_x) / 64).clamp(4, 32);
    let step_y = ((hi_y - lo_y) / 64).clamp(4, 32);

    // The frames' aligned corners: both masks are tightly framed, so the best placement of a
    // matching picture is at or near here.
    let (mut bx, mut by) = (0i64, 0i64);
    for round in 0..2 {
        // Coarse: the whole range at the step; fine: every offset within one coarse step
        // of the coarse winner.
        let (sx, sy, x0, x1, y0, y1) = if round == 0 {
            (step_x, step_y, lo_x, hi_x, lo_y, hi_y)
        } else {
            (1, 1, (bx - step_x).max(lo_x), (bx + step_x).min(hi_x), (by - step_y).max(lo_y), (by + step_y).min(hi_y))
        };
        let (mut best_score, mut best_x, mut best_y) = (i64::MIN, bx, by);
        let mut dy = y0;
        while dy <= y1 {
            let mut dx = x0;
            while dx <= x1 {
                let hits = intersection(&mdl, &pic, dx, dy);
                let union = model_area + pic_area - hits;
                let score = if union <= 0 { i64::MIN } else { hits * 1_000_000 / union };
                if score > best_score {
                    best_score = score;
                    best_x = dx;
                    best_y = dy;
                }
                dx += sx;
            }
            dy += sy;
        }
        bx = best_x;
        by = best_y;
    }

    let hits = intersection(&mdl, &pic, bx, by);
    let union = model_area + pic_area - hits;
    Best { overlap: if union <= 0 { 0.0 } else { hits as f64 / union as f64 }, dx: bx, dy: by }
}

fn spans_of(mask: &[Vec<bool>]) -> Vec<Vec<[usize; 2]>> {
    mask.iter().map(|row| silhouette::spans(row)).collect()
}

/// Pixels the model shares with the picture at model offset (dx, dy), from run-length rows.
fn intersection(mdl: &[Vec<[usize; 2]>], pic: &[Vec<[usize; 2]>], dx: i64, dy: i64) -> i64 {
    let mut hits = 0i64;
    for (r, spans) in mdl.iter().enumerate() {
        if spans.is_empty() {
            continue;
        }
        let Some(y) = (dy + r as i64).try_into().ok().and_then(|y: usize| pic.get(y)) else { continue };
        if y.is_empty() {
            continue;
        }
        for &[a, b] in spans {
            let (mut lo, hi) = ((a as i64 + dx).max(0), b as i64 + dx);
            if hi <= lo {
                continue;
            }
            for &[pa, pb] in y {
                let (s, e) = (lo.max(pa as i64), hi.min(pb as i64));
                if e > s {
                    hits += e - s;
                }
                lo = lo.max(pb as i64);
                if lo >= hi || pb as i64 >= hi {
                    break;
                }
            }
        }
    }
    hits
}

/// The model's edge against the picture's, in mm, at nine stations along one side; positive
/// where the model reaches past the picture, null where a station misses one of the two.
fn edge_offsets(model: &Placed, picture: &Part, side: Edge, mmpp: f64) -> Vec<Value> {
    (1..=STATIONS)
        .map(|i| {
            let station = match side {
                Edge::Top | Edge::Bottom => (picture.width as u64 * i as u64 / (STATIONS as u64 + 1)) as usize,
                Edge::Left | Edge::Right => (picture.height as u64 * i as u64 / (STATIONS as u64 + 1)) as usize,
            };
            let offset = match side {
                Edge::Top | Edge::Bottom => {
                    let pic = column_edge(&picture.mask, station, matches!(side, Edge::Top));
                    let mdl = (station as i64 - model.dx)
                        .try_into()
                        .ok()
                        .and_then(|mx: usize| column_edge(model.mask, mx, matches!(side, Edge::Top)))
                        .map(|e| e as i64 + model.dy);
                    match (mdl, pic) {
                        (Some(m), Some(p)) => Some(match side {
                            Edge::Top => (p as f64 - m as f64) * mmpp,
                            _ => (m as f64 - p as f64) * mmpp,
                        }),
                        _ => None,
                    }
                }
                Edge::Left | Edge::Right => {
                    let pic = row_edge(&picture.mask, station, matches!(side, Edge::Left));
                    let mdl = (station as i64 - model.dy)
                        .try_into()
                        .ok()
                        .and_then(|my: usize| row_edge(model.mask, my, matches!(side, Edge::Left)))
                        .map(|e| e as i64 + model.dx);
                    match (mdl, pic) {
                        (Some(m), Some(p)) => Some(match side {
                            Edge::Left => (p as f64 - m as f64) * mmpp,
                            _ => (m as f64 - p as f64) * mmpp,
                        }),
                        _ => None,
                    }
                }
            };
            offset.map(|mm| json!(round3(mm))).unwrap_or(Value::Null)
        })
        .collect()
}

/// The first (or last, when `first` is false) row holding column `x`; None if none does.
fn column_edge(mask: &[Vec<bool>], x: usize, first: bool) -> Option<usize> {
    let mut found = None;
    for (y, row) in mask.iter().enumerate() {
        if row.get(x).copied().unwrap_or(false) {
            if first {
                return Some(y);
            }
            found = Some(y);
        }
    }
    found
}

/// The first (or last) column set in row `y`; None if the row is empty or missing.
fn row_edge(mask: &[Vec<bool>], y: usize, first: bool) -> Option<usize> {
    let row = mask.get(y)?;
    if first { row.iter().position(|&on| on) } else { row.iter().rposition(|&on| on) }
}

/// The overlay PNG: the picture in blue, the model in red, on a canvas that holds both.
fn overlay_png(picture: &Part, model: &Placed) -> Vec<u8> {
    // A margin so a model that hangs off the picture still fits the canvas.
    let m = 8i64;
    let (x0, y0) = (0.min(model.dx) - m, 0.min(model.dy) - m);
    let (x1, y1) = (
        (picture.width as i64).max(model.dx + model.mask.first().map_or(0, |r| r.len()) as i64) + m,
        (picture.height as i64).max(model.dy + model.mask.len() as i64) + m,
    );
    let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([250, 250, 250, 255]));
    for (y, row) in picture.mask.iter().enumerate() {
        for (x, &on) in row.iter().enumerate() {
            if on {
                img.put_pixel((x as i64 - x0) as u32, (y as i64 - y0) as u32, image::Rgba([70, 110, 230, 255]));
            }
        }
    }
    for (r, row) in model.mask.iter().enumerate() {
        for (c, &on) in row.iter().enumerate() {
            if !on {
                continue;
            }
            let (x, y) = (model.dx + c as i64 - x0, model.dy + r as i64 - y0);
            if (0..w as i64).contains(&x) && (0..h as i64).contains(&y) {
                img.put_pixel(x as u32, y as u32, image::Rgba([235, 60, 60, 255]));
            }
        }
    }
    let mut out = Cursor::new(Vec::new());
    // Encoding a mask-sized PNG cannot realistically fail; if it ever does, no overlay.
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    use image::ImageEncoder as _;
    if enc.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8).is_err() {
        return Vec::new();
    }
    out.into_inner()
}

fn round4(v: f64) -> f64 {
    if v.abs() < 1e12 { (v * 10_000.0).round() / 10_000.0 } else { v }
}

fn round3(v: f64) -> f64 {
    if v.abs() < 1e12 { (v * 1_000.0).round() / 1_000.0 } else { v }
}
