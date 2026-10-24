//! Original illustrative opaque finishes; not measured optical or engineering data.
//! (name, sRGB color, metal fraction, perceptual roughness). IOR is an illustrative 1.5.
//! Separate names preserve the scalar library's appearance and saved assignments.
pub const OPAQUE_FINISHES: [(&str, [u8; 3], f64, f64); 6] = [
    ("Steel - Satin PBR", [158, 164, 172], 1.0, 0.4),
    ("Aluminum - Satin PBR", [204, 208, 214], 1.0, 0.3),
    ("Copper - Clean PBR", [204, 122, 80], 1.0, 0.25),
    ("ABS - Molded PBR", [222, 222, 216], 0.0, 0.4),
    ("PLA - Smooth PBR", [110, 160, 222], 0.0, 0.35),
    ("Wood - Oak Uniform PBR Preview", [190, 146, 92], 0.0, 0.65),
];
