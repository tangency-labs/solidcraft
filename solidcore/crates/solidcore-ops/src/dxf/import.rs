//! Reading DXF: a source's unit scale and the checks a read entity must pass, through the
//! kernel's `prism::{transform, arc_conversion}`. Numeric operations live in the kernel.
use solidcore::prism::arc_conversion::ellipse_support as kernel_ellipse_support;
pub use solidcore::prism::arc_conversion::{finite_degrees, EllipseSupport};
use solidcore::prism::transform as kernel;
pub use solidcore::prism::transform::scale_value;
use solidcore_convert as convert;
use solidcore_record::Vec2;
/// Scale coordinates with the reader's original ordinary multiplication.
pub fn scale_point(point: Vec2, scale: f64) -> Vec2 {
    convert::record_vec2(kernel::scale_point(convert::vec2(point), scale))
}
/// Whether unit conversion collapsed a nonzero coordinate: a comparison, no geometry.
pub fn collapsed_coordinate(source: Vec2, scaled: Vec2) -> bool {
    (source.x != 0.0 && scaled.x == 0.0) || (source.y != 0.0 && scaled.y == 0.0)
}
/// Admit the supporting ellipse without changing either axis.
pub fn ellipse_support(center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport {
    kernel_ellipse_support(convert::vec2(center), convert::vec2(major), ratio)
}
