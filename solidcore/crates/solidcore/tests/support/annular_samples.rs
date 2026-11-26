//! Shared geometric samples and closed forms for annular-face selection tests.
//! Included by application tests so their setup contains no geometric calculations.
#![allow(dead_code)] // Each caller uses a subset of these test helpers.

/// Points 0.05 mm from either rim of the annulus with radii 1.75 and 3.5 mm.
pub fn rim_points(z: f64) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for angle in [0.3_f64, 1.0, 2.0] {
        let (sin, cos) = angle.sin_cos();
        for radius in [3.45, 1.8] {
            points.push([radius * cos, radius * sin, z]);
        }
    }
    points
}

/// Volume of that annulus extruded by `height`: pi (3.5² - 1.75²) height.
pub fn annular_volume(height: f64) -> f64 {
    std::f64::consts::PI * (3.5_f64.powi(2) - 1.75_f64.powi(2)) * height
}

/// A 100 x 100 x 5 plate drilled through at r = 1.75, with a 14 mm annular boss.
pub fn plate_and_extended_boss_volume() -> f64 {
    50000.0 - std::f64::consts::PI * 1.75_f64.powi(2) * 5.0 + annular_volume(14.0)
}

/// An interior display-triangle point, off the exact surface when the face curves.
pub fn triangle_centroid([a, b, c]: [[f64; 3]; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (a[i] + b[i] + c[i]) / 3.0)
}
