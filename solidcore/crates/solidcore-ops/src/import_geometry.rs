//! Fieldwise planar reader geometry seam. Numeric operations live in the kernel.
pub use kernel::{scale_value, EllipseSupport};
use solidcore::prism::import_geometry as kernel;
use solidcore_convert as convert;
use solidcore_record::Vec2;
/// Scale coordinates with the reader's original ordinary multiplication.
pub fn scale_point(point: Vec2, scale: f64) -> Vec2 {
    convert::record_vec2(kernel::scale_point(convert::vec2(point), scale))
}
/// Whether unit conversion collapsed a nonzero coordinate.
pub fn collapsed_coordinate(source: Vec2, scaled: Vec2) -> bool {
    kernel::collapsed_coordinate(convert::vec2(source), convert::vec2(scaled))
}
/// Admit the supporting ellipse without changing either axis.
pub fn ellipse_support(center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport {
    kernel::ellipse_support(convert::vec2(center), convert::vec2(major), ratio)
}

/// Transform a retained source anchor with the kernel's original FMA order.
pub fn transform_anchor(point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
    convert::record_vec2(kernel::transform_anchor(
        convert::vec2(point),
        scale,
        convert::vec2(translation),
    ))
}
