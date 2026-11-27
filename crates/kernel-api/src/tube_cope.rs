//! Round-tube copes ([`TubeCopes`]): a tube's straight end cut to fit the outside cylinders of
//! mating tubes, and the cut unwrapped as a 1:1 template. The values are the record's
//! (`solidcore_record::tube_cope`); the refusals are the kernel's, as data.

use solidcore_record::tube_cope::{Box3, Cope, Fit, Mate, Template, Terminal};
use solidcore_record::{Plane, Vec3};

use crate::Backend;

/// Why a mate cannot cope a tube's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CopeRefusal {
    /// The mate's axis passes this far (mm) from the endpoint.
    OffAxis {
        /// The distance from the endpoint to the mate's axis (mm).
        distance: f64,
    },
    /// The mate is smaller than the tube.
    Smaller {
        /// The mate's outside diameter (mm).
        mate_diameter: f64,
        /// The tube's outside diameter (mm).
        tube_diameter: f64,
    },
    /// The mate runs almost parallel to the tube.
    Parallel {
        /// The joint's sine.
        sine: f64,
    },
    /// The cut may reach the tube's other end or its next bend.
    PastTerminal,
    /// The cut may meet one of the mate's end caps.
    MateEnd,
    /// The joint reaches one of the mate's own copes.
    MateCope,
    /// The mate's cylinder may also meet a bend or another portion of the tube.
    MeetsOther,
}

/// Why a cope cannot be unwrapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateRefusal {
    /// The sampling bound needs more than the allowed segments.
    TooManySegments,
    /// A station left the cut's retained depth: the record does not describe this cut.
    OutsideDepth,
    /// The caller stopped it.
    Cancelled,
}

/// An end, mate, radius, seam or tolerance the kernel refuses as out of range, in its words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CopeFailed(pub String);

/// The round-tube copes of one kernel ([`crate::OpKind::TubeCope`]).
pub trait TubeCopes: Backend {
    /// The smallest joint sine a cope takes: below it the cut runs too far along the tube.
    const MIN_SINE: f64;
    /// The end of a swept path as a [`Terminal`]: from the section plane there (its normal along
    /// the path) and the straight length from it; `at_end` for the path's end.
    fn terminal_at(&self, plane: &Plane, length: f64, at_end: bool) -> Result<Terminal, CopeFailed>;
    /// Whether `t` is a usable end.
    fn terminal_ok(&self, t: &Terminal) -> bool;
    /// Whether `m` is a usable mate cylinder.
    fn mate_ok(&self, m: &Mate) -> bool;
    /// A box holding a straight swept piece from `a` to `b`, `pad` round it.
    fn line_box(&self, a: Vec3, b: Vec3, pad: f64) -> Box3;
    /// A box holding a circular bend from `a` about `center`, `pad` round it.
    fn arc_box(&self, a: Vec3, center: Vec3, pad: f64) -> Box3;
    /// Mate `m` fitted to end `t` of a tube of outside radius `radius` whose other swept pieces
    /// lie in `others`, or why it cannot cope it.
    fn fit(&self, t: &Terminal, radius: f64, m: &Mate, others: &[Box3]) -> Result<Result<Fit, CopeRefusal>, CopeFailed>;
    /// `cope` unwrapped from `seam` with at most `tolerance` mm of error in at most
    /// `max_segments` segments, or why it cannot be.
    fn template(
        &self,
        cope: &Cope<'_>,
        seam: f64,
        tolerance: f64,
        max_segments: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Result<Template, TemplateRefusal>, CopeFailed>;
    /// Whether the two end cuts of one straight tube, reaching `start` and `end` mm in from its
    /// two ends, may cross within its straight `length`.
    fn ends_cross(&self, start: f64, end: f64, length: f64) -> bool;
}
