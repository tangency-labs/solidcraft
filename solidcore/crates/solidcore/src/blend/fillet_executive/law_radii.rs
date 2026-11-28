//! A radius law from its control radii: the B-spline law (`SmBSplineFilletLaw`) whose curve
//! has the radius as its x and its own parameter as its y, the control points at the
//! Greville abscissae, so its y runs along the edge as the parameter does.
//!
//! Not upstream: the fork's C API builds the same curve for its variable fillet
//! (`law_along`). No recursion.

use super::p1::{FilletLaw, FilletLawKind};
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::crv_nurb::greville_abscissa;
use crate::topology::edge as topo_edge;
use crate::topology::vertex as topo_vertex;
use crate::topology::{Brep, EdgeId, VertexId};
use crate::{Result, SolidError, Vec3};

const OP: &str = "blend::fillet_executive::FilletLaw::of_radii";
const OP_CHAIN: &str = "blend::fillet_executive::FilletLaw::chain_of_radii";

impl FilletLaw {
    /// The law of a clamped B-spline radius of `degree` over `knots` (from 0 to 1, `n +
    /// degree + 1` of them) with the control radii `radii` (`n` of them): its curve's control
    /// points are `(r_i, g_i, 0)`, `g_i` the Greville abscissa of control point i. `reversed`
    /// runs it from the edge's end: the knots mirrored about 1/2 and the radii taken from
    /// the last. The law's edge map is the whole curve, 0 to 1.
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] for knots and radii whose counts do not make a B-spline of
    /// `degree`; those of the Greville abscissae and the curve's construction.
    pub fn of_radii(degree: usize, knots: &[f64], radii: &[f64], reversed: bool) -> Result<Self> {
        let (nk, nr) = (knots.len(), radii.len());
        if degree == 0
            || nr <= degree
            || degree.checked_add(nr).and_then(|m| m.checked_add(1)) != Some(nk)
        {
            return Err(SolidError::invalid_input(
                OP,
                "a law has degree + 1 to n control radii and n + degree + 1 knots",
            ));
        }
        let kv: Vec<f64> = if reversed {
            knots.iter().rev().map(|&u| 1.0 - u).collect()
        } else {
            knots.to_vec()
        };
        let rs: Vec<f64> = if reversed {
            radii.iter().rev().copied().collect()
        } else {
            radii.to_vec()
        };
        let mut cps = Vec::new();
        for (i, &r) in rs.iter().enumerate() {
            cps.push(Vec3::new(r, greville_abscissa(&kv, degree, i)?, 0.0));
        }
        let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
        for &x in &kv {
            match (unique.last(), mults.last_mut()) {
                (Some(&u), Some(m)) if u == x => *m += 1,
                _ => {
                    unique.push(x);
                    mults.push(1);
                }
            }
        }
        let data = CanonicalData {
            dim: 3,
            degree,
            control_points: &cps,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        };
        let law_curve = BSplineCurve::create_canonical(&data)?;
        Ok(FilletLaw::new(FilletLawKind::BSpline {
            law_curve,
            extended_law: None,
        }))
    }

    /// One radius law along a tangent chain: the law of [`FilletLaw::of_radii`] running from
    /// the chain's end nearest `start` (0) to its other end (1), each edge of `edges` (of
    /// `b`, in any order) taking the part of it that its share of the chain gives, as its edge
    /// map. An edge's share is its parameter interval as the smooth sequence joins the chain's
    /// curves (each stretched so the joined curve's speed agrees where two edges meet), so the
    /// radius is one smooth function of the joined curve's parameter. An edge whose curve runs back along the chain takes the reversed law over
    /// the mirrored part, so the radius is one function of the position along the chain and
    /// agrees where two edges meet. Within an edge the position runs with its curve's
    /// parameter, as it does for a law along one edge. The laws come in the order of `edges`.
    ///
    /// `None` when `edges` are not one open chain (each edge's two vertices distinct, the
    /// edges joined end to end, no vertex on more than two of them, two ends) or an edge is
    /// no longer than the zone tolerance or stops at an end.
    ///
    /// # Errors
    /// Those of [`FilletLaw::of_radii`], of the edges' vertices and curves, and of the
    /// curves' lengths and derivatives.
    pub fn chain_of_radii(
        k: &Kernel,
        b: &Brep,
        edges: &[EdgeId],
        start: Vec3,
        degree: usize,
        knots: &[f64],
        radii: &[f64],
    ) -> Result<Option<Vec<Self>>> {
        let Some(path) = chain_path(b, edges, start)? else {
            return Ok(None);
        };
        // each edge's share of the chain, in the chain's order: its parameter interval as the
        // smooth sequence joins it (`CreateSmoothEdgeSequence`), stretched so the joined
        // curve's speed agrees where two edges meet, so the law is one function of the
        // joined curve's parameter and as smooth along it as its own curve
        let mut lengths = Vec::new();
        let (mut stretch, mut exit_speed) = (1.0, None);
        for &(i, reversed) in &path {
            let Some(&e) = edges.get(i) else {
                return Ok(None);
            };
            let curve =
                b.edge(e)?.curve.as_ref().ok_or_else(|| {
                    SolidError::invalid_input(OP_CHAIN, "an edge without a curve")
                })?;
            let interval = topo_edge::interval(b, e)?;
            let length = curve.length(k, &interval, EFF_ZERO)?;
            if !(length.is_finite() && length > k.tolerances.zone_tol_3d) {
                return Ok(None);
            }
            let speed = |t: f64| -> Result<f64> {
                Ok(curve
                    .evaluate(k, t, 1, false, true)?
                    .get(1)
                    .copied()
                    .map_or(0.0, Vec3::length))
            };
            let (lo, hi) = (speed(interval.min())?, speed(interval.max())?);
            let (entry, exit) = if reversed { (hi, lo) } else { (lo, hi) };
            if !(entry.is_finite() && exit.is_finite() && entry > EFF_ZERO && exit > EFF_ZERO) {
                return Ok(None);
            }
            if let Some(before) = exit_speed {
                stretch *= entry / before;
            }
            exit_speed = Some(exit);
            let share = interval.length() * stretch;
            if !(share.is_finite() && share > 0.0) {
                return Ok(None);
            }
            lengths.push(share);
        }
        let total: f64 = lengths.iter().sum();
        if !(total.is_finite() && total > 0.0) {
            return Err(SolidError::invalid_input(
                OP_CHAIN,
                "chain parameter total is not finite and positive",
            ));
        }
        let mut laws: Vec<Option<Self>> = vec![None; edges.len()];
        let mut run = 0.0;
        for (n, (&(i, reversed), &length)) in path.iter().zip(&lengths).enumerate() {
            let from = run / total;
            run += length;
            // the chain's far end is 1 exactly
            let to = if n + 1 == path.len() {
                1.0
            } else {
                run / total
            };
            let mut law = Self::of_radii(degree, knots, radii, reversed)?;
            law.edge_map = chain_law_map(from, to, reversed)?;
            if let Some(slot) = laws.get_mut(i) {
                *slot = Some(law);
            }
        }
        Ok(laws.into_iter().collect())
    }
}

/// Normalization and mirroring must both preserve a positive parameter width.
fn chain_law_map(from: f64, to: f64, reversed: bool) -> Result<Extent1d> {
    let (lo, hi) = if reversed {
        (1.0 - to, 1.0 - from)
    } else {
        (from, to)
    };
    if !(lo.is_finite() && hi.is_finite() && hi > lo) {
        return Err(SolidError::invalid_input(
            OP_CHAIN,
            "tangent chain has an unrepresentable radius-law parameter interval",
        ));
    }
    Ok(Extent1d::from_min_max(lo, hi))
}

/// The edges of `edges` (of `b`) as one open chain from its end nearest `start`: each edge's
/// index in `edges` and whether its curve runs back along the chain (from its end vertex).
/// `None` for edges that are not one open chain.
fn chain_path(b: &Brep, edges: &[EdgeId], start: Vec3) -> Result<Option<Vec<(usize, bool)>>> {
    let mut ends = Vec::new();
    for (n, &e) in edges.iter().enumerate() {
        if edges.get(..n).is_some_and(|before| before.contains(&e)) {
            return Ok(None);
        }
        let (Some(s), Some(t)) = topo_edge::start_end_vertices(b, e)? else {
            return Ok(None);
        };
        if s == t {
            return Ok(None);
        }
        ends.push([s, t]);
    }
    // each vertex with the number of the edges' ends on it
    let mut counts: Vec<(VertexId, usize)> = Vec::new();
    for &v in ends.iter().flatten() {
        match counts.iter_mut().find(|(w, _)| *w == v) {
            Some((_, c)) => *c += 1,
            None => counts.push((v, 1)),
        }
    }
    if counts.iter().any(|&(_, c)| c > 2) {
        return Ok(None);
    }
    let open: Vec<VertexId> = counts
        .iter()
        .filter(|&&(_, c)| c == 1)
        .map(|&(v, _)| v)
        .collect();
    let [a, z] = open[..] else {
        return Ok(None);
    };
    let near = |v: VertexId| -> Result<f64> { Ok(topo_vertex::point(b, v)?.distance(start)) };
    let mut at = if near(z)? < near(a)? { z } else { a };
    let mut used = vec![false; edges.len()];
    let mut path = Vec::new();
    while path.len() < edges.len() {
        let Some((i, &[s, t])) = ends
            .iter()
            .enumerate()
            .find(|&(i, [s, t])| !used.get(i).copied().unwrap_or(true) && (*s == at || *t == at))
        else {
            return Ok(None);
        };
        let reversed = t == at;
        at = if reversed { s } else { t };
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        path.push((i, reversed));
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent1d::Extent1d;

    #[test]
    fn chain_radius_maps_reject_collapsed_normalization() {
        // 56 shares 1, 1/2, ... are individually positive, but their running sum
        // rounds to the total before the last two shares in binary64.
        let shares: Vec<f64> = (0..56).map(|i| 2.0_f64.powi(-i)).collect();
        let total: f64 = shares.iter().sum();
        let mut run = 0.0;
        let mut refused = 0;
        for share in shares {
            let from = run / total;
            run += share;
            if chain_law_map(from, run / total, false).is_err() {
                refused += 1;
            }
        }
        assert!(refused > 0);
        assert!(chain_law_map(0.0, f64::EPSILON / 4.0, true).is_err());
        for reversed in [false, true] {
            let map = chain_law_map(0.25, 0.75, reversed).unwrap();
            assert_eq!(map.length(), 0.5);
        }
    }

    /// A straight law's radius runs from its first radius to its last, reversed from the
    /// last; a bent one has its corner at the inner knot, mirrored when reversed.
    #[test]
    fn a_law_of_radii_runs_along_its_curve() {
        let law = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0], false).unwrap();
        let c = law.law_curve().unwrap();
        let (lo, hi) = (
            c.evaluate(0.0, 0, false, false).unwrap(),
            c.evaluate(1.0, 0, false, false).unwrap(),
        );
        assert_eq!((lo[0].x, hi[0].x), (1.0, 2.0));
        assert_eq!((lo[0].y, hi[0].y), (0.0, 1.0));
        let back = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0], true).unwrap();
        assert_eq!(
            back.law_curve()
                .unwrap()
                .evaluate(0.0, 0, false, false)
                .unwrap()[0]
                .x,
            2.0
        );

        let edge = Extent1d::from_raw(0.0, 10.0);
        let bent =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.25, 1.0, 1.0], &[1.0, 2.0, 1.0], false).unwrap();
        assert_eq!(bent.corner_parameters(&edge, false).unwrap(), vec![2.5]);
        let bent =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.25, 1.0, 1.0], &[1.0, 2.0, 1.0], true).unwrap();
        assert_eq!(bent.corner_parameters(&edge, false).unwrap(), vec![7.5]);
        let straight =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 1.5, 2.0], false).unwrap();
        assert!(straight.corner_parameters(&edge, false).unwrap().is_empty());
        assert!(FilletLaw::of_radii(1, &[0.0, 0.0, 1.0], &[1.0, 2.0], false).is_err());
    }
}
