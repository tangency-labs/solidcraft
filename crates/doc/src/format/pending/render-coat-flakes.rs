//! A saved clear coat in a render setup can carry metallic paint flakes under it (`flakes` in
//! the coat: `coverage`, `size_mm`, `spread`, `depth`, `color` as sRGB, `metallic` and
//! `roughness`). Designs without the field have no flakes, and a coat without flakes saves as
//! before. The last release warns about the newer design format, cannot read a coat with
//! flakes, so it leaves the render setup out, and keeps it as it came when saving.
