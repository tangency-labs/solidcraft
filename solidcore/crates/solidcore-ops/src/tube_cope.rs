//! Round-tube copes on record values: a tube's straight end cut to fit the outside cylinders of
//! mating tubes, and the cut unwrapped as a 1:1 template. Every quantity is the kernel's
//! ([`solidcore::query::tube_cope`]); this converts the record's vectors to the kernel's and back
//! and passes the kernel's refusals on.

use solidcore::query::tube_cope as k;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::{Plane, Vec3};

pub use solidcore::query::tube_cope::{Refusal, TemplateRefusal, AXIS_SNAP, MIN_SINE};
/// The cope's data, the record's ([`solidcore_record::tube_cope`]).
pub use solidcore_record::tube_cope::{Box3, Cope, Fit, Mate, Template, Terminal};

use crate::error::status;
use crate::Result;

fn kernel_terminal(t: &Terminal) -> k::Terminal {
    k::Terminal {
        origin: vec3(t.origin),
        axis: vec3(t.axis),
        x: vec3(t.x),
        y: vec3(t.y),
        length: t.length,
    }
}

fn record_terminal(t: &k::Terminal) -> Terminal {
    Terminal {
        origin: record_vec3(t.origin),
        axis: record_vec3(t.axis),
        x: record_vec3(t.x),
        y: record_vec3(t.y),
        length: t.length,
    }
}

fn kernel_mate(m: &Mate) -> k::Mate {
    k::Mate {
        origin: vec3(m.origin),
        axis: vec3(m.axis),
        radius: m.radius,
        length: m.length,
        coped_start: m.coped_start,
        coped_end: m.coped_end,
    }
}

fn record_box(b: k::Box3) -> Box3 {
    Box3 {
        min: record_vec3(b.min),
        max: record_vec3(b.max),
    }
}

fn kernel_box(b: &Box3) -> k::Box3 {
    k::Box3 {
        min: vec3(b.min),
        max: vec3(b.max),
    }
}

/// The end of a swept path as a [`Terminal`] ([`k::terminal_at`]): from the section plane there
/// (its normal along the path) and the straight length from it; `at_end` for the path's end.
///
/// # Errors
/// The kernel's refusal of a plane that is not orthonormal or a length out of range.
pub fn terminal_at(plane: &Plane, length: f64, at_end: bool) -> Result<Terminal> {
    k::terminal_at(
        vec3(plane.origin),
        vec3(plane.x),
        vec3(plane.y),
        length,
        at_end,
    )
    .map(|t| record_terminal(&t))
    .map_err(status)
}

/// Whether `t` is a usable end ([`k::terminal_ok`]).
#[must_use]
pub fn terminal_ok(t: &Terminal) -> bool {
    k::terminal_ok(&kernel_terminal(t))
}

/// Whether `m` is a usable mate cylinder ([`k::mate_ok`]).
#[must_use]
pub fn mate_ok(m: &Mate) -> bool {
    k::mate_ok(&kernel_mate(m))
}

/// A box holding a straight swept piece ([`k::line_box`]).
#[must_use]
pub fn line_box(a: Vec3, b: Vec3, pad: f64) -> Box3 {
    record_box(k::line_box(vec3(a), vec3(b), pad))
}

/// A box holding a circular bend ([`k::arc_box`]).
#[must_use]
pub fn arc_box(a: Vec3, center: Vec3, pad: f64) -> Box3 {
    record_box(k::arc_box(vec3(a), vec3(center), pad))
}

/// Fit mate `m` to end `t` of a tube of outside radius `radius` whose other swept pieces lie in
/// `others` ([`k::fit`]).
///
/// # Errors
/// The kernel's refusal of an end, mate or radius out of range.
pub fn fit(
    t: &Terminal,
    radius: f64,
    m: &Mate,
    others: &[Box3],
) -> Result<core::result::Result<Fit, Refusal>> {
    let others: Vec<k::Box3> = others.iter().map(kernel_box).collect();
    let fitted = k::fit(&kernel_terminal(t), radius, &kernel_mate(m), &others).map_err(status)?;
    Ok(fitted.map(|f| Fit {
        origin: record_vec3(f.origin),
        depth: f.depth,
    }))
}

/// `cope` unwrapped from `seam` with at most `tolerance` mm of error in at most `max_segments`
/// segments ([`k::template`]).
///
/// # Errors
/// The kernel's refusal of an end, radius, mate, seam or tolerance out of range.
pub fn template(
    cope: &Cope<'_>,
    seam: f64,
    tolerance: f64,
    max_segments: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<core::result::Result<Template, TemplateRefusal>> {
    let end = kernel_terminal(cope.end);
    let mates: Vec<(solidcore::Vec3, f64)> = cope
        .mates
        .iter()
        .map(|&(axis, r)| (vec3(axis), r))
        .collect();
    let kc = k::Cope {
        end: &end,
        radius: cope.radius,
        mates: &mates,
        depth: cope.depth,
    };
    let t = k::template(&kc, seam, tolerance, max_segments, cancelled).map_err(status)?;
    Ok(t.map(|t| Template {
        points: t.points,
        seam_direction: record_vec3(t.seam_direction),
        circumference: t.circumference,
        outer_diameter: t.outer_diameter,
    }))
}

/// Whether the two end cuts of one straight tube, reaching `start` and `end` mm in from its two
/// ends, may cross within its straight `length` ([`k::ends_cross`]).
#[must_use]
pub fn ends_cross(start: f64, end: f64, length: f64) -> bool {
    k::ends_cross(start, end, length)
}
