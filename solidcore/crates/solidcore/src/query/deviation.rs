//! How far one curve strays from another curve or from a surface: the largest distance found,
//! where it is, the sampled pairs a comb is drawn from, and, where it applies, a certified bound.
//!
//! Not upstream. `samples` stations at even steps of the first curve's natural interval, its
//! ends included; each station's point dropped onto the other curve (its whole natural
//! interval, the kernel's [`Curve::drop_point`], minimised) or onto the surface (its given
//! parameter box, [`Surface::drop_point`]). A station whose drop fails is left out. The largest
//! of the stations' distances is the deviation: a lower bound of the true largest distance, the
//! true one between stations not being looked at.
//!
//! The bound: for two B-spline curves on one parameter domain, the kernel's
//! [`crate::nurbs::crv_deviation::certified_distance`] bounds `|a(t) - b(t)|` at every `t`, so
//! the largest distance from `a` to `b` is at most it (each `a(t)` is that close to a point of
//! `b`). It is given where it applies (both curves B-splines, not rational, on one domain; a
//! rational form whose weights are all equal counts as polynomial) and `None` elsewhere; it is
//! never smaller than the true deviation, the sampled one never larger.
//!
//! Bounded: at most [`MAX_SAMPLES`] drops; no recursion.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::ZONE_TOL_3D;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;

const OP: &str = "query::deviation";

/// The fewest stations: the two ends.
pub const MIN_SAMPLES: usize = 2;
/// The most stations.
pub const MAX_SAMPLES: usize = 10_000;

/// How far a curve strays from another curve or a surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Deviation {
    /// The largest distance found at the stations.
    pub max: f64,
    /// Where on the first curve it is.
    pub at: Point3,
    /// Its foot on the other curve or the surface.
    pub to: Point3,
    /// Each station's point and its foot, in order along the first curve: a comb.
    pub comb: Vec<(Point3, Point3)>,
    /// A certified bound of the largest distance, where one applies (module notes).
    pub bound: Option<f64>,
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The stations' parameters on `c`: `samples` even steps of its natural interval, its ends
/// included.
fn stations(c: &Curve, samples: usize) -> Result<Vec<f64>> {
    if !(MIN_SAMPLES..=MAX_SAMPLES).contains(&samples) {
        return Err(invalid("stations between 2 and 10000"));
    }
    let ivl = c.natural_interval();
    let steps = (samples - 1) as f64;
    Ok((0..samples)
        .map(|i| ivl.evaluate(i as f64 / steps))
        .collect())
}

/// The deviation from the pairs `comb`, with `bound`; refused when no station measured.
fn of(comb: Vec<(Point3, Point3)>, bound: Option<f64>) -> Result<Deviation> {
    let mut worst: Option<(f64, Point3, Point3)> = None;
    for &(p, q) in &comb {
        let d = p.distance(q);
        if worst.is_none_or(|(w, _, _)| d > w) {
            worst = Some((d, p, q));
        }
    }
    let Some((max, at, to)) = worst else {
        return Err(SolidError::query_failed(
            OP,
            "no station dropped onto the other",
        ));
    };
    Ok(Deviation {
        max,
        at,
        to,
        comb,
        bound,
    })
}

/// How far curve `a` strays from curve `b`, at `samples` stations of `a` (module notes).
///
/// # Errors
///
/// `InvalidInput` for a station count outside [`MIN_SAMPLES`]..=[`MAX_SAMPLES`]; a query failure
/// when no station drops onto `b`; those of the evaluations and the drops.
pub fn curve_deviation(k: &Kernel, a: &Curve, b: &Curve, samples: usize) -> Result<Deviation> {
    let ts = stations(a, samples)?;
    let over = b.natural_interval();
    let mut comb = Vec::new();
    for t in ts {
        let p = a.evaluate_point(k, t)?;
        let d = b.drop_point(
            k,
            &over,
            p,
            None,
            ZONE_TOL_3D,
            None,
            SolverOperation::Minimize,
        )?;
        if d.success {
            comb.push((p, b.evaluate_point(k, d.parameter)?));
        }
    }
    of(comb, certified(a, b))
}

/// How far curve `c` strays from `surface` over its parameter box `domain`, at `samples`
/// stations of `c` (module notes).
///
/// # Errors
///
/// As [`curve_deviation`], the drops onto the surface's.
pub fn curve_surface_deviation(
    k: &Kernel,
    c: &Curve,
    surface: &Surface,
    domain: &Extent2d,
    samples: usize,
) -> Result<Deviation> {
    let ts = stations(c, samples)?;
    let mut comb = Vec::new();
    for t in ts {
        let p = c.evaluate_point(k, t)?;
        let d = surface.drop_point::<NoObject>(k, p, domain, &DropPointOptions::default())?;
        if d.success {
            comb.push((p, surface.evaluate_point(k, d.result_uv)?));
        }
    }
    of(comb, None)
}

/// The kernel's certified `|a(t) - b(t)|` bound, where both are non-rational B-splines on one
/// domain; `None` otherwise.
fn certified(a: &Curve, b: &Curve) -> Option<f64> {
    let (CurveKind::BSpline(a), CurveKind::BSpline(b)) = (a.kind(), b.kind()) else {
        return None;
    };
    let (mut a, mut b) = (a.clone(), b.clone());
    let mut na = a.get_or_create_gw_nurb().ok()?.clone();
    let mut nb = b.get_or_create_gw_nurb().ok()?.clone();
    // A polynomial spline built from a record carries weights of 1 (a rational form with
    // equal weights); the bound takes polynomial curves, so equal weights are dropped first.
    na.prune_rat(false);
    nb.prune_rat(false);
    crate::nurbs::crv_deviation::certified_distance(&na, &nb).ok()
}

#[cfg(test)]
mod tests;
