//! Built-in physical materials: densities in g/cm³ and how each looks.

/// The default material's name: what a body without a usable material of its own is made of.
/// Assigning it clears a body's assignment.
pub const DEFAULT_MATERIAL: &str = "Default";
/// Density of the default material (g/cm³).
pub const DEFAULT_DENSITY: f64 = 1.29;

/// Physical materials: name, density (g/cm³) and look, an sRGB colour and an opacity. The default
/// material has no look of its own.
pub const MATERIALS: &[(&str, f64, Option<([u8; 3], f64)>)] = &[
    (DEFAULT_MATERIAL, DEFAULT_DENSITY, None),
    ("Steel", 7.85, Some(([158, 164, 172], 1.0))),
    ("Stainless Steel", 8.0, Some(([186, 191, 198], 1.0))),
    ("Aluminum", 2.70, Some(([204, 208, 214], 1.0))),
    ("Brass", 8.50, Some(([208, 172, 88], 1.0))),
    ("Copper", 8.96, Some(([204, 122, 80], 1.0))),
    ("Titanium", 4.43, Some(([150, 148, 158], 1.0))),
    ("Cast Iron", 7.20, Some(([104, 104, 110], 1.0))),
    ("ABS Plastic", 1.06, Some(([222, 222, 216], 1.0))),
    ("PLA", 1.24, Some(([110, 160, 222], 1.0))),
    ("Nylon", 1.14, Some(([236, 230, 214], 1.0))),
    ("Polycarbonate", 1.20, Some(([196, 216, 232], 1.0))),
    ("Oak", 0.75, Some(([190, 146, 92], 1.0))),
    ("Glass", 2.50, Some(([170, 208, 218], 0.25))),
];
