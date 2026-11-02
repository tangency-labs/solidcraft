//! Additive (FFF) G-code import and layer simulation helpers.
//!
//! Part Handling / Inspection emit machine macros via the post; additive setups either
//! generate a simple layered toolpath or import an existing `*.gcode` print file and
//! replay G0/G1 moves for Simulate Additive Toolpath.

use crate::PathSegment;

/// Soft cap on imported G-code retained in the document (bytes).
pub const MAX_GCODE_BYTES: usize = 256 * 1024;
/// Soft cap on simulated segments from imported G-code.
pub const MAX_SIM_SEGMENTS: usize = 8_192;

/// Parse a subset of FFF G-code (G0/G1 with X/Y/Z/F) into path segments for simulation.
pub fn segments_from_gcode(gcode: &str) -> Vec<PathSegment> {
    let mut out = Vec::new();
    let mut x = 0.0_f64;
    let mut y = 0.0_f64;
    let mut z = 0.0_f64;
    let mut feed = 1_200.0_f64;
    for raw in gcode.lines() {
        if out.len() >= MAX_SIM_SEGMENTS {
            break;
        }
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        let is_rapid = upper.contains("G0") || upper.contains("G00");
        let is_linear = upper.contains("G1") || upper.contains("G01");
        if !is_rapid && !is_linear {
            continue;
        }
        let mut nx = x;
        let mut ny = y;
        let mut nz = z;
        let mut nf = feed;
        for tok in upper.split_whitespace() {
            if tok.len() < 2 {
                continue;
            }
            let (axis, rest) = tok.split_at(1);
            let Ok(v) = rest.parse::<f64>() else {
                continue;
            };
            match axis {
                "X" => nx = v,
                "Y" => ny = v,
                "Z" => nz = v,
                "F" if v > 0.0 && v.is_finite() => nf = v,
                _ => {}
            }
        }
        if !nx.is_finite() || !ny.is_finite() || !nz.is_finite() {
            continue;
        }
        let to = [nx, ny, nz];
        if is_rapid {
            out.push(PathSegment::Rapid { to });
        } else {
            feed = nf;
            out.push(PathSegment::Linear { to, feed });
        }
        x = nx;
        y = ny;
        z = nz;
    }
    out
}

/// Generate a simple rectangular-layer fill for a generated additive toolpath.
pub fn segments_for_generated(layer_height_mm: f64, layer_count: u32, size_xy_mm: f64, feedrate_mmpm: f64, travel_mmpm: f64) -> Vec<PathSegment> {
    let lh = if layer_height_mm > 0.0 && layer_height_mm.is_finite() { layer_height_mm } else { 0.2 };
    let layers = layer_count.clamp(1, 256);
    let size = if size_xy_mm > 0.0 && size_xy_mm.is_finite() { size_xy_mm } else { 40.0 };
    let feed = if feedrate_mmpm > 0.0 && feedrate_mmpm.is_finite() { feedrate_mmpm } else { 1_200.0 };
    let travel = if travel_mmpm > 0.0 && travel_mmpm.is_finite() { travel_mmpm } else { 3_000.0 };
    let step = (size / 8.0).max(0.5);
    let mut out = Vec::new();
    out.push(PathSegment::Rapid { to: [0.0, 0.0, lh] });
    for layer in 0..layers {
        let z = lh * f64::from(layer + 1);
        out.push(PathSegment::Rapid { to: [0.0, 0.0, z] });
        let mut y = 0.0;
        let mut flip = false;
        while y <= size + 1e-9 {
            let (x0, x1) = if flip { (size, 0.0) } else { (0.0, size) };
            out.push(PathSegment::Linear { to: [x0, y, z], feed: travel });
            out.push(PathSegment::Linear { to: [x1, y, z], feed });
            y += step;
            flip = !flip;
            if out.len() >= MAX_SIM_SEGMENTS {
                return out;
            }
        }
    }
    out
}

/// Truncate G-code for document storage.
pub fn truncate_gcode(src: &str) -> String {
    if src.len() <= MAX_GCODE_BYTES {
        return src.to_string();
    }
    let mut end = MAX_GCODE_BYTES;
    while end > 0 && !src.is_char_boundary(end) {
        end -= 1;
    }
    let mut s = src[..end].to_string();
    s.push_str("\n; … truncated for document storage\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_g0_g1() {
        let g = "G0 X0 Y0 Z0.2\nG1 X10 Y0 F1200\nG1 X10 Y10\n";
        let segs = segments_from_gcode(g);
        assert!(segs.len() >= 3);
    }

    #[test]
    fn generated_has_layers() {
        let segs = segments_for_generated(0.2, 3, 20.0, 1000.0, 3000.0);
        assert!(segs.len() > 5);
    }
}
