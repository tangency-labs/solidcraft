//! A model's footprint for a display picture: the shadow a preview draws under it.

use solidcore_record::Vec3;

use crate::{Camera, Scene};

/// Where a footprint mask lies against its picture.
#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    /// The picture's width and height in pixels.
    pub picture: [usize; 2],
    /// The mask's width and height in cells.
    pub mask: [usize; 2],
    /// The area the mask covers, in the picture's pixels: left, top, width and height.
    pub area: [f64; 4],
}

/// Every triangle of `scene` dropped onto its lowest level and filled into the mask, as `camera`
/// sees it. A display picture only: nothing about the model is decided from it.
pub fn footprint(scene: &Scene, camera: &Camera, layout: Footprint) -> Option<Vec<u8>> {
    let floor = scene.meshes.iter().flat_map(|m| m.mesh.positions.iter()).map(|p| p.z).filter(|z| z.is_finite()).fold(f64::INFINITY, f64::min);
    if !floor.is_finite() {
        return None;
    }
    let (right, up, _) = camera.basis();
    let [width, height] = layout.mask;
    let [left, top, across, down] = layout.area;
    if width == 0 || height == 0 || layout.picture[1] == 0 || across <= 0.0 || down <= 0.0 {
        return None;
    }
    // World units to a picture pixel.
    let unit = camera.half_height() * 2.0 / layout.picture[1] as f64;
    let at = |p: Vec3| {
        let from = Vec3::new(p.x - camera.target.x, p.y - camera.target.y, floor - camera.target.z);
        let x = layout.picture[0] as f64 * 0.5 + (from.x * right.x + from.y * right.y + from.z * right.z) / unit;
        let y = layout.picture[1] as f64 * 0.5 - (from.x * up.x + from.y * up.y + from.z * up.z) / unit;
        [(x - left) * width as f64 / across, (y - top) * height as f64 / down]
    };
    let mut mask = vec![0u8; width.checked_mul(height)?];
    for mesh in &scene.meshes {
        for t in &mesh.mesh.triangles {
            let corner = |k: usize| t.get(k).and_then(|&i| mesh.mesh.positions.get(i as usize)).map(|&p| at(p));
            let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else { continue };
            fill(&mut mask, layout.mask, [a, b, c]);
        }
    }
    Some(mask)
}

/// Fill the mask's cells whose centres lie in the triangle, and the one at its middle, so that
/// triangles smaller than a cell still leave their mark.
fn fill(mask: &mut [u8], [width, height]: [usize; 2], [a, b, c]: [[f64; 2]; 3]) {
    let mut set = |x: usize, y: usize| {
        if x < width
            && let Some(cell) = mask.get_mut(y * width + x)
        {
            *cell = 255;
        }
    };
    let middle = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
    if middle[0] >= 0.0 && middle[1] >= 0.0 {
        set(middle[0] as usize, middle[1] as usize);
    }
    let span = |k: usize, most: usize| {
        let low = a[k].min(b[k]).min(c[k]).clamp(0.0, most as f64) as usize;
        let high = a[k].max(b[k]).max(c[k]).clamp(0.0, most as f64).ceil() as usize;
        low..high.min(most)
    };
    let edge = |p: [f64; 2], q: [f64; 2], x: f64, y: f64| (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0]);
    for y in span(1, height) {
        for x in span(0, width) {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            let sides = [edge(a, b, cx, cy), edge(b, c, cx, cy), edge(c, a, cx, cy)];
            if sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0) {
                set(x, y);
            }
        }
    }
}
