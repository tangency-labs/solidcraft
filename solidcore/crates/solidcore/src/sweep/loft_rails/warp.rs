//! The warp of a station guided by three or more rails: SolidCraft's `warp_station`, `solve` and
//! `refine` (PR #12188), on the kernel's plain curves.
//!
//! With three or more guided points, one affine move across the station's plane fits the
//! rails best (exact for three), and what is left is taken up by a smooth field of inverse
//! multiquadrics about the guided points. The field is solved through the curves, not their
//! control points: a curve's point is linear in its control points for fixed weights, so the
//! field moves every point of the station the same way wherever it is, and curves that meet
//! still meet. A station's section curves have few control points (a line has two), so
//! [`refine`] gives each curve spans first (at least [`WARP_SPANS`]), the curve's points and
//! parameters unchanged.

use super::{invalid, wire_point, Plain};
use crate::error::Result;
use crate::math::det;
use crate::math::{Point3, Vec3};
use crate::nurbs::crv_basic::Curve as NurbCurve;

/// Spans each curve of a loft of three or more rails is given before the stations are warped.
pub(super) const WARP_SPANS: usize = 8;

/// The points at wire parameters `gs` of `plains` (each curve built once).
fn at_wires(plains: &[Plain], gs: &[f64]) -> Result<Vec<Point3>> {
    let curves = plains
        .iter()
        .map(|p| -> Result<(Plain, NurbCurve)> { Ok((p.clone(), p.nurb()?)) })
        .collect::<Result<Vec<_>>>()?;
    gs.iter().map(|g| wire_point(&curves, *g)).collect()
}

/// `station` moved so that its points guided by three or more rails (`pairs`: each guided
/// point, at wire parameter `gs`, with its rail's point) land on the rails. `normal` is the
/// station's plane's unit normal and `size` the loft's size (module docs).
pub(super) fn warp_station(
    station: &[Plain],
    gs: &[f64],
    pairs: &[(Point3, Point3)],
    normal: Vec3,
    size: f64,
) -> Result<Vec<Plain>> {
    let n = pairs.len() as f64;
    let (a_sum, q_sum) = pairs
        .iter()
        .fold((Vec3::ZERO, Vec3::ZERO), |(a, q), (p, t)| (a + *p, q + *t));
    let (a_mid, q_mid) = (a_sum * (1.0 / n), q_sum * (1.0 / n));
    // Two directions across the station's plane.
    let seed = if normal.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let e1 = (seed - normal * seed.dot(normal))
        .unitize()
        .map_err(|_| invalid("a station's plane"))?;
    let e2 = normal.cross(e1);
    // Least squares for the images U, V of e1, e2: q - q_mid ~ s U + t V.
    let (mut ss, mut st, mut tt) = (0.0, 0.0, 0.0);
    let (mut bs, mut bt) = (Vec3::ZERO, Vec3::ZERO);
    for (p, q) in pairs {
        let d = *p - a_mid;
        let (s, t) = (d.dot(e1), d.dot(e2));
        ss += s * s;
        st += s * t;
        tt += t * t;
        bs += (*q - q_mid) * s;
        bt += (*q - q_mid) * t;
    }
    let det = ss * tt - st * st;
    if det <= 1e-10 * det::powi(ss + tt, 2).max(1e-300) {
        return Err(invalid(
            "the guide rails meet the loft along one line: three or more rails have to surround it",
        ));
    }
    let u = (bs * tt - bt * st) * (1.0 / det);
    let v = (bt * ss - bs * st) * (1.0 / det);
    let affine = |x: Point3| {
        let d = x - a_mid;
        q_mid + u * d.dot(e1) + v * d.dot(e2) + normal * d.dot(normal)
    };
    let mut moved: Vec<Vec<Point3>> = station
        .iter()
        .map(|c| c.points.iter().map(|p| affine(*p)).collect())
        .collect();
    let with_points = |pts: &[Vec<Point3>]| -> Vec<Plain> {
        station
            .iter()
            .zip(pts)
            .map(|(c, p)| c.with(p.clone()))
            .collect()
    };
    // What the affine move leaves: each guided point's way still to its rail.
    let shaped = with_points(&moved);
    let at = at_wires(&shaped, gs)?;
    let rest: Vec<Vec3> = pairs.iter().zip(&at).map(|((_, q), p)| *q - *p).collect();
    let tiny = 1e-9 * size.max(1e-9);
    if rest.iter().all(|r| r.length() <= tiny) {
        return Ok(shaped);
    }
    // The field's width: the mean distance from each guided point to its nearest other.
    let width = at
        .iter()
        .enumerate()
        .map(|(i, p)| {
            at.iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, q)| p.distance(*q))
                .fold(f64::INFINITY, f64::min)
        })
        .sum::<f64>()
        / n;
    let c2 = det::powi(width.max(tiny), 2);
    // A point beside a smooth joint takes the field's first-order part about the joint, a
    // linear map, so the joint's three points stay in line and the station stays tangent there:
    // the field itself would bend them and crease the loft (#12188).
    let anchors = tangent_anchors(station, &moved, tiny);
    let phi = |x: Point3, centre: Point3, anchor: Option<Point3>| match anchor {
        None => 1.0 / (det::powi(x.distance(centre), 2) + c2).sqrt(),
        Some(a) => {
            let r2 = det::powi(a.distance(centre), 2) + c2;
            (1.0 - (a - centre).dot(x - a) / r2) / r2.sqrt()
        }
    };
    // column k: how far each guided point moves for a unit weight on centre k (the field at
    // the control points, through the curves).
    let columns: Vec<Vec<f64>> = at
        .iter()
        .map(|centre| {
            let field: Vec<Plain> = station
                .iter()
                .zip(&moved)
                .zip(&anchors)
                .map(|((c, e), an)| {
                    c.with(
                        e.iter()
                            .zip(an)
                            .map(|(p, a)| Vec3::new(phi(*p, *centre, *a), 0.0, 0.0))
                            .collect(),
                    )
                })
                .collect();
            Ok(at_wires(&field, gs)?.iter().map(|p| p.x).collect())
        })
        .collect::<Result<_>>()?;
    let m: Vec<Vec<f64>> = (0..at.len())
        .map(|row| {
            columns
                .iter()
                .map(|col| col.get(row).copied().unwrap_or(0.0))
                .collect()
        })
        .collect();
    let w = solve(m, rest)
        .ok_or_else(|| invalid("the guide rails are too close together to follow"))?;
    for (p, a) in moved.iter_mut().flatten().zip(anchors.iter().flatten()) {
        let d = at
            .iter()
            .zip(&w)
            .fold(Vec3::ZERO, |d, (centre, wk)| d + *wk * phi(*p, *centre, *a));
        *p += d;
    }
    Ok(with_points(&moved))
}

/// For each control point of `points` (the station's curves' control points), the smooth
/// joint it stands beside, if any: a point where the curves are tangent but the
/// parametrization is not smooth, either a knot of multiplicity equal to the degree inside a
/// curve or the shared end of two curves (the last and the first too, on a closed station).
/// A point beside two joints, or itself a joint or a curve's end, has none.
fn tangent_anchors(
    station: &[Plain],
    points: &[Vec<Point3>],
    tiny: f64,
) -> Vec<Vec<Option<Point3>>> {
    let mut anchors: Vec<Vec<Option<Point3>>> =
        points.iter().map(|p| vec![None; p.len()]).collect();
    let mut fixed: Vec<Vec<bool>> = points
        .iter()
        .map(|p| (0..p.len()).map(|i| i == 0 || i + 1 == p.len()).collect())
        .collect();
    // (curve, index) of a joint's point and of the points either side of it
    let mut joints: Vec<[(usize, usize); 3]> = Vec::new();
    for (c, plain) in station.iter().enumerate() {
        let len = points.get(c).map_or(0, Vec::len);
        let p = plain.degree;
        let mut j = p + 1;
        while j < plain.knots.len().saturating_sub(p + 1) {
            let Some(&u) = plain.knots.get(j) else { break };
            let m = plain.knots.iter().skip(j).take_while(|k| **k == u).count();
            if m == p && j >= 2 && j < len {
                joints.push([(c, j - 2), (c, j - 1), (c, j)]);
            }
            j += m.max(1);
        }
    }
    let n = points.len();
    for c in 0..n {
        let Some(next) = (c + 1).checked_rem(n) else {
            continue;
        };
        let (Some(a), Some(b)) = (points.get(c), points.get(next)) else {
            continue;
        };
        if a.len() < 3 || b.len() < 3 {
            continue;
        }
        let (Some(end), Some(start)) = (a.last(), b.first()) else {
            continue;
        };
        if end.distance(*start) <= tiny {
            joints.push([(c, a.len() - 2), (c, a.len() - 1), (next, 1)]);
        }
    }
    let at = |(c, i): (usize, usize)| points.get(c).and_then(|p| p.get(i)).copied();
    let mut used: Vec<Vec<u8>> = points.iter().map(|p| vec![0; p.len()]).collect();
    let mut smooth: Vec<[(usize, usize); 3]> = Vec::new();
    for joint in joints {
        let (Some(l), Some(m), Some(r)) = (at(joint[0]), at(joint[1]), at(joint[2])) else {
            continue;
        };
        let (u, v) = (m - l, r - m);
        let (lu, lv) = (u.length(), v.length());
        if lu <= tiny || lv <= tiny || u.dot(v) <= 0.0 || u.cross(v).length() > 1e-6 * lu * lv {
            continue;
        }
        if let Some(f) = fixed
            .get_mut(joint[1].0)
            .and_then(|f| f.get_mut(joint[1].1))
        {
            *f = true;
        }
        for side in [joint[0], joint[2]] {
            if let Some(k) = used.get_mut(side.0).and_then(|u| u.get_mut(side.1)) {
                *k += 1;
            }
        }
        smooth.push(joint);
    }
    for joint in smooth {
        let Some(m) = at(joint[1]) else { continue };
        for (c, i) in [joint[0], joint[2]] {
            let alone = used.get(c).and_then(|u| u.get(i)) == Some(&1);
            let free = fixed.get(c).and_then(|f| f.get(i)) == Some(&false);
            if alone && free {
                if let Some(slot) = anchors.get_mut(c).and_then(|a| a.get_mut(i)) {
                    *slot = Some(m);
                }
            }
        }
    }
    anchors
}

/// The entry `(r, c)` of a row-major system's matrix, its absolute value (0 past its end).
fn abs_at(rows: &[(Vec<f64>, Vec3)], r: usize, c: usize) -> f64 {
    rows.get(r)
        .and_then(|(row, _)| row.get(c))
        .map_or(0.0, |v| v.abs())
}

/// `m x = b` (a square system, each `b` a vector) by elimination with partial pivoting; `None`
/// when `m` is singular or not square.
fn solve(m: Vec<Vec<f64>>, b: Vec<Vec3>) -> Option<Vec<Vec3>> {
    let n = b.len();
    if m.len() != n || m.iter().any(|row| row.len() != n) {
        return None;
    }
    let mut rows: Vec<(Vec<f64>, Vec3)> = m.into_iter().zip(b).collect();
    let scale = rows
        .iter()
        .flat_map(|(row, _)| row.iter())
        .fold(0.0f64, |a, v| a.max(v.abs()));
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| abs_at(&rows, i, c).total_cmp(&abs_at(&rows, j, c)))?;
        rows.swap(c, p);
        let (head, tail) = rows.split_at_mut(c + 1);
        let (prow, pb) = head.last()?;
        let pivot = prow.get(c).copied()?;
        if pivot.abs() <= 1e-12 * scale {
            return None;
        }
        for (row, bv) in tail.iter_mut() {
            let f = row.get(c).copied()? / pivot;
            if f != 0.0 {
                for (x, y) in row.iter_mut().skip(c).zip(prow.iter().skip(c)) {
                    *x -= f * y;
                }
                *bv -= *pb * f;
            }
        }
    }
    // back substitution, the solution built last unknown first
    let mut sol: Vec<Vec3> = Vec::new();
    for r in (0..n).rev() {
        let (row, bv) = rows.get(r)?;
        let mut acc = *bv;
        for (k, xj) in sol.iter().enumerate() {
            acc -= *xj * row.get(n - 1 - k).copied()?;
        }
        let diag = row.get(r).copied()?;
        sol.push(acc * (1.0 / diag));
    }
    sol.reverse();
    Some(sol)
}

/// A column's curves with more control points for a warp to move, refined together: the first
/// curve decides for all of them whether a straight line (two control points, unit weights)
/// becomes a cubic, and which knots are inserted to give `spans` even spans, so curves that pair
/// still pair after it (each curve's own knots, within the pairing's tolerance, would otherwise
/// decide differently). The points and parameters are unchanged.
#[allow(clippy::cast_precision_loss)] // at most WARP_SPANS
pub(super) fn refine_column(column: &[Plain], spans: usize) -> Result<Vec<Plain>> {
    let Some(first) = column.first() else {
        return Ok(Vec::new());
    };
    let raise = is_straight(first);
    // a follower pairs with the leader, so it is raised with it whatever its own weights are
    let plains: Vec<Plain> = column
        .iter()
        .map(|c| {
            if raise && c.degree == 1 && c.points.len() == 2 {
                raised(c)
            } else {
                Ok(c.clone())
            }
        })
        .collect::<Result<_>>()?;
    let lead = plains.first().ok_or_else(|| invalid("a column's curves"))?;
    let (Some(&t0), Some(&t1)) = (
        lead.knots.get(lead.degree),
        lead.knots.get(lead.points.len()),
    ) else {
        return Err(invalid("a curve's knots"));
    };
    let mut new_knots: Vec<f64> = Vec::new();
    for i in 1..spans {
        let u = t0 + (t1 - t0) * i as f64 / spans as f64;
        if lead
            .knots
            .iter()
            .any(|k| (k - u).abs() <= 1e-12 * (1.0 + u.abs()))
        {
            continue;
        }
        new_knots.push(u);
    }
    if new_knots.is_empty() {
        return Ok(plains);
    }
    plains.iter().map(|p| insert_knots(p, &new_knots)).collect()
}

/// A straight line: two control points, unit weights.
fn is_straight(c: &Plain) -> bool {
    c.degree == 1
        && c.points.len() == 2
        && c.weights
            .as_ref()
            .is_none_or(|w| w.iter().all(|x| (x - 1.0).abs() < 1e-12))
}

/// The straight line `c` as a cubic on the same parameters, its control points spread evenly
/// along it.
fn raised(c: &Plain) -> Result<Plain> {
    let (Some(&t0), Some(&t1)) = (c.knots.get(c.degree), c.knots.get(c.points.len())) else {
        return Err(invalid("a curve's knots"));
    };
    let (Some(a), Some(b)) = (c.points.first(), c.points.get(1)) else {
        return Err(invalid("a curve's control points"));
    };
    Ok(Plain {
        degree: 3,
        knots: vec![t0, t0, t0, t0, t1, t1, t1, t1],
        points: (0..4)
            .map(|i| *a + (*b - *a) * (f64::from(i) / 3.0))
            .collect(),
        weights: None,
    })
}

/// `plain` with knots inserted at `new_knots`, the same curve.
fn insert_knots(plain: &Plain, new_knots: &[f64]) -> Result<Plain> {
    let refined = plain.nurb()?.refine(new_knots)?;
    let cp = refined.cpts();
    let weights = plain
        .weights
        .as_ref()
        .map(|_| cp.iter().map(|q| q.w).collect());
    Ok(Plain {
        degree: plain.degree,
        knots: refined.knots().to_vec(),
        points: cp.iter().map(|q| q.to_euclid()).collect(),
        weights,
    })
}

#[cfg(test)]
mod tests {
    use super::{at_wires, refine_column, warp_station, Plain, WARP_SPANS};
    use crate::math::{Point3, Vec3};

    /// The circle of radius 10 round the origin in the XY plane: four rational quadratic arcs
    /// with double knots, tangent where they meet only because the points there line up.
    fn circle() -> Plain {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        Plain {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            points: vec![
                p(10.0, 0.0),
                p(10.0, 10.0),
                p(0.0, 10.0),
                p(-10.0, 10.0),
                p(-10.0, 0.0),
                p(-10.0, -10.0),
                p(0.0, -10.0),
                p(10.0, -10.0),
                p(10.0, 0.0),
            ],
            weights: Some(vec![1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0]),
        }
    }

    /// Four rails that no affine move fits (one pulls its point out, the others hold theirs)
    /// warp the circle onto them, and it stays tangent at its four joints (#12188: a crease
    /// there made the loft's offset leave a gap, and Shell refuse it).
    #[test]
    fn a_warped_circle_stays_tangent_where_its_arcs_meet() {
        let station = refine_column(&[circle()], WARP_SPANS).unwrap();
        let gs = [0.0, 0.25, 0.5, 0.75];
        let at = at_wires(&station, &gs).unwrap();
        let targets = [at[0], at[1] + Vec3::new(0.0, 3.0, 0.0), at[2], at[3]];
        let pairs: Vec<(Point3, Point3)> = at.iter().copied().zip(targets).collect();
        let warped = warp_station(&station, &gs, &pairs, Vec3::Z, 20.0).unwrap();
        for (got, want) in at_wires(&warped, &gs).unwrap().iter().zip(targets) {
            assert!(got.distance(want) < 1e-9, "{got:?} against {want:?}");
        }
        let c = &warped[0];
        let mut joints = 0;
        for (j, k) in c
            .knots
            .iter()
            .enumerate()
            .skip(c.degree + 1)
            .take(c.points.len() - c.degree - 1)
        {
            if c.knots.iter().filter(|x| *x == k).count() != c.degree || c.knots[j - 1] == *k {
                continue;
            }
            let (l, m, r) = (c.points[j - 2], c.points[j - 1], c.points[j]);
            let (u, v) = (m - l, r - m);
            let sine = u.cross(v).length() / (u.length() * v.length());
            assert!(
                sine < 1e-9 && u.dot(v) > 0.0,
                "joint at knot {k}: sine {sine}"
            );
            joints += 1;
        }
        assert_eq!(
            joints, 3,
            "the joints inside the curve (its start and end are one point)"
        );
        let (n, first, second, last) = (
            c.points.len(),
            c.points[0],
            c.points[1],
            c.points[c.points.len() - 2],
        );
        let sine = (first - last).cross(second - first).length()
            / ((first - last).length() * (second - first).length());
        assert!(
            sine < 1e-9 && n > 3,
            "the joint where the circle closes: sine {sine}"
        );
    }

    /// The straight line from (0, 0, 0) to (10, 0, 0) with `weights` (`None` a polynomial line).
    fn line(weights: Option<Vec<f64>>) -> Plain {
        Plain {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            weights,
        }
    }

    /// A straight leader raises the column, and a follower line whose weights are 1 + 1e-10
    /// (within the pairing's tolerance, not within 1e-12) is raised with it: the column refines
    /// to curves of the same degree, control count and knots, so they still pair.
    #[test]
    fn a_follower_line_within_the_pairing_tolerance_is_raised_with_its_leader() {
        let column = [line(None), line(Some(vec![1.0 + 1e-10, 1.0 + 1e-10]))];
        let refined = refine_column(&column, 8).unwrap();
        let (a, b) = (&refined[0], &refined[1]);
        assert_eq!(a.degree, b.degree);
        assert_eq!(a.points.len(), b.points.len());
        assert_eq!(a.knots.len(), b.knots.len());
    }
}
