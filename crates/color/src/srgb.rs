//! The sRGB transfer function (IEC 61966-2-1), once: linear light to an sRGB value and back, and the 8-bit codes most
//! callers want. Every encode and decode in Rust goes through here, so a change to the rule is one change. (Shaders
//! can't call Rust and keep their own copies.)

/// Linear light (0..1) to an sRGB value (0..1): straight below the knee at 0.0031308, a 1/2.4 power above it.
pub fn encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
}

/// An sRGB value (0..1) to linear light: straight below the knee at 0.04045, a 2.4 power above it.
pub fn decode(encoded: f64) -> f64 {
    if encoded <= 0.040_45 { encoded / 12.92 } else { ((encoded + 0.055) / 1.055).powf(2.4) }
}

/// Linear light to the nearest 8-bit sRGB code; out of range reads as the nearest end, and NaN as 0.
pub fn encode8(linear: f64) -> u8 {
    (encode(linear).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// An 8-bit sRGB code to linear light.
pub fn decode8(code: u8) -> f64 {
    decode(f64::from(code) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formulas as each caller wrote them before this module: the module must give the same results.
    fn encode_as_written(v: f64) -> f64 {
        if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
    }
    fn decode_as_written(v: f64) -> f64 {
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    #[test]
    fn every_code_decodes_as_before_and_encodes_back_to_itself() {
        for code in 0..=255u8 {
            let linear = decode8(code);
            assert_eq!(linear.to_bits(), decode_as_written(f64::from(code) / 255.0).to_bits(), "{code}");
            assert_eq!(encode8(linear), code, "{code}");
        }
        assert_eq!((decode8(0), decode8(255)), (0.0, 1.0));
        assert!((decode8(10) - (10.0 / 255.0) / 12.92).abs() < 1e-15, "the straight part below the knee");
        assert!((decode8(128) - 0.21586050011389926).abs() < 1e-14);
    }

    #[test]
    fn values_decode_as_every_caller_did() {
        // Either side of the knee (no 8-bit code falls between 0.04 and 0.04045), out of range, and a dense sweep.
        let mut values = vec![0.0404, 0.040_45, 0.040_45_f64.next_up(), -0.5, 1.5];
        values.extend((0..=100_000).map(|i| f64::from(i) / 100_000.0));
        for v in values {
            assert_eq!(decode(v).to_bits(), decode_as_written(v).to_bits(), "{v}");
        }
    }

    #[test]
    fn linear_values_encode_as_every_caller_did() {
        // Below zero, the knees, above one, the extremes and NaN, and a dense sweep in between.
        let mut values = vec![-1.0, -0.0, 0.0, 0.003_130_8, 0.003_130_8_f64.next_up(), 0.04045, 1.0, 1.0f64.next_up(), 2.0];
        values.extend([f64::MIN_POSITIVE, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN]);
        values.extend((0..=200_000).map(|i| f64::from(i) / 100_000.0 - 0.5));
        for v in values {
            assert_eq!(encode(v).to_bits(), encode_as_written(v).to_bits(), "{v}");
            let byte = encode8(v);
            // Clamped after encoding (film_png, raytrace, photographic, presentation, appearance_pack) ...
            assert_eq!(byte, (encode_as_written(v).clamp(0.0, 1.0) * 255.0).round() as u8, "{v}");
            // ... before it (engine's render library) ...
            let first = (encode_as_written(v.clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8;
            // ... or not at all, the cast saturating (render's stage).
            let none = (encode_as_written(v) * 255.0).round() as u8;
            assert_eq!((byte, byte), (first, none), "{v}");
        }
    }
}
