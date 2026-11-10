//! Complete finite maxima for identical native noncircular open ellipse arcs.
//!
//! Not upstream. A strict edge maximum has a zero first derivative and negative
//! second derivative. Interval exclusion covers the entire edge; strict endpoint
//! signs and a negative derivative enclose exactly one stationary maximum.
//! Deviations: a selected structural class replaces the incomplete search; at most
//! 4096 cells, depth 48 and 64 interval-Newton contractions. Endpoint/multiple-root ambiguity
//! refuses. Closed seams and distinct ellipse pairs remain outside this class.

use super::projected_pair::{
    bounded_domain, dot, norm_squared, project, square, sub, Candidate, Cell, Definition, Piece,
    Query,
};
use super::{Curve, CurveKind};
use crate::blend::fillet_section::certificate::curve_derivs_over;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{SolutionArray, SolutionObject};
use crate::kernel::Kernel;
use crate::math::Vec3;

const OP: &str = "curve::curve::projected_pair_ellipse";
const MAX_CELLS: usize = 4096;
const MAX_DEPTH: usize = 48;
const NEWTON_STEPS: usize = 64;

fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// Select identical native noncircular open ellipse arcs before proof.
/// Projection degeneracy, traversal and root ambiguity remain typed refusals.
pub(super) fn supports(curves: [&Curve; 2], intervals: [Extent1d; 2]) -> bool {
    let [a, b] = curves;
    let (CurveKind::Ellipse(ea), CurveKind::Ellipse(eb)) = (a.kind(), b.kind()) else {
        return false;
    };
    let (Some(na), Some(nb)) = (ea.base().gw_nurb(), eb.base().gw_nurb()) else {
        return false;
    };
    let span = ea.step_interval().length();
    span.is_finite()
        && span > 0.0
        && span < 360.0
        && ea.step_interval() == eb.step_interval()
        && ea.x_radius() != ea.y_radius()
        && ea.x_radius() == eb.x_radius()
        && ea.y_radius() == eb.y_radius()
        && ea.position().origin() == eb.position().origin()
        && ea.position().x_axis() == eb.position().x_axis()
        && ea.position().y_axis() == eb.position().y_axis()
        && ea.inside_out() == eb.inside_out()
        && ea.base().dim() == eb.base().dim()
        && na.degree() == 2
        && nb.degree() == 2
        && na.knots() == nb.knots()
        && na.cpts() == nb.cpts()
        && curves.into_iter().zip(intervals).all(|(curve, interval)| {
            bounded_domain(curve, interval).is_ok_and(|domain| domain.min() < domain.max())
        })
}

fn finite(x: Ivl) -> bool {
    x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi
}

/// Establish the regular noncircular projected conic needed by the finite
/// interior-candidate theorem. `definition` must come from `Definition::of`
/// for this same immutable curve, which already proves its analytic exact class.
fn projected_frame(curve: &Curve, definition: &Definition<'_>, normal: Vec3) -> Result<V3<Ivl>> {
    let (CurveKind::Ellipse(ellipse), Definition::Nurbs(net)) = (curve.kind(), definition) else {
        return Err(refused(
            "finite ellipse enumeration requires a native ellipse",
        ));
    };
    if net.p != 2 || net.n > MAX_CELLS {
        return Err(refused(
            "finite ellipse enumeration requires bounded quadratic native pieces",
        ));
    }
    let domain = curve.natural_interval();
    // A conic identity on separate pieces does not prove they meet. For
    // quadratic positive-weight NURBS, internal multiplicity <=2 guarantees
    // C0 continuity; a full-multiplicity internal break must refuse.
    if net.u.windows(3).any(|run| match run {
        [a, _, c] => a == c && domain.min() < *a && *a < domain.max(),
        _ => false,
    }) {
        return Err(refused("ellipse form has a discontinuous internal knot"));
    }
    let center = ellipse.position().origin();
    let x = ellipse.position().x_axis();
    let y = if ellipse.inside_out() {
        -ellipse.position().y_axis()
    } else {
        ellipse.position().y_axis()
    };
    let sweep = crate::surface::implicit::conic_turn(
        ellipse.base(),
        center,
        x,
        y,
        ellipse.x_radius(),
        ellipse.y_radius(),
    )?
    .ok_or_else(|| refused("ellipse form has no regular nonreversing traversal certificate"))?;
    if !(sweep > 0.0 && sweep < 360.0) {
        return Err(refused(
            "ellipse form has no regular open-arc coverage certificate",
        ));
    }
    let scaled = |axis: Vec3, radius: f64| {
        let r = Ivl::from(radius);
        V3 {
            x: Ivl::from(axis.x) * r,
            y: Ivl::from(axis.y) * r,
            z: Ivl::from(axis.z) * r,
        }
    };
    let n = V3::from_vec3(normal);
    let a = project(scaled(x, ellipse.x_radius()), n);
    let b = project(scaled(y, ellipse.y_radius()), n);
    let aa = dot(a, a);
    let ab = dot(a, b);
    let bb = dot(b, b);
    let determinant = aa * bb - ab * ab;
    let difference = aa - bb;
    let discriminant = difference * difference + Ivl::from(4.0) * ab * ab;
    if !(finite(determinant)
        && finite(discriminant)
        && determinant.lo > 0.0
        && discriminant.lo > 0.0)
    {
        return Err(refused(
            "projected ellipse is degenerate, circular or unresolved",
        ));
    }
    Ok(V3::from_vec3(center))
}

/// One unique strict stationary maximum, in the original parameter coordinates.
/// This parameter enclosure is geometric uncertainty, never point rounding.
#[derive(Clone, Copy, Debug)]
struct EdgeMaximum {
    parameter: Ivl,
    #[cfg(test)]
    original_bracket: Ivl,
}

/// Half the first and second derivatives of the squared projected edge distance.
fn edge_jet(
    definition: &Definition<'_>,
    piece: Piece,
    fixed: V3<Ivl>,
    normal: Vec3,
    parameter: Ivl,
) -> Result<(Ivl, Ivl)> {
    let Definition::Nurbs(net) = definition else {
        return Err(refused(
            "ellipse edge needs its certified native rational definition",
        ));
    };
    let jet = curve_derivs_over(net, parameter, 2, piece.knot)?;
    let point = *jet
        .first()
        .ok_or_else(|| refused("missing edge point enclosure"))?;
    let first = *jet
        .get(1)
        .ok_or_else(|| refused("missing edge tangent enclosure"))?;
    let second = *jet
        .get(2)
        .ok_or_else(|| refused("missing edge curvature enclosure"))?;
    let n = V3::from_vec3(normal);
    let delta = project(sub(point, fixed), n);
    let first = project(first, n);
    let second = project(second, n);
    Ok((dot(delta, first), dot(first, first) + dot(delta, second)))
}

/// Objective enclosure on a root bracket. Its geometric remainder stays in the
/// bound: only a separately evaluated point interval may supply rounding slack.
fn edge_value_over(
    definition: &Definition<'_>,
    piece: Piece,
    fixed: V3<Ivl>,
    normal: Vec3,
    range: Ivl,
) -> Result<Ivl> {
    let midpoint = range.mid();
    let (point, _) = definition.jet(Ivl::from(midpoint), piece.knot)?;
    let projected_delta = project(sub(point, fixed), V3::from_vec3(normal));
    let at_midpoint = norm_squared(projected_delta);
    let (first, _) = edge_jet(definition, piece, fixed, normal, Ivl::from(midpoint))?;
    let (_, second) = edge_jet(definition, piece, fixed, normal, range)?;
    let delta = range - Ivl::from(midpoint);
    let squared_delta = Ivl::checked(delta.mig() * delta.mig(), delta.mag() * delta.mag());
    let linear = Ivl::from(2.0) * first * delta;
    let quadratic = second * squared_delta;
    let original = at_midpoint + linear + quadratic;
    // The same exact Taylor terms, still outward rounded, can be grouped
    // before the final large sum. This avoids rounding the full metric again
    // merely to add a tiny geometric remainder; no remainder is discarded and
    // the feasible point's independently computed interval is never enlarged.
    let mut components = [
        square(projected_delta.x),
        square(projected_delta.y),
        square(projected_delta.z),
    ];
    components.sort_by(|a, b| a.mag().total_cmp(&b.mag()));
    let [small, middle, large] = components;
    let grouped = (((small + linear) + quadratic) + middle) + large;
    let squared = Ivl::new(original.lo.max(grouped.lo), original.hi.min(grouped.hi));
    if !finite(squared) {
        return Err(refused(
            "root Taylor enclosures have no finite intersection",
        ));
    }
    let value = squared.sqrt();
    if !finite(value) {
        return Err(refused("root objective has no finite Taylor enclosure"));
    }
    Ok(value)
}

/// Contract a bracket whose strict sign change already proves existence.
/// Every interval Newton intersection retains its unique root; a contraction
/// stopping at arithmetic resolution keeps the remaining geometric enclosure.
fn contract(mut range: Ivl, evaluate: &impl Fn(Ivl) -> Result<(Ivl, Ivl)>) -> Result<Ivl> {
    for _ in 0..NEWTON_STEPS {
        let midpoint = range.mid();
        let (value, _) = evaluate(Ivl::from(midpoint))?;
        let (_, derivative) = evaluate(range)?;
        if !(finite(value) && finite(derivative) && derivative.hi < 0.0) {
            return Err(refused(
                "stationary maximum lost its strict derivative certificate",
            ));
        }
        let newton = Ivl::from(midpoint) - value / derivative;
        let next = Ivl::new(range.lo.max(newton.lo), range.hi.min(newton.hi));
        if !finite(next) {
            return Err(refused("interval Newton lost a previously certified root"));
        }
        if next == range {
            return Ok(range);
        }
        range = next;
    }
    Ok(range)
}

/// Isolate every strict interior edge maximum, or refuse an unresolved region.
/// Endpoint roots belong to the corner construction but are not silently dropped:
/// until an exact endpoint identity can be proved they cause explicit refusal.
#[cfg(test)]
fn isolate(
    kernel: &Kernel,
    initial: Ivl,
    evaluate: impl Fn(Ivl) -> Result<(Ivl, Ivl)>,
) -> Result<Vec<EdgeMaximum>> {
    isolate_with_budget(kernel, initial, evaluate, &mut 0)
}

fn isolate_with_budget(
    kernel: &Kernel,
    initial: Ivl,
    evaluate: impl Fn(Ivl) -> Result<(Ivl, Ivl)>,
    visited: &mut usize,
) -> Result<Vec<EdgeMaximum>> {
    if !(finite(initial) && initial.lo < initial.hi) {
        return Err(refused(
            "edge parameter domain is not finite and increasing",
        ));
    }
    let mut todo = vec![(initial, 0_usize)];
    let mut maxima = Vec::new();
    while let Some((range, depth)) = todo.pop() {
        kernel.check_cancelled(OP)?;
        *visited += 1;
        if *visited > MAX_CELLS {
            return Err(refused("edge stationary-root cell budget exhausted"));
        }
        let (value, derivative) = evaluate(range)?;
        if !(finite(value) && finite(derivative)) {
            return Err(refused("edge stationary function has no finite enclosure"));
        }
        // Nonzero first derivative excludes stationarity. Positive second
        // derivative excludes a local maximum even if a minimum root is present.
        if value.lo > 0.0 || value.hi < 0.0 || derivative.lo > 0.0 {
            continue;
        }
        if derivative.hi < 0.0 {
            let (left, _) = evaluate(Ivl::from(range.lo))?;
            let (right, _) = evaluate(Ivl::from(range.hi))?;
            if !(finite(left) && finite(right)) {
                return Err(refused("edge endpoint derivative has no finite enclosure"));
            }
            if left.hi < 0.0 || right.lo > 0.0 {
                continue;
            }
            if left.lo > 0.0 && right.hi < 0.0 {
                maxima.push(EdgeMaximum {
                    parameter: contract(range, &evaluate)?,
                    #[cfg(test)]
                    original_bracket: range,
                });
                continue;
            }
        }
        let mid = range.mid();
        if depth >= MAX_DEPTH || !(range.lo < mid && mid < range.hi) {
            return Err(refused(
                "edge stationary root or endpoint identity is unresolved",
            ));
        }
        todo.push((Ivl::new(mid, range.hi), depth + 1));
        todo.push((Ivl::new(range.lo, mid), depth + 1));
    }
    Ok(maxima)
}

/// A feasible representative and the separate enclosure of the actual maximum.
struct MaximumCandidate {
    point: Candidate,
    objective: Ivl,
}

#[derive(Clone, Copy)]
struct RootPiece {
    piece: Piece,
    root: EdgeMaximum,
}

fn roots_over(
    query: &Query<'_>,
    definition: &Definition<'_>,
    domain: Extent1d,
    fixed: V3<Ivl>,
    visited: &mut usize,
) -> Result<Vec<RootPiece>> {
    let mut roots = Vec::new();
    for piece in definition.pieces(domain)? {
        if piece.range.lo == piece.range.hi {
            continue;
        }
        for root in isolate_with_budget(
            query.kernel,
            piece.range,
            |t| edge_jet(definition, piece, fixed, query.normal, t),
            visited,
        )? {
            roots.push(RootPiece { piece, root });
        }
    }
    Ok(roots)
}

fn candidate_over(query: &Query<'_>, a: Piece, b: Piece) -> Result<MaximumCandidate> {
    let mut point = query.candidate(a.range.mid(), b.range.mid())?;
    for s in [a.range.lo, a.range.mid(), a.range.hi] {
        for t in [b.range.lo, b.range.mid(), b.range.hi] {
            let candidate = query.candidate(s, t)?;
            if candidate.upper > point.upper {
                point = candidate;
            }
        }
    }
    let mut objective = if a.range.lo == a.range.hi && b.range.lo == b.range.hi {
        Ivl::new(point.lower, point.upper)
    } else {
        let squared = query.bound(Cell { a, b, depth: 0 })?;
        if !finite(squared) {
            return Err(refused("root objective has no finite squared enclosure"));
        }
        squared.sqrt()
    };
    // Root isolation width is geometric. A second-order enclosure keeps its
    // remaining contribution explicit, rather than calling it point rounding.
    let [definition_a, definition_b] = &query.definitions;
    for (fixed, moving, fixed_definition, moving_definition) in [
        (a, b, definition_a, definition_b),
        (b, a, definition_b, definition_a),
    ] {
        if fixed.range.lo == fixed.range.hi && moving.range.lo < moving.range.hi {
            let (value, _) = fixed_definition.jet(fixed.range, fixed.knot)?;
            let taylor =
                edge_value_over(moving_definition, moving, value, query.normal, moving.range)?;
            objective = Ivl::new(objective.lo.max(taylor.lo), objective.hi.min(taylor.hi));
        }
    }
    if !finite(objective) {
        return Err(refused("finite maximum has no objective enclosure"));
    }
    Ok(MaximumCandidate { point, objective })
}

fn inward_derivative(
    query: &Query<'_>,
    a: Piece,
    b: Piece,
    first: bool,
    lower: bool,
) -> Result<Ivl> {
    let [da, db] = &query.definitions;
    let (pa, va) = da.jet(a.range, a.knot)?;
    let (pb, vb) = db.jet(b.range, b.knot)?;
    let n = V3::from_vec3(query.normal);
    let delta = project(sub(pa, pb), n);
    let direction = if first {
        project(va, n)
    } else {
        project(vb, n)
    };
    let sign = if first == lower { 1.0 } else { -1.0 };
    Ok(dot(delta, direction) * Ivl::from(sign))
}

fn strict_inward(value: Ivl) -> Result<bool> {
    if finite(value) && value.hi < 0.0 {
        return Ok(true);
    }
    if finite(value) && value.lo > 0.0 {
        return Ok(false);
    }
    Err(refused(
        "inward derivative does not isolate a constrained maximum",
    ))
}

/// Complete finite local maxima for identical, regular noncircular projected
/// ellipses: strict corners, strict edge roots and opposite major-axis points.
/// Every unknown classification refuses; retention is a separate construction.
fn finite_candidates(query: &Query<'_>, domains: [Extent1d; 2]) -> Result<Vec<MaximumCandidate>> {
    let [ca, cb] = query.curves;
    let (CurveKind::Ellipse(ea), CurveKind::Ellipse(eb)) = (ca.kind(), cb.kind()) else {
        return Err(refused(
            "finite ellipse construction needs two native ellipses",
        ));
    };
    let [da, db] = &query.definitions;
    let (Definition::Nurbs(na), Definition::Nurbs(nb)) = (da, db) else {
        return Err(refused(
            "finite ellipse construction needs certified native forms",
        ));
    };
    if na.p != nb.p
        || na.n != nb.n
        || na.u != nb.u
        || na.pw != nb.pw
        || ea.position().origin() != eb.position().origin()
        || ea.position().x_axis() != eb.position().x_axis()
        || ea.position().y_axis() != eb.position().y_axis()
        || ea.x_radius() != eb.x_radius()
        || ea.y_radius() != eb.y_radius()
        || ea.inside_out() != eb.inside_out()
    {
        return Err(refused(
            "finite ellipse construction requires identical definitions",
        ));
    }
    let center = projected_frame(ca, da, query.normal)?;
    let [ia, ib] = domains;
    if !(ia.min() < ia.max() && ib.min() < ib.max()) {
        return Err(refused(
            "finite ellipse construction needs positive-length domains",
        ));
    }
    let fixed_piece = |t| Piece {
        range: Ivl::from(t),
        knot: None,
    };
    let mut maxima = Vec::new();
    // Equal native parameters on identical curves have zero separation and
    // cannot maximize a nondegenerate pair of positive-length intervals.
    for (s, lower_a) in [(ia.min(), true), (ia.max(), false)] {
        for (t, lower_b) in [(ib.min(), true), (ib.max(), false)] {
            if s == t {
                continue;
            }
            let a = fixed_piece(s);
            let b = fixed_piece(t);
            let ga = inward_derivative(query, a, b, true, lower_a)?;
            let gb = inward_derivative(query, a, b, false, lower_b)?;
            if ga.lo > 0.0 || gb.lo > 0.0 {
                continue;
            }
            if strict_inward(ga)? && strict_inward(gb)? {
                maxima.push(candidate_over(query, a, b)?);
            }
        }
    }
    let mut visited = 0;
    for (first, fixed_t, lower) in [
        (true, ia.min(), true),
        (true, ia.max(), false),
        (false, ib.min(), true),
        (false, ib.max(), false),
    ] {
        let (fixed_definition, moving_definition, moving_domain) =
            if first { (da, db, ib) } else { (db, da, ia) };
        let (fixed, _) = fixed_definition.jet(Ivl::from(fixed_t), None)?;
        for root in roots_over(query, moving_definition, moving_domain, fixed, &mut visited)? {
            let moving = Piece {
                range: root.root.parameter,
                knot: root.piece.knot,
            };
            let fixed = fixed_piece(fixed_t);
            let (a, b) = if first {
                (fixed, moving)
            } else {
                (moving, fixed)
            };
            if strict_inward(inward_derivative(query, a, b, first, lower)?)? {
                maxima.push(candidate_over(query, a, b)?);
            }
        }
    }
    // For an ellipse with distinct Gram eigenvalues, center-distance maxima
    // are its two major-axis points. Distinct opposite ones are precisely the
    // interior pair maxima; minor-axis antipodes are pair-objective saddles.
    let a_roots = roots_over(query, da, ia, center, &mut visited)?;
    let b_roots = roots_over(query, db, ib, center, &mut visited)?;
    let normal = V3::from_vec3(query.normal);
    for a in a_roots {
        for b in &b_roots {
            query.kernel.check_cancelled(OP)?;
            visited += 1;
            if visited > MAX_CELLS {
                return Err(refused("finite ellipse root-pair budget exhausted"));
            }
            let pa = Piece {
                range: a.root.parameter,
                knot: a.piece.knot,
            };
            let pb = Piece {
                range: b.root.parameter,
                knot: b.piece.knot,
            };
            let (va, _) = da.jet(pa.range, pa.knot)?;
            let (vb, _) = db.jet(pb.range, pb.knot)?;
            let alignment = dot(
                project(sub(va, center), normal),
                project(sub(vb, center), normal),
            );
            if finite(alignment) && alignment.lo > 0.0 {
                continue;
            }
            if !(finite(alignment) && alignment.hi < 0.0) {
                return Err(refused(
                    "major-axis roots have unresolved opposite correspondence",
                ));
            }
            maxima.push(candidate_over(query, pa, pb)?);
            if maxima.len() > MAX_CELLS {
                return Err(refused("finite ellipse candidate budget exhausted"));
            }
        }
    }
    if maxima.is_empty() {
        return Err(refused(
            "nonempty ellipse domain has no certified finite maximum",
        ));
    }
    Ok(maxima)
}

/// Construct the complete finite maximum set before applying caller retention.
/// A root representative must enclose its root objective using only its own
/// point arithmetic interval; unresolved geometry is never retention slack.
pub(super) fn solve<O: SolutionObject>(
    kernel: &Kernel,
    curves: [&Curve; 2],
    intervals: [Extent1d; 2],
    normal: Vec3,
    tolerance: f64,
    handles: [Option<O>; 2],
    target: Option<f64>,
) -> Result<SolutionArray<O>> {
    kernel.check_cancelled(OP)?;
    if !(tolerance.is_finite()
        && tolerance >= 0.0
        && normal.x.is_finite()
        && normal.y.is_finite()
        && normal.z.is_finite())
        || target.is_some_and(|value| !value.is_finite())
    {
        return Err(refused(
            "finite ellipse query needs finite retention and projection",
        ));
    }
    let [a, b] = curves;
    let [ia, ib] = intervals;
    let domains = [bounded_domain(a, ia)?, bounded_domain(b, ib)?];
    let [da, db] = domains;
    let query = Query {
        kernel,
        curves,
        normal,
        definitions: [Definition::of(a, da)?, Definition::of(b, db)?],
    };
    let maxima = finite_candidates(&query, domains)?;
    let lower = maxima
        .iter()
        .map(|maximum| maximum.point.lower)
        .fold(target.map_or(0.0, f64::abs), f64::max);
    let exclude = (Ivl::from(lower) - Ivl::from(tolerance)).lo;
    let mut retained = Vec::new();
    for maximum in maxima {
        // A certified root-objective bound can exclude an entire local maximum
        // from the retention band before constructing its rounded representative.
        // It never permits a coarse representative to be returned.
        if maximum.objective.hi < exclude {
            continue;
        }
        let candidate = maximum.point;
        if maximum.objective.hi > candidate.upper {
            return Err(refused(
                "isolated root objective exceeds its representative enclosure",
            ));
        }
        if let Some(target) = target {
            let cutoff = Ivl::from(target.abs()) - Ivl::from(tolerance);
            if candidate.upper < cutoff.lo {
                continue;
            }
            if candidate.lower < cutoff.hi {
                return Err(refused(
                    "ellipse maximum straddles the external incumbent cutoff",
                ));
            }
        }
        retained.push(candidate);
    }
    // Each retained point encloses its root objective above. The shared
    // accumulator verifies the returned set covers every retained point bound.
    super::projected_pair_all::accumulate(&query, retained, tolerance, handles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::ellipse::{Ellipse, EllipseOptions};

    #[test]
    fn edge_isolation_proves_unique_maximum_and_excludes_minimum() {
        let roots = isolate(&Kernel::default(), Ivl::new(-2.0, 2.0), |x| {
            Ok((Ivl::from(0.3) - x, Ivl::from(-1.0)))
        })
        .unwrap();
        assert_eq!(roots.len(), 1);
        assert!(roots[0].parameter.lo <= 0.3 && roots[0].parameter.hi >= 0.3);
        assert!(roots[0].parameter.hi - roots[0].parameter.lo < 1e-14);
        assert_eq!(roots[0].original_bracket, Ivl::new(-2.0, 2.0));
        assert!(isolate(&Kernel::default(), Ivl::new(-2.0, 2.0), |x| {
            Ok((x, Ivl::from(1.0)))
        })
        .unwrap()
        .is_empty());
    }

    #[test]
    fn edge_isolation_refuses_unresolved_endpoint_and_continuum() {
        assert!(isolate(&Kernel::default(), Ivl::new(0.0, 1.0), |x| {
            Ok((Ivl::from(0.0) - x, Ivl::from(-1.0)))
        })
        .is_err());
        assert!(isolate(&Kernel::default(), Ivl::new(0.0, 1.0), |_| {
            Ok((Ivl::from(0.0), Ivl::from(0.0)))
        })
        .is_err());
    }

    #[test]
    fn edge_subdivision_covers_two_maxima_and_refuses_a_multiple_root() {
        // g=x-x^3 is the derivative of x^2/2-x^4/4: strict maxima
        // at -1 and +1, and a minimum at zero. Asymmetric limits keep
        // those roots off artificial bisection boundaries.
        let roots = isolate(&Kernel::default(), Ivl::new(-1.8, 1.9), |x| {
            Ok((x - x * x * x, Ivl::from(1.0) - Ivl::from(3.0) * x * x))
        })
        .unwrap();
        assert_eq!(roots.len(), 2);
        for (root, expected) in roots.iter().zip([-1.0, 1.0]) {
            assert!(root.parameter.lo <= expected && expected <= root.parameter.hi);
            assert!(root.parameter.hi - root.parameter.lo < 1e-12);
        }
        // The quartic's flat maximum has a repeated stationary root. A
        // sign-changing g alone cannot replace the required uniqueness proof.
        assert!(isolate(&Kernel::default(), Ivl::new(-0.7, 1.1), |x| {
            Ok((Ivl::from(0.0) - x * x * x, Ivl::from(-3.0) * x * x))
        })
        .is_err());
    }

    #[test]
    fn projected_conic_frame_and_closed_seam_boundaries_are_explicit() {
        let kernel = Kernel::default();
        let make = |rx, ry, end, inside_out| {
            Curve::from(
                Ellipse::new(
                    &kernel,
                    Vec3::ZERO,
                    Vec3::X,
                    Vec3::Y,
                    Extent1d::from_min_max(-90.0, end),
                    rx,
                    ry,
                    &EllipseOptions {
                        inside_out,
                        ..EllipseOptions::default()
                    },
                )
                .unwrap(),
            )
        };
        let circle = make(2.0, 2.0, 100.0, false);
        assert!(!supports(
            [&circle, &circle],
            [circle.natural_interval(); 2]
        ));
        let circle_definition = Definition::of(&circle, circle.natural_interval()).unwrap();
        assert!(projected_frame(&circle, &circle_definition, Vec3::Z).is_err());
        let reversed = make(4.0, 2.0, 100.0, true);
        let reversed_definition = Definition::of(&reversed, reversed.natural_interval()).unwrap();
        projected_frame(&reversed, &reversed_definition, Vec3::new(0.0, 0.0, 2.0)).unwrap();
        let closed = make(4.0, 2.0, 270.0, false);
        assert!(!supports(
            [&closed, &closed],
            [closed.natural_interval(); 2]
        ));
        let query = Query {
            kernel: &kernel,
            curves: [&closed, &closed],
            definitions: [
                Definition::of(&closed, closed.natural_interval()).unwrap(),
                Definition::of(&closed, closed.natural_interval()).unwrap(),
            ],
            normal: Vec3::Z,
        };
        // Closed seam aliases require an endpoint construction; the current
        // finite strict-classifier must refuse instead of inventing an All set.
        assert!(finite_candidates(&query, [closed.natural_interval(); 2]).is_err());
    }

    fn original_ellipse(kernel: &Kernel) -> Curve {
        Curve::from(
            Ellipse::new(
                kernel,
                Vec3::new(1.9892188469923955, -3.246996295465655, -3.4858186076942532),
                Vec3::X,
                Vec3::Y,
                Extent1d::from_min_max(-90.0, 101.59684508222117),
                4.723327346620286,
                2.505285780586296,
                &EllipseOptions::default(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn public_original_ellipse_all_constructs_complete_pairs_and_propagates_refusal() {
        use crate::curve::curve::p9::CurveSolveOptions;
        use crate::geom::global_solver::{SolutionRequested, SolverOperation};
        use crate::geom::solution_array::OperandObject;
        let requested = Extent1d::from_min_max(-12.067776972579878, 65.71806952381752);
        for pass_on in [false, true] {
            let mut kernel = Kernel::default();
            kernel.fixes.query_statuses_pass_on = pass_on;
            let curve = original_ellipse(&kernel);
            let handle = OperandObject::Curve(0, curve.natural_interval());
            for scale in [1.0, 2.0] {
                let normal = [Vec3::Z * scale];
                let tolerance = 1e-5 * scale * scale;
                let expected = 6.327472493603755 * scale * scale;
                let options = CurveSolveOptions {
                    handles: [Some(handle); 2],
                    opt_vectors: Some(&normal),
                    target_distance: None,
                };
                let answers = curve
                    .global_curve_solve(
                        &kernel,
                        &requested,
                        &curve,
                        &requested,
                        SolverOperation::ProjectedMaximize,
                        tolerance,
                        &options,
                        SolutionRequested::All,
                    )
                    .unwrap();
                assert_eq!(answers.len(), 2);
                for answer in answers.as_slice() {
                    let s = answer.start.parameters[0];
                    let t = answer.start.parameters[1];
                    assert!(curve.natural_interval().contains_value(s, 0.0));
                    assert!(curve.natural_interval().contains_value(t, 0.0));
                    assert!((answer.start.solution_value - expected).abs() < 1e-12 * scale * scale);
                    let delta = curve.evaluate_point(&kernel, s).unwrap()
                        - curve.evaluate_point(&kernel, t).unwrap();
                    assert_eq!(
                        answer.start.solution_value,
                        normal[0].cross(delta).cross(normal[0]).length()
                    );
                }
                let options = CurveSolveOptions {
                    target_distance: Some(expected + 1.0),
                    ..options
                };
                assert!(curve
                    .global_curve_solve(
                        &kernel,
                        &requested,
                        &curve,
                        &requested,
                        SolverOperation::ProjectedMaximize,
                        tolerance,
                        &options,
                        SolutionRequested::All,
                    )
                    .unwrap()
                    .is_empty());
            }
            let normal = [Vec3::X];
            let result = curve.global_curve_solve(
                &kernel,
                &requested,
                &curve,
                &requested,
                SolverOperation::ProjectedMaximize,
                1e-5,
                &CurveSolveOptions {
                    handles: [Some(handle); 2],
                    opt_vectors: Some(&normal),
                    target_distance: None,
                },
                SolutionRequested::All,
            );
            assert!(matches!(
                result,
                Err(SolidError::Unsupported {
                    operation: "curve::curve::projected_pair_ellipse",
                    ..
                })
            ));
        }
    }

    #[test]
    fn original_ellipse_edge_encloses_the_stronger_native_parameter_witness() {
        let kernel = Kernel::default();
        let curve = original_ellipse(&kernel);
        assert!(supports([&curve, &curve], [curve.natural_interval(); 2]));
        let definition = Definition::of(&curve, curve.natural_interval()).unwrap();
        projected_frame(&curve, &definition, Vec3::Z).unwrap();
        assert!(projected_frame(&curve, &definition, Vec3::ZERO).is_err());
        assert!(projected_frame(&curve, &definition, Vec3::X).is_err());
        let (fixed, _) = definition.jet(Ivl::from(1.0), None).unwrap();
        let mut roots = Vec::new();
        for piece in definition.pieces(curve.natural_interval()).unwrap() {
            roots.extend(
                isolate(&kernel, piece.range, |t| {
                    edge_jet(&definition, piece, fixed, Vec3::Z, t)
                })
                .unwrap(),
            );
        }
        assert_eq!(roots.len(), 1);
        let root = roots[0].parameter;
        assert!(root.lo < 0.381209626 && root.hi > 0.381209624);
        assert!(root.hi - root.lo < 1e-10);
        // This tests the original native parameter map, not STEP degrees.
        let (point, _) = definition.jet(root, None).unwrap();
        let distance = norm_squared(project(sub(point, fixed), V3::from_vec3(Vec3::Z))).sqrt();
        assert!(distance.lo <= 6.327472493603755 && distance.hi >= 6.327472493603755);
        let piece = definition
            .pieces(curve.natural_interval())
            .unwrap()
            .into_iter()
            .find(|piece| piece.range.lo <= root.lo && root.hi <= piece.range.hi)
            .unwrap();
        let objective = edge_value_over(&definition, piece, fixed, Vec3::Z, root).unwrap();
        assert!(objective.lo <= 6.327472493603755 && objective.hi >= 6.327472493603755);
        assert!(objective.hi - objective.lo < 2e-11);
        let query = Query {
            kernel: &kernel,
            curves: [&curve, &curve],
            definitions: [
                Definition::of(&curve, curve.natural_interval()).unwrap(),
                Definition::of(&curve, curve.natural_interval()).unwrap(),
            ],
            normal: Vec3::Z,
        };
        let maxima = finite_candidates(&query, [curve.natural_interval(); 2]).unwrap();
        let best_lower = maxima
            .iter()
            .map(|maximum| maximum.point.lower)
            .fold(0.0_f64, f64::max);
        for maximum in &maxima {
            if maximum.objective.hi >= best_lower - 0.5 {
                assert!(
                    maximum.objective.hi <= maximum.point.upper,
                    "root parameters={:?}, objective={:?}, point=[{},{}], reported={}",
                    maximum.point.parameters,
                    maximum.objective,
                    maximum.point.lower,
                    maximum.point.upper,
                    maximum.point.reported
                );
            }
        }
        for tolerance in [0.0, 1e-5, 0.5] {
            let answers = solve::<crate::geom::solution_array::OperandObject>(
                &kernel,
                [&curve, &curve],
                [curve.natural_interval(); 2],
                Vec3::Z,
                tolerance,
                [None, None],
                None,
            )
            .unwrap();
            assert!(!answers.is_empty());
            if tolerance > 0.0 {
                assert_eq!(answers.len(), 2);
            }
            for answer in answers.as_slice() {
                assert!((answer.start.solution_value - 6.327472493603755).abs() < 1e-12);
            }
        }

        let global_upper = maxima
            .iter()
            .map(|m| m.objective.hi)
            .fold(0.0_f64, f64::max);
        let retained: Vec<_> = maxima
            .iter()
            .filter(|m| m.point.reported >= global_upper - 1e-5)
            .collect();
        assert_eq!(retained.len(), 2);
        for maximum in retained {
            assert!((maximum.point.reported - 6.327472493603755).abs() < 1e-12);
            assert!(maximum.point.parameters.contains(&1.0));
        }
    }
}
