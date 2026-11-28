//! The router's round-tube copes ([`solidcraft_kernel_api::tube_cope::TubeCopes`]), as
//! `solidcraft_kernel::tube_cope`: a tube's straight end cut to fit the outside cylinders of
//! mating tubes, and the cut unwrapped as a 1:1 template.

use solidcore_record::{Plane, Vec3};
use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::tube_cope::TubeCopes;

pub use solidcore_record::tube_cope::{Box3, Cope, Fit, Mate, Template, Terminal};
pub use solidcraft_kernel_api::tube_cope::{CopeFailed, CopeRefusal as Refusal, TemplateRefusal};

use super::{Which, which};
use crate::backend::Solidcore;

/// The kernel that runs copes.
fn kernel() -> Solidcore {
    match which(OpKind::TubeCope) {
        Which::Solidcore => Solidcore,
    }
}

/// The smallest joint sine a cope takes: the kernel's. This build holds solidcore only.
pub const MIN_SINE: f64 = <Solidcore as TubeCopes>::MIN_SINE;

/// The end of a swept path as a [`Terminal`]: from the section plane there (its normal along the
/// path) and the straight length from it; `at_end` for the path's end.
///
/// # Errors
/// The kernel's refusal of a plane that is not orthonormal or a length out of range.
pub fn terminal_at(plane: &Plane, length: f64, at_end: bool) -> Result<Terminal, CopeFailed> {
    kernel().terminal_at(plane, length, at_end)
}

/// Whether `t` is a usable end.
#[must_use]
pub fn terminal_ok(t: &Terminal) -> bool {
    kernel().terminal_ok(t)
}

/// Whether `m` is a usable mate cylinder.
#[must_use]
pub fn mate_ok(m: &Mate) -> bool {
    kernel().mate_ok(m)
}

/// A box holding a straight swept piece from `a` to `b`, `pad` round it.
#[must_use]
pub fn line_box(a: Vec3, b: Vec3, pad: f64) -> Box3 {
    kernel().line_box(a, b, pad)
}

/// A box holding a circular bend from `a` about `center`, `pad` round it.
#[must_use]
pub fn arc_box(a: Vec3, center: Vec3, pad: f64) -> Box3 {
    kernel().arc_box(a, center, pad)
}

/// Mate `m` fitted to end `t` of a tube of outside radius `radius` whose other swept pieces lie in
/// `others`, or why it cannot cope it.
///
/// # Errors
/// The kernel's refusal of an end, mate or radius out of range.
pub fn fit(t: &Terminal, radius: f64, m: &Mate, others: &[Box3]) -> Result<Result<Fit, Refusal>, CopeFailed> {
    kernel().fit(t, radius, m, others)
}

/// `cope` unwrapped from `seam` with at most `tolerance` mm of error in at most `max_segments`
/// segments, or why it cannot be.
///
/// # Errors
/// The kernel's refusal of an end, radius, mate, seam or tolerance out of range.
pub fn template(
    cope: &Cope<'_>,
    seam: f64,
    tolerance: f64,
    max_segments: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Result<Template, TemplateRefusal>, CopeFailed> {
    kernel().template(cope, seam, tolerance, max_segments, cancelled)
}

/// Whether the two end cuts of one straight tube, reaching `start` and `end` mm in from its two
/// ends, may cross within its straight `length`.
#[must_use]
pub fn ends_cross(start: f64, end: f64, length: f64) -> bool {
    kernel().ends_cross(start, end, length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::tube_cope as ops;

    #[test]
    fn routed_copes_are_solidcores() {
        assert_eq!(MIN_SINE, ops::MIN_SINE);
        let plane = Plane { origin: Vec3::new(0.0, 0.0, 0.0), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0) };
        let t = terminal_at(&plane, 100.0, false).unwrap();
        assert_eq!(t, ops::terminal_at(&plane, 100.0, false).unwrap());
        assert!(terminal_ok(&t));
        let (a, b) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 100.0));
        assert_eq!(line_box(a, b, 5.0), ops::line_box(a, b, 5.0));
        // a 30 mm mate square across the end of a 20 mm tube
        let mate = Mate {
            origin: Vec3::new(-50.0, 0.0, -15.0),
            axis: Vec3::new(1.0, 0.0, 0.0),
            radius: 15.0,
            length: 100.0,
            coped_start: 0.0,
            coped_end: 0.0,
        };
        assert!(mate_ok(&mate));
        let routed = fit(&t, 10.0, &mate, &[]).unwrap();
        let direct = ops::fit(&t, 10.0, &mate, &[]).unwrap();
        assert_eq!(routed.ok(), direct.ok());
        // a refusal carries solidcore's words whole
        let words = ops::terminal_at(&plane, f64::NAN, false).unwrap_err().to_string();
        assert_eq!(terminal_at(&plane, f64::NAN, false), Err(CopeFailed(words)));
        assert_eq!(ends_cross(10.0, 10.0, 100.0), ops::ends_cross(10.0, 10.0, 100.0));
    }
}
