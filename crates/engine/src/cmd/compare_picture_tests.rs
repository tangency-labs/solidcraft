//! Tests for `inspect.compare_picture` (#1486): synthetic reference pictures drawn in code,
//! written to the temp dir and read back through the vfs like any other reference.

use std::io::Cursor;

use serde_json::{Value, json};

use super::super::Session;
use image::ImageEncoder as _;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn box_body(s: &mut Session, name: &str, x1: f64, y1: f64, z: f64) {
    let sketch = format!("{name}Sketch");
    run(s, "sketch.create", json!({"plane": "XY", "name": sketch}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [x1, y1]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": sketch, "distance": z, "operation": "new_body", "body_name": name}));
}

/// A PNG whose pixels come from `paint`.
fn png(w: usize, h: usize, paint: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    png_rgba(w, h, |x, y| {
        let [r, g, b] = paint(x, y);
        [r, g, b, 255]
    })
}

/// A PNG with alpha, whose pixels come from `paint`.
fn png_rgba(w: usize, h: usize, paint: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x as u32, y as u32, image::Rgba(paint(x, y)));
        }
    }
    let mut out = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut out).write_image(img.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgba8).unwrap();
    out.into_inner()
}

fn jpeg(w: usize, h: usize, paint: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    jpeg_q(w, h, 90, paint)
}

fn jpeg_q(w: usize, h: usize, q: u8, paint: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = paint(x, y);
            img.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, 255]));
        }
    }
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q)
        .write_image(rgb.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgb8)
        .unwrap();
    out.into_inner()
}

/// Write a reference into the temp dir and return its path.
fn reference(name: &str, bytes: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("solidcraft-compare-picture-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

/// A white rectangle on a grey backdrop: `x0..x1, y0..y1` in pixels.
fn rect_picture(w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<u8> {
    png(w, h, |x, y| if (x0..x1).contains(&x) && (y0..y1).contains(&y) { [255, 255, 255] } else { [200, 200, 200] })
}

fn edges(v: &Value) -> Vec<Vec<Option<f64>>> {
    ["top", "bottom", "left", "right"]
        .iter()
        .map(|side| v["edges"][side].as_array().unwrap().iter().map(|e| e.as_f64()).collect::<Vec<Option<f64>>>())
        .collect()
}

fn near(v: Option<f64>, want: f64, tol: f64) -> bool {
    v.is_some_and(|e| (e - want).abs() <= tol)
}

#[test]
fn a_matching_picture_scores_high_with_flat_edges() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0); // front view: 20 wide, 30 tall
    let path = reference("match.png", &rect_picture(200, 280, 20, 20, 180, 260));
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "overlap {}", out["overlap"]);
    for side in edges(&out) {
        for e in side {
            assert!(near(e, 0.0, 0.6), "edge {e:?}");
        }
    }
    assert!(!out["overlay_png"].as_str().unwrap().is_empty());
}

#[test]
fn a_step_in_the_bottom_edge_shows_up_in_mm() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    // The aligned picture: a plain part 160 x 240 px — the model's own shape at 8 px per mm.
    let aligned_path = reference("aligned.png", &rect_picture(200, 280, 20, 20, 180, 260));
    let aligned = run(&mut s, "inspect.compare_picture", json!({"reference": aligned_path, "view": "front"}));
    // The stepped picture: over its right half the part's bottom edge sits 35 px higher.
    // The width fit gives 8 px per mm, so 35 px is 4.375 mm of the model reaching past
    // the picture.
    let stepped_path = reference(
        "stepped.png",
        &png(200, 280, |x, y| {
            let bottom = if x >= 100 { 225 } else { 260 };
            if (20..180).contains(&x) && (20..bottom).contains(&y) { [255, 255, 255] } else { [200, 200, 200] }
        }),
    );
    let stepped = run(&mut s, "inspect.compare_picture", json!({"reference": stepped_path, "view": "front"}));
    let flat = edges(&aligned)[1].clone();
    let step = edges(&stepped)[1].clone();
    for e in flat.iter() {
        assert!(near(*e, 0.0, 0.6), "flat bottom {e:?}");
    }
    // Stations sit at local x = 16..144 px; the notch starts at local x = 80 (global 100),
    // so stations 5..9 are over it.
    for e in step.iter().take(4) {
        assert!(near(*e, 0.0, 0.6), "step bottom left {e:?}");
    }
    for e in step.iter().skip(4) {
        assert!(near(*e, 4.375, 0.6), "step bottom right {e:?}");
    }
    assert!(stepped["overlap"].as_f64().unwrap() < aligned["overlap"].as_f64().unwrap());
}

/// #1486's example, pinned: `examples/bracket.json` as it is (upright 30 mm) against the
/// same bracket at 40 mm, both from the front, scaled by the 40 mm arm they share.
/// The issue's prototype numbers: overlap 0.865, bottom −9.9 mm at the upright (6 % across),
/// 0.0 mm everywhere else.
#[test]
fn the_issue_bracket_pins_overlap_and_the_short_upright() {
    let mut s = Session::default();
    // The bracket profile of examples/bracket.json: a 40 x 6 arm with a 6-wide upright to
    // 30 mm, on XZ so the front view shows the upright hanging below the arm.
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Profile"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 6], [6, 6], [6, 30], [0, 30]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20.0, "direction": "negative", "body_name": "Bracket"}));
    // The same bracket with the upright at 40 mm, at 20 px per mm: an 800 x 800 part, the
    // 800 x 120 arm on top and the 120-wide upright down to 800, on a grey margin.
    let path = reference(
        "bracket.png",
        &png(840, 840, |x, y| {
            let part = ((20..820).contains(&x) && (20..140).contains(&y)) || ((20..140).contains(&x) && (20..820).contains(&y));
            if part { [255, 255, 255] } else { [200, 200, 200] }
        }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "rows": 480, "overlay": false}));
    let overlap = out["overlap"].as_f64().unwrap();
    assert!((overlap - 0.865).abs() <= 0.01, "overlap {overlap}");
    let e = edges(&out);
    // The upright is the first station (x = 80 of 800): the model is 10 mm short there.
    assert!(near(e[1][0], -10.0, 0.2), "bottom at the upright {:?}", e[1][0]);
    for side in 1..9 {
        assert!(near(e[1][side], 0.0, 0.1), "bottom elsewhere {:?}", e[1]);
    }
    for i in 0..9 {
        assert!(near(e[0][i], 0.0, 0.1), "top {:?}", e[0]);
        // Below the model's 30 mm upright the model has no row to answer with: those
        // side stations are null, as documented.
        for side in [e[2].clone(), e[3].clone()] {
            match side[i] {
                None => {}
                Some(v) => assert!(v.abs() <= 0.1, "side {side:?}"),
            }
        }
    }
}

#[test]
fn a_gradient_backdrop_still_separates_from_the_part() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    // A backdrop that shifts one step per row: the border flood follows it; the step at the
    // part's outline (to white) stops it.
    let path = reference(
        "gradient.png",
        &png(200, 300, |x, y| if (20..180).contains(&x) && (20..280).contains(&y) { [255, 255, 255] } else { [200 + (y / 30) as u8, 200, 200] }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.9, "overlap {}", out["overlap"]);
}

#[test]
fn a_jpeg_reference_works_too() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    let path =
        reference("match.jpg", &jpeg(200, 300, |x, y| if (20..180).contains(&x) && (20..280).contains(&y) { [255, 255, 255] } else { [40, 40, 40] }));
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.9, "overlap {}", out["overlap"]);
}

#[test]
fn crop_takes_one_part_off_the_board() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    // Two parts side by side; the crop keeps the left one, a plain rectangle that matches
    // the model. The right part is bigger overall but L-shaped, so without the crop the
    // largest piece wins and the score drops.
    let path = reference(
        "two.png",
        &png(400, 300, |x, y| {
            let left = (20..180).contains(&x) && (20..260).contains(&y);
            let right = (210..390).contains(&x) && (5..295).contains(&y) && !((300..390).contains(&x) && (100..200).contains(&y));
            if left || right { [255, 255, 255] } else { [200, 200, 200] }
        }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path.clone(), "view": "front", "crop_fraction": [0.0, 0.0, 0.5, 1.0]}));
    // The picture size is the found part's tight frame: the left rectangle is 160 x 240 px.
    assert_eq!(out["picture"]["width"], 160);
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "overlap {}", out["overlap"]);
    let whole = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front"}));
    assert!(whole["overlap"].as_f64().unwrap() < out["overlap"].as_f64().unwrap());
}

#[test]
fn crop_px_has_explicit_units() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    let path = reference("px.png", &rect_picture(200, 280, 20, 20, 180, 260));
    // Pixels mean pixels: [10, 10, 180, 260] keeps the part with a margin, and a crop of
    // one pixel is one pixel, not the whole picture.
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path.clone(), "view": "front", "crop_px": [10, 10, 180, 260]}));
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "overlap {}", out["overlap"]);
    let dot = reference("dot.png", &rect_picture(200, 280, 100, 100, 101, 101));
    assert!(
        s.execute("inspect.compare_picture", &json!({"reference": dot, "crop_px": [0, 0, 1, 1]})).unwrap_err().to_string().contains("no part found")
    );
    // Giving both spellings is refused, and so is a fraction above 1.
    assert!(
        s.execute("inspect.compare_picture", &json!({"reference": path, "crop_px": [0, 0, 10, 10], "crop_fraction": [0.0, 0.0, 0.1, 0.1]}))
            .unwrap_err()
            .to_string()
            .contains("not both")
    );
    assert!(
        s.execute("inspect.compare_picture", &json!({"reference": path, "crop_fraction": [0.0, 0.0, 1.5, 1.0]}))
            .unwrap_err()
            .to_string()
            .contains("0..1")
    );
}

#[test]
fn fit_both_never_stretches_the_axes_apart() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0); // front view: 20 wide, 30 tall
    // A 20 x 20 mm square picture of a 20 x 30 mm model: `both` used to stretch the model
    // to the square and report a perfect 1.000 with every edge at 0.0 mm. One uniform
    // scale (from the width they share) leaves the 10 mm of overhang visible instead.
    let path = reference("square.png", &rect_picture(220, 220, 10, 10, 210, 210));
    let both = run(&mut s, "inspect.compare_picture", json!({"reference": path.clone(), "view": "front", "fit": "both", "overlay": false}));
    assert!((both["overlap"].as_f64().unwrap() - 200.0 / 300.0).abs() < 0.02, "overlap {}", both["overlap"]);
    assert!((both["aspect_mismatch"].as_f64().unwrap() - 1.5).abs() < 0.01, "aspect_mismatch {}", both["aspect_mismatch"]);
    // And `width` is the default: leaving `fit` out answers the same as naming it.
    let plain = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    assert_eq!(plain["overlap"], both["overlap"]);
    assert_eq!(plain["edges"], both["edges"]);
}

#[test]
fn fit_width_keeps_the_models_aspect() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    // The picture part is 160 x 160 px: fit=width scales the model uniformly to 160 x 240,
    // so it hangs 80 px (10 mm) over the picture's height in total, wherever the slide puts
    // it. The overlap of a contained model is 160/240 wherever it sits.
    let path = reference("short.png", &rect_picture(200, 220, 20, 20, 180, 180));
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "fit": "width"}));
    assert!((out["overlap"].as_f64().unwrap() - 160.0 / 240.0).abs() < 0.05, "overlap {}", out["overlap"]);
    let bottom: Vec<Option<f64>> = out["edges"]["bottom"].as_array().unwrap().iter().map(|e| e.as_f64()).collect();
    let top: Vec<Option<f64>> = out["edges"]["top"].as_array().unwrap().iter().map(|e| e.as_f64()).collect();
    for i in 0..9 {
        // The overhang splits between top and bottom but always totals the extra 10 mm.
        assert!(bottom[i].unwrap() >= 0.0 && top[i].unwrap() >= 0.0, "bottom {:?} top {:?}", bottom[i], top[i]);
        assert!((bottom[i].unwrap() + top[i].unwrap() - 10.0).abs() < 0.6, "bottom {:?} top {:?}", bottom[i], top[i]);
    }
}

#[test]
fn a_narrow_model_fits_the_grid_not_the_memory() {
    let mut s = Session::default();
    // A 2 mm x 1000 mm sliver seen from the front: fitted by width against a 1024-wide
    // picture it used to become 1024 x 524 288 booleans. The fitted model now takes the
    // same MAX_GRID cap as the picture, so the comparison just runs.
    box_body(&mut s, "Sliver", 2.0, 1.0, 1000.0);
    let path = reference("sliver.png", &rect_picture(1030, 1010, 3, 5, 1027, 1005));
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    // The shapes are nothing alike (the sliver is 1:500, the picture 1024:1000), and the
    // capped model grid keeps the overlap small — what matters is that it ran at all.
    assert!(out["overlap"].as_f64().unwrap() < 0.1, "overlap {}", out["overlap"]);
}

#[test]
fn ordinary_pictures_keep_their_part() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    // A part touching the picture's border: only border pixels close to the border's main
    // colour seed the background, so the part's own border pixels do not flood it.
    let border = reference("border.png", &rect_picture(200, 280, 0, 20, 160, 260));
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": border, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "border overlap {}", out["overlap"]);
    // A white part on a (245, 245, 245) studio backdrop: the step of 10 is below BG_STEP,
    // but the stricter retry separates it.
    let studio = reference(
        "studio.png",
        &png(200, 280, |x, y| if (20..180).contains(&x) && (20..260).contains(&y) { [255, 255, 255] } else { [245, 245, 245] }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": studio, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "studio overlap {}", out["overlap"]);
    // A part on a transparent background: alpha below 128 is background.
    let alpha = reference(
        "alpha.png",
        &png_rgba(200, 280, |x, y| if (20..180).contains(&x) && (20..260).contains(&y) { [90, 90, 90, 255] } else { [0, 0, 0, 0] }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": alpha, "view": "front"}));
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "alpha overlap {}", out["overlap"]);
}

#[test]
fn a_through_hole_counts_the_same_on_both_sides() {
    let mut s = Session::default();
    // A 200 x 200 mm plate with a 100 x 100 mm through-hole, against a picture of itself
    // at 1 px per mm, seen from the top (the hole goes through the plate's faces). The
    // picture's hole is filled by the border flood; the model's hole now gets the same
    // rule, so the plate scores ~1.0 instead of 0.75.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [200.0, 200.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Plate", "distance": 10.0, "operation": "new_body", "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [50.0, 50.0], "p1": [150.0, 150.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "distance": 20.0, "operation": "cut"}));
    let path = reference(
        "plate.png",
        &png(220, 220, |x, y| {
            let plate = (10..210).contains(&x) && (10..210).contains(&y);
            let hole = (60..160).contains(&x) && (60..160).contains(&y);
            if plate && !hole { [255, 255, 255] } else { [200, 200, 200] }
        }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "top", "overlay": false}));
    assert!(out["overlap"].as_f64().unwrap() > 0.98, "overlap {}", out["overlap"]);
    for side in edges(&out) {
        for e in side {
            assert!(near(e, 0.0, 0.6), "edge {e:?}");
        }
    }
}

#[test]
fn hostile_and_empty_references_fail_cleanly() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    assert!(s.execute("inspect.compare_picture", &json!({})).is_err());
    // Not an image at all.
    let text = reference("not.png", b"just text, not a picture");
    assert!(s.execute("inspect.compare_picture", &json!({"reference": text})).unwrap_err().to_string().contains("PNG or a JPEG"));
    // A uniform picture: the background eats everything, and says so.
    let flat = reference("flat.png", &png(64, 64, |_, _| [200, 200, 200]));
    assert!(s.execute("inspect.compare_picture", &json!({"reference": flat})).unwrap_err().to_string().contains("no part found"));
    // A fully transparent picture: alpha says there is nothing there.
    let clear = reference("clear.png", &png_rgba(64, 64, |_, _| [0, 0, 0, 0]));
    assert!(s.execute("inspect.compare_picture", &json!({"reference": clear})).unwrap_err().to_string().contains("transparent"));
    // A missing file.
    assert!(s.execute("inspect.compare_picture", &json!({"reference": "no-such-file.png"})).is_err());
    // A bad fit name.
    let path = reference("match2.png", &rect_picture(200, 280, 20, 20, 180, 260));
    assert!(s.execute("inspect.compare_picture", &json!({"reference": path, "fit": "diagonal"})).is_err());
    // overlay: false leaves the PNG out.
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    assert!(out.get("overlay_png").is_none());
}

/// Reviewer re-test case: a 60 x 60 opaque PNG with a white 40 x 40 square and a single
/// backdrop pixel at alpha 254 used to be read as all part — 1.000 against the 40 x 40
/// square at 0.067 mm/px instead of 0.1. Alpha now decides only when the border is mostly
/// see-through, and a part drawn at alpha 100 on a transparent backdrop is still the part.
#[test]
fn one_translucent_pixel_does_not_switch_to_alpha() {
    let mut s = Session::default();
    box_body(&mut s, "Square", 4.0, 1.0, 4.0); // front view: 4 x 4 mm
    let path = reference(
        "speck.png",
        &png_rgba(60, 60, |x, y| {
            let alpha = if x == 5 && y == 55 { 254 } else { 255 };
            let c = if (10..50).contains(&x) && (10..50).contains(&y) { 255u8 } else { 128 };
            [c, c, c, alpha]
        }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    // The part is the 40 x 40 square: 4 mm over 40 px is 0.1 mm/px, not 4/60 = 0.067.
    let mmpp = out["mm_per_px"]["x"].as_f64().unwrap();
    assert!((mmpp - 0.1).abs() < 0.01, "mm_per_px {mmpp}");
    assert!(out["overlap"].as_f64().unwrap() > 0.9, "overlap {}", out["overlap"]);
    // A part drawn at alpha 100 on a transparent backdrop is the part, not the background.
    let faint = reference(
        "faint.png",
        &png_rgba(60, 60, |x, y| if (10..50).contains(&x) && (10..50).contains(&y) { [255, 255, 255, 100] } else { [0, 0, 0, 0] }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": faint, "view": "front", "overlay": false}));
    assert!(out["overlap"].as_f64().unwrap() > 0.9, "faint overlap {}", out["overlap"]);
    let mmpp = out["mm_per_px"]["x"].as_f64().unwrap();
    assert!((mmpp - 0.1).abs() < 0.01, "faint mm_per_px {mmpp}");
}

/// Reviewer re-test case: one view cropped off a board — the part touches three sides and
/// takes 270 of the 400 border samples, so the border's main colour is the part's own and
/// the flood eats it: the 15 px backdrop strip becomes "the part" and an exact model scores
/// 0.18. `part_fills_crop: true` says the crop box is the part: no flood, exact model ~1.0.
#[test]
fn a_tight_crop_can_declare_the_box_is_the_part() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 17.0, 10.0, 20.0); // front view: 17 wide, 20 tall
    // The tight crop: the white part fills x 0..85 of a 100 x 100 box, touching three
    // sides — 270 of the border samples — with a 15 px backdrop strip down the right.
    let path = reference("tight.png", &png(100, 100, |x, _| if x < 85 { [255, 255, 255] } else { [200, 200, 200] }));
    let flood = run(&mut s, "inspect.compare_picture", json!({"reference": path.clone(), "view": "front", "overlay": false}));
    assert!(flood["overlap"].as_f64().unwrap() < 0.5, "the flood keeps the strip: overlap {}", flood["overlap"]);
    let out = run(
        &mut s,
        "inspect.compare_picture",
        json!({"reference": path, "view": "front", "crop_px": [0, 0, 85, 100], "part_fills_crop": true, "overlay": false}),
    );
    assert!(out["overlap"].as_f64().unwrap() > 0.95, "overlap {}", out["overlap"]);
    // 17 mm over the 85 px the part fills: 0.2 mm/px.
    assert!((out["mm_per_px"]["x"].as_f64().unwrap() - 0.2).abs() < 0.01, "mm_per_px {}", out["mm_per_px"]);
    // A crop of a 2 x 2 box is the part; a 1 px box is refused.
    let dot = reference("tight-dot.png", &png(4, 4, |_, _| [255, 255, 255]));
    assert!(
        s.execute("inspect.compare_picture", &json!({"reference": dot, "part_fills_crop": true, "crop_px": [0, 0, 1, 1]}))
            .unwrap_err()
            .to_string()
            .contains("2 x 2")
    );
}

/// Reviewer re-test case: holes are filled on both sides, so a model missing a 100 x 100
/// through-hole scores ~1.000 against a picture that shows the hole. That is the contract —
/// outline only — and the command description must say so, so an agent does not read 1.000
/// as "matches".
#[test]
fn the_compare_is_outline_only_holes_are_not_compared() {
    assert!(
        crate::cmd::compare_picture::COMMANDS[0].params.contains("holes are not compared"),
        "the command description must say the compare is outline only"
    );
    let mut s = Session::default();
    // A solid 200 x 200 mm plate against a picture of a plate with a 100 x 100 through-hole.
    box_body(&mut s, "Plate", 200.0, 10.0, 200.0);
    let path = reference(
        "hole-only.png",
        &png(220, 220, |x, y| {
            let plate = (10..210).contains(&x) && (10..210).contains(&y);
            let hole = (60..160).contains(&x) && (60..160).contains(&y);
            if plate && !hole { [255, 255, 255] } else { [200, 200, 200] }
        }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    // The outlines match: the hole is not compared, as the description says.
    assert!(out["overlap"].as_f64().unwrap() > 0.98, "overlap {}", out["overlap"]);
}

/// Reviewer re-test case: a white part on (245, 245, 245) saved as JPEG at quality 75 gave
/// "no part" even on the stricter retry — the codec blurs the 10-level step at the part's
/// outline into steps of a few levels each, under any local limit. The third pass takes the
/// backdrop globally: near the border's main colour.
#[test]
fn a_light_jpeg_keeps_its_part() {
    let mut s = Session::default();
    box_body(&mut s, "Box", 20.0, 10.0, 30.0);
    let path = reference(
        "light.jpg",
        &jpeg_q(200, 300, 75, |x, y| if (20..180).contains(&x) && (20..280).contains(&y) { [255, 255, 255] } else { [245, 245, 245] }),
    );
    let out = run(&mut s, "inspect.compare_picture", json!({"reference": path, "view": "front", "overlay": false}));
    assert!(out["overlap"].as_f64().unwrap() > 0.9, "overlap {}", out["overlap"]);
}
