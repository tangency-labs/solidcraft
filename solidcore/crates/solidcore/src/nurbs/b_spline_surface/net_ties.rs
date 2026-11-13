//! Which control points of a B-spline surface's net move together ([`BSplineSurface::net_ties`]):
//! the two boundary columns that meet at the seam of a closed surface, and a boundary row that
//! collapses to one point (a pole or an apex).
//!
//! Not upstream: a control-point edit of a closed surface (a sphere's, a cylinder's or a
//! torus's exact NURBS form) moves each tied group by one amount, so the seam stays closed and
//! a pole stays a point. The kernel decides the ties from the net; the modelling layer only
//! expands its picks by them.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeMap;

use crate::error::{Result, SolidError};
use crate::geom::vector3d::Point3;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};

const OP: &str = "nurbs::b_spline_surface::net_ties::BSplineSurface::net_ties";

/// A control point by its u and v indices.
type Index = (usize, usize);

/// The groups of tied points, as they are joined: each point's group, named by its least member.
#[derive(Default)]
struct Ties(BTreeMap<Index, Index>);

impl Ties {
    fn group(&self, p: Index) -> Index {
        self.0.get(&p).copied().unwrap_or(p)
    }

    /// Joins the groups of `a` and `b` (under the lesser name, so a group's name stays its
    /// least member).
    fn tie(&mut self, a: Index, b: Index) {
        let (ga, gb) = (self.group(a), self.group(b));
        self.0.entry(a).or_insert(ga);
        self.0.entry(b).or_insert(gb);
        let (keep, gone) = if ga <= gb { (ga, gb) } else { (gb, ga) };
        if keep == gone {
            return;
        }
        for g in self.0.values_mut() {
            if *g == gone {
                *g = keep;
            }
        }
    }

    /// The groups, each in index order, in the order of their least member.
    fn groups(self) -> Vec<Vec<Index>> {
        let mut by_name: BTreeMap<Index, Vec<Index>> = BTreeMap::new();
        for (p, g) in self.0 {
            by_name.entry(g).or_default().push(p);
        }
        by_name.into_values().collect()
    }
}

impl BSplineSurface {
    /// Whether the knots in `param`'s direction are clamped: the first and the last knot each
    /// of multiplicity above the degree, so the net's first and last lines in that direction
    /// are the surface's boundary curves.
    fn is_clamped_in(&self, param: SurfParamType) -> Result<bool> {
        let (_, multiplicities) = self.knots(param, None)?;
        let degree = self.degree(param);
        let ends = |m: Option<&usize>| m.is_some_and(|&m| m > degree);
        Ok(ends(multiplicities.first()) && ends(multiplicities.last()))
    }

    /// The control points of the net that move together, each group in index order (`(u
    /// index, v index)`) and the groups in the order of their first member; no group for a
    /// point that moves alone.
    ///
    /// - **A seam:** in a clamped direction, when the net's first and last lines are one curve
    ///   (each point of one within `tol` of the same point of the other), each point is tied
    ///   to its twin: a cylinder's, a cone's or a sphere's sweep, a torus's two directions.
    /// - **A pole:** a boundary line of a clamped direction whose points all lie within `tol`
    ///   of its first is one point, and all of it is tied: a sphere's poles, a cone's apex.
    ///
    /// Groups that share a point are one group (a torus's four corners; a pole row holds its
    /// seam pair). A direction that is not clamped has no boundary line in its net, so nothing
    /// is tied across it. `tol` is a distance in model units (the body's tolerance).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a `tol` not finite or below zero, or a net shorter than its counts;
    /// those of [`Self::control_point_net`] and [`Self::knots`].
    pub fn net_ties(&self, tol: f64) -> Result<Vec<Vec<(usize, usize)>>> {
        if !(tol.is_finite() && tol >= 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a tolerance not finite or below zero",
            ));
        }
        let net = self.control_point_net()?;
        let (nu, nv) = (net.u_count, net.v_count);
        let short = || SolidError::invalid_input(OP, "a net shorter than its counts");
        let at = |(i, j): Index| -> Result<Point3> {
            i.checked_mul(nv)
                .and_then(|row| row.checked_add(j))
                .and_then(|n| net.points.get(n))
                .copied()
                .ok_or_else(short)
        };
        let same = |a: Index, b: Index| -> Result<bool> { Ok(at(a)?.distance(at(b)?) <= tol) };
        // the net's boundary lines: u index fixed (running in v) and v index fixed (in u)
        let u_line = |i: usize| (0..nv).map(move |j| (i, j));
        let v_line = |j: usize| (0..nu).map(move |i| (i, j));

        let mut ties = Ties::default();
        // (the line, its opposite) in each clamped direction of more than one line
        let mut seams: Vec<(Vec<Index>, Vec<Index>)> = Vec::new();
        let mut boundaries: Vec<Vec<Index>> = Vec::new();
        if let (true, Some(last)) = (
            self.is_clamped_in(SurfParamType::U)?,
            nu.checked_sub(1).filter(|&l| l > 0),
        ) {
            seams.push((u_line(0).collect(), u_line(last).collect()));
            boundaries.push(u_line(0).collect());
            boundaries.push(u_line(last).collect());
        }
        if let (true, Some(last)) = (
            self.is_clamped_in(SurfParamType::V)?,
            nv.checked_sub(1).filter(|&l| l > 0),
        ) {
            seams.push((v_line(0).collect(), v_line(last).collect()));
            boundaries.push(v_line(0).collect());
            boundaries.push(v_line(last).collect());
        }

        for (first, last) in &seams {
            let mut closed = true;
            for (&a, &b) in first.iter().zip(last) {
                closed = closed && same(a, b)?;
            }
            if closed {
                for (&a, &b) in first.iter().zip(last) {
                    ties.tie(a, b);
                }
            }
        }
        for line in &boundaries {
            let Some((&head, rest)) = line.split_first() else {
                continue;
            };
            let mut collapsed = !rest.is_empty();
            for &p in rest {
                collapsed = collapsed && same(head, p)?;
            }
            if collapsed {
                for &p in rest {
                    ties.tie(head, p);
                }
            }
        }
        Ok(ties.groups())
    }
}
