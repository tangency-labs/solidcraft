//! Small, deterministic diagnostic pictures, independent of the application viewport.

use solidcraft_geom::{Vec2, Vec3};

use crate::{Camera, Rgb, Scene, SceneLine, StandardView};

pub struct SketchIssueCurve {
    pub points: Vec<Vec2>,
    pub construction: bool,
    pub highlighted: bool,
}

/// Saved sketch coordinates, not a solved or repaired sketch. Markers carry the report issue
/// number. Missing references are explained by the caller, never placed at invented locations.
pub fn sketch_issue_png(curves: &[SketchIssueCurve], points: &[Vec2], axes: [bool; 2], issue: usize) -> Option<Vec<u8>> {
    const WIDTH: usize = 960;
    const HEIGHT: usize = 640;
    if curves.len() > 256 || curves.iter().any(|c| c.points.len() > 128) || points.len() > 6 || issue > 9999 {
        return None;
    }
    let all: Vec<_> = curves.iter().flat_map(|c| c.points.iter()).chain(points).copied().collect();
    if all.is_empty() || all.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1e9 || p.y.abs() > 1e9) {
        return None;
    }
    let first = *all.first()?;
    let mut low = first;
    let mut high = first;
    for p in &all {
        low.x = low.x.min(p.x);
        low.y = low.y.min(p.y);
        high.x = high.x.max(p.x);
        high.y = high.y.max(p.y);
    }
    if axes[0] || axes[1] {
        low.x = low.x.min(0.0);
        low.y = low.y.min(0.0);
        high.x = high.x.max(0.0);
        high.y = high.y.max(0.0);
    }
    let center = (low + high) * 0.5;
    let span = (high.y - low.y).max((high.x - low.x) / 1.5).max(1.0) * 1.45;
    let camera = Camera { target: Vec3::new(center.x, center.y, 0.0), distance: span, ..Camera::default() };
    let mut camera = camera;
    camera.set_view(StandardView::Top);
    let red = Rgb(190, 42, 38);
    let gray = Rgb(90, 102, 116);
    let mut scene = Scene { radius: span * 3.0, background: Some((Rgb(255, 255, 255), Rgb(255, 255, 255))), ..Scene::default() };
    let mut line = |points: Vec<Vec2>, color, width| {
        scene.lines.push(SceneLine {
            points: points.into_iter().map(|p| Vec3::new(p.x, p.y, 0.0)).collect(),
            color,
            alpha: 1.0,
            width,
            on_top: true,
        });
    };
    // Context first, highlighted entities last, so coincident construction curves stay visible.
    line(
        vec![Vec2::new(center.x - span, 0.0), Vec2::new(center.x + span, 0.0)],
        if axes[0] { red } else { Rgb(205, 215, 225) },
        if axes[0] { 3.0 } else { 1.0 },
    );
    line(
        vec![Vec2::new(0.0, center.y - span), Vec2::new(0.0, center.y + span)],
        if axes[1] { red } else { Rgb(205, 215, 225) },
        if axes[1] { 3.0 } else { 1.0 },
    );
    for highlighted in [false, true] {
        for curve in curves.iter().filter(|c| c.highlighted == highlighted) {
            line(
                curve.points.clone(),
                if highlighted {
                    red
                } else if curve.construction {
                    Rgb(130, 154, 176)
                } else {
                    gray
                },
                if highlighted { 3.0 } else { 1.5 },
            );
        }
    }
    let mut markers = points.to_vec();
    if markers.is_empty() {
        if let Some(curve) = curves.iter().find(|c| c.highlighted) {
            if let Some(point) = curve.points.get(curve.points.len() / 2) {
                markers.push(*point);
            }
        } else if axes[0] || axes[1] {
            markers.push(Vec2::new(0.0, 0.0));
        }
    }
    for point in &markers {
        let r = span / 110.0;
        line(
            (0..=32)
                .map(|i| {
                    let a = f64::from(i) * std::f64::consts::TAU / 32.0;
                    *point + Vec2::new(a.cos(), a.sin()) * r
                })
                .collect(),
            red,
            2.0,
        );
    }
    let mut canvas = crate::render(&scene, &camera, WIDTH, HEIGHT);
    for (index, point) in markers.iter().enumerate() {
        let (x, y, _) = camera.to_screen(Vec3::new(point.x, point.y, 0.0), WIDTH as f64, HEIGHT as f64, scene.radius)?;
        let label_x = (x as i32 + 18).clamp(4, WIDTH as i32 - 90);
        let label_y = (y as i32 - 26 - (index as i32 % 2) * 18).clamp(4, HEIGHT as i32 - 22);
        label(&mut canvas, &issue.to_string(), label_x, label_y, [190, 42, 38, 255]);
    }
    for (axis, name, position) in [(0, "X", Vec3::new(center.x + span * 0.48, 0.0, 0.0)), (1, "Y", Vec3::new(0.0, center.y + span * 0.32, 0.0))] {
        if let Some((x, y, _)) = camera.to_screen(position, WIDTH as f64, HEIGHT as f64, scene.radius)
            && (10.0..WIDTH as f64 - 30.0).contains(&x)
            && (10.0..HEIGHT as f64 - 30.0).contains(&y)
        {
            label(
                &mut canvas,
                name,
                x as i32 + 8,
                if axis == 1 { 20 } else { y as i32 + 8 },
                if axes[axis] { [190, 42, 38, 255] } else { [90, 102, 116, 255] },
            );
        }
    }
    crate::png(WIDTH, HEIGHT, canvas.rgba)
}

/// Original small digit masks; no font files or platform font dependency.
fn label(canvas: &mut crate::Canvas, text: &str, x: i32, y: i32, color: [u8; 4]) {
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    for (digit_index, digit) in text.bytes().enumerate() {
        let mask = match digit {
            b'X' => &[5, 5, 2, 5, 5],
            b'Y' => &[5, 5, 2, 2, 2],
            b'0'..=b'9' => {
                let Some(mask) = DIGITS.get(usize::from(digit - b'0')) else { continue };
                mask
            }
            _ => continue,
        };
        for row in -1..6i32 {
            for col in -1..4i32 {
                let ink =
                    usize::try_from(row).ok().and_then(|r| mask.get(r)).is_some_and(|bits| (0..3).contains(&col) && bits & (1 << (2 - col)) != 0);
                for dy in 0..3 {
                    for dx in 0..3 {
                        let px = x + digit_index as i32 * 12 + col * 3 + dx;
                        let py = y + row * 3 + dy;
                        if px >= 0 && py >= 0 && (px as usize) < canvas.w && (py as usize) < canvas.h {
                            let offset = (py as usize * canvas.w + px as usize) * 4;
                            if let Some(pixel) = canvas.rgba.get_mut(offset..offset + 4) {
                                pixel.copy_from_slice(if ink { &color } else { &[255, 255, 255, 255] });
                            }
                        }
                    }
                }
            }
        }
    }
}
