//! How far a fillet solver's trace extends its base surfaces' domains: the extension
//! factor's share, and the reach across the edge that SolidCraft's fork lets a trace
//! have so that a blend wider than its faces is traced as far as its rails.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork
//! (tangency-labs/smlib, rev `68fb9162`) adds `m_dTraceReach`, `m_abTraceAcross`,
//! `SetTraceReach` and `SmFilletSolver::GetDomainExtension` (`SmFilletSolver.h`,
//! `SmFilletSolver.cpp`) in commit `a9277fd` (WR-609). They are used only with the
//! opt-in `Kernel::fixes.fillet_trace_reaches_rails`: with it off the reach stays `0`
//! and [`FilletSolver::domain_extension`] is the extension factor times the domain's
//! size, as upstream's `SetupOffsetExtension` computes it.
//!
//! | Rust                               | Upstream (fork)                      |
//! |------------------------------------|--------------------------------------|
//! | [`FilletSolver::domain_extension`] | `SmFilletSolver::GetDomainExtension` |
//! | [`FilletSolver::set_trace_reach`]  | `SmFilletSolver::SetTraceReach`      |
//!
//! Deviations: the fork tells which of the solver's base surfaces `pSurf` is by its
//! address. The port's callers pass copies of the base surfaces, so a surface is taken
//! for a solver's base surface when it is that object or shares its cache slot, or
//! else when the base surface has the same natural domain and gives bit-identical
//! points at the five parameter points the extension evaluates (the four ends and
//! the middle of the domain's middle lines); as in the fork, the last of the two that
//! matches wins. A failed evaluation gives the factor's extension, as the fork's does,
//! except that [`SolidError::Unported`] propagates. There is no recursion.

use super::p1::FilletSolver;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::{Point2, Vec2};
use crate::kernel::Kernel;
use crate::math::{Point3, EFF_ZERO_SQRT};
use crate::surface::surface::Surface;

/// `smos_Max` of two doubles (the first when it is larger, else the second).
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// The four points `GetDomainExtension` evaluates: the ends of the domain's middle
/// lines, `[u min, u max, v min, v max]`.
fn domain_ends(k: &Kernel, surface: &Surface, domain: &Extent2d) -> Result<Option<[Point3; 4]>> {
    let mid = domain.mid();
    let uvs = [
        Point2::new(domain.min.x, mid.y),
        Point2::new(domain.max.x, mid.y),
        Point2::new(mid.x, domain.min.y),
        Point2::new(mid.x, domain.max.y),
    ];
    let mut ends = [Point3::UNDEF; 4];
    for (end, uv) in ends.iter_mut().zip(uvs) {
        match surface.evaluate_point(k, uv) {
            Ok(p) => *end = p,
            Err(e) if e.is_unported() => return Err(e),
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(ends))
}

/// Whether `candidate` stands for the solver's base surface `base` (the fork compares
/// addresses; see the module's deviations).
fn is_same_surface(
    k: &Kernel,
    base: &Surface,
    candidate: &Surface,
    domain: &Extent2d,
    candidate_ends: &[Point3; 4],
) -> Result<bool> {
    if std::ptr::eq(base, candidate) || base.shares_cache_slot_with(candidate) {
        return Ok(true);
    }
    let same_domain = match (
        base.natural_uv_domain_with_nurb(k),
        candidate.natural_uv_domain_with_nurb(k),
    ) {
        (Ok(a), Ok(b)) => a.min == b.min && a.max == b.max,
        (Err(e), _) | (_, Err(e)) if e.is_unported() => return Err(e),
        _ => false,
    };
    if !same_domain {
        return Ok(false);
    }
    let Some(base_ends) = domain_ends(k, base, domain)? else {
        return Ok(false);
    };
    if base_ends != *candidate_ends {
        return Ok(false);
    }
    let mid = domain.mid();
    match (
        base.evaluate_point(k, mid),
        candidate.evaluate_point(k, mid),
    ) {
        (Ok(a), Ok(b)) => Ok(a == b),
        (Err(e), _) | (_, Err(e)) if e.is_unported() => Err(e),
        _ => Ok(false),
    }
}

impl FilletSolver {
    /// `SetTraceReach(dReach, crabAcross)` (fork, WR-609): how far (3D) the trace's base
    /// surface domains reach past their own at least, across the edge (`0` = by the
    /// extension factor alone), and which parameter directions of each base surface
    /// (`[surface][u, v]`) run across the edge.
    pub fn set_trace_reach(&mut self, reach: f64, across: [[bool; 2]; 2]) {
        self.trace_reach = reach;
        self.trace_across = across;
    }

    /// `GetDomainExtension(pSurf, crDomain)` (fork, WR-609): how far the trace extends
    /// a base surface's domain past `domain`, in each parameter direction.
    ///
    /// The extension factor times the domain's size (upstream), and in the directions
    /// that run across the edge ([`FilletSolver::set_trace_reach`]) at least as far as
    /// the trace's reach in 3D: the size in parameters scaled by the domain's length in
    /// 3D along that direction, through its middle. Along the edge it keeps the
    /// factor's. A direction of no length in 3D keeps the factor's. `surface` (`None`
    /// for the fork's `NULL`) need not be one of the solver's base surfaces; one that
    /// is not takes the reach in both directions.
    ///
    /// # Errors
    ///
    /// [`crate::error::SolidError::Unported`] from the evaluations; other failures give
    /// the factor's extension, as the fork's does.
    pub fn domain_extension(
        &self,
        k: &Kernel,
        surface: Option<&Surface>,
        domain: &Extent2d,
    ) -> Result<Vec2> {
        let mut size: Vec2 = self.surface_extension_factor * domain.size();
        let Some(surface) = surface else {
            return Ok(size);
        };
        // `!( m_dTraceReach > 0.0 )`: a NaN reach returns too
        let reaches = self.trace_reach > 0.0;
        if !reaches {
            return Ok(size);
        }
        let Some(ends) = domain_ends(k, surface, domain)? else {
            return Ok(size);
        };
        // which of the solver's base surfaces this is: its directions across the edge
        let mut which = None;
        for (index, slot) in self.surfaces.iter().enumerate() {
            if let Some(slot) = slot {
                if is_same_surface(k, slot.surface.base_surface(), surface, domain, &ends)? {
                    which = Some(index);
                }
            }
        }
        let across = which
            .and_then(|index| self.trace_across.get(index))
            .copied()
            .unwrap_or([true, true]);
        let [across_u, across_v] = across;
        let uv_size = domain.size();
        let [u_min, u_max, v_min, v_max] = ends;
        let d3d_u = u_min.distance(u_max);
        let d3d_v = v_min.distance(v_max);
        if across_u && d3d_u > EFF_ZERO_SQRT {
            size.x = smos_max(size.x, self.trace_reach * uv_size.x / d3d_u);
        }
        if across_v && d3d_v > EFF_ZERO_SQRT {
            size.y = smos_max(size.y, self.trace_reach * uv_size.y / d3d_v);
        }
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::BSplineSurface;

    fn plane(_k: &Kernel) -> Surface {
        Surface::from(
            BSplineSurface::create_bilinear_surface(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 4.0, 0.0),
                Point3::new(10.0, 4.0, 0.0),
            )
            .unwrap(),
        )
    }

    #[test]
    fn no_reach_is_the_factor_times_the_size() {
        let k = Kernel::default();
        let s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        let surface = plane(&k);
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let size = s.domain_extension(&k, Some(&surface), &domain).unwrap();
        let expect = 0.5 * domain.size();
        assert_eq!((size.x, size.y), (expect.x, expect.y));
        assert_eq!(s.domain_extension(&k, None, &domain).unwrap(), expect);
    }

    #[test]
    fn a_reach_grows_only_the_directions_across() {
        let k = Kernel::default();
        let mut s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        let surface = plane(&k);
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let factor = 0.5 * domain.size();
        // a surface that is none of the solver's: both directions get the reach
        s.set_trace_reach(40.0, [[false, true], [true, true]]);
        let size = s.domain_extension(&k, Some(&surface), &domain).unwrap();
        let ends = domain_ends(&k, &surface, &domain).unwrap().unwrap();
        let (du, dv) = (ends[0].distance(ends[1]), ends[2].distance(ends[3]));
        assert_eq!(size.x, 40.0 * domain.size().x / du);
        assert_eq!(size.y, 40.0 * domain.size().y / dv);
        assert!(size.x > factor.x && size.y > factor.y);
    }
}
