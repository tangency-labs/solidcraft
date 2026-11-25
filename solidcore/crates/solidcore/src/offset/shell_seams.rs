//! Not upstream (solidcore#3353, acid-8 stage 5): a shell's cones and cylinders with their
//! seams turned clear of the holes in them before the offset, and turned back after it on
//! the faces the shell keeps as they were.
//!
//! The offset keeps each face's topology: an offset face has the loops of its face, a seam
//! edge (both sides on one closed face) offsets to the seam of the offset surface, and each
//! inner loop to the offset of its edges. An inner loop that grows in the offset (a hole's
//! rim on a cylinder shelled inward: the hole's radius grows by the wall while the cylinder's
//! shrinks) can reach the seam the face's loop did not reach. Its offset then crosses the
//! seam, which no edge of the offset face splits, and the mitre at the hole is refused (a
//! Ø100 drum with one Ø6 radial hole whose rim ends 1.7° short of the seam, shelled 2: the
//! hole's offset reaches 0.15° past the seam, and its rim's offset no longer closes,
//! `SplitAtEdgeEnd`). The same hole 15° round, or a 1 mm wall, builds.
//!
//! A seam is the surface's parameterisation, not a feature of the body (#2864): the face is
//! the same point set with its seam on any ruling. So before the shell each such face is
//! given its own surface turned about its axis ([`turn_seam`]) so that the seam runs down the
//! middle of the widest angle its inner loops leave free, as far from every hole as a ruling
//! can be. After the shell, every face still on a turned surface (an input face the shell
//! kept: a closed shell's outside, an open shell's walls' outsides) is turned back, so the
//! result's outside is parameterised as the input was; the offset faces keep the turned seam.
//!
//! The angles a loop covers are enclosed exactly, without sampling: a rational B-spline lies
//! in the convex hull of its control points (positive weights), and a hull that lies in a
//! half-plane through the axis (its points' angles within less than half a turn) covers no
//! more angles than its points. A loop whose points' angles span half a turn or more is not
//! enclosed this way and leaves its face as it is.
//!
//! The faces turned: a face on a cone or cylinder that closes round its axis, whose seam
//! edges' loops are made of seams and closed edges only (a band between two rims, as an
//! extruded circle makes: no hole already crosses the seam), with one inner loop or more
//! whose angles are enclosed, that leave a gap. Other faces, and a face whose turn fails,
//! keep their seams (the turn is tried on a copy).

use crate::error::Result;
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::{Point3, PI};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::seam_turn::turn_seam;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, Brep, EdgeId, FaceId};

/// A full turn, in radians.
const TURN: f64 = 2.0 * PI;

/// A cone or cylinder face of the shell's input whose seam was turned: the surface it had
/// and the surface it was given.
#[derive(Debug, Clone)]
pub(crate) struct TurnedSeam {
    was: Surface,
    turned: Surface,
}

/// The seams of `b`'s cone and cylinder faces turned clear of their inner loops (module
/// docs), in place; the turns made, for [`seams_back`].
///
/// # Errors
///
/// A cancellation, or an unported step, of a turn or a query. Any other failure leaves the
/// face it concerns as it was.
pub(crate) fn seams_clear_of_loops(k: &mut Kernel, b: &mut Brep) -> Result<Vec<TurnedSeam>> {
    let mut turned = Vec::new();
    for f in bp1::faces(b, None)? {
        let placed = match clear_seam(k, b, f) {
            Ok(Some(placed)) => placed,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(None) | Err(_) => continue,
        };
        let Some(was) = b.face(f)?.surface.clone() else {
            continue;
        };
        let mut trial = b.clone_standing_for_self_keeping_definitions()?;
        match turn_seam(k, &mut trial, f, placed.clone()) {
            Ok(()) => {
                *b = trial;
                turned.push(TurnedSeam {
                    was,
                    turned: placed,
                });
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => {}
        }
    }
    Ok(turned)
}

/// The seams [`seams_clear_of_loops`] turned, put back on `b` (a shell's result): each face
/// whose surface is still one of the turned surfaces is given the surface it had.
///
/// # Errors
///
/// A cancellation, or an unported step, of a turn. A face whose turn back fails keeps the
/// turned seam: the same face, parameterised from another ruling.
pub(crate) fn seams_back(k: &mut Kernel, b: &mut Brep, turned: &[TurnedSeam]) -> Result<()> {
    if turned.is_empty() {
        return Ok(());
    }
    for f in bp1::faces(b, None)? {
        let Some(SurfaceKind::Cone(now)) = b.face(f)?.surface.as_ref().map(Surface::kind) else {
            continue;
        };
        let Some(t) = turned
            .iter()
            .find(|t| matches!(t.turned.kind(), SurfaceKind::Cone(c) if c.approx_eq(now)))
        else {
            continue;
        };
        let mut trial = b.clone_standing_for_self_keeping_definitions()?;
        match turn_seam(k, &mut trial, f, t.was.clone()) {
            Ok(()) => *b = trial,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => {}
        }
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Face `f`'s surface turned about its axis so that its seam lies in the middle of the
/// widest gap its inner loops leave, or `None` where the face is not one the module turns.
fn clear_seam(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Option<Surface>> {
    let Some(surface) = b.face(f)?.surface.clone() else {
        return Ok(None);
    };
    let SurfaceKind::Cone(cone) = surface.kind() else {
        return Ok(None);
    };
    let rev = cone.rev();
    if rev.anal_uv_domain.u_interval().length() < 360.0 {
        return Ok(None);
    }
    let position = *rev.position();
    let Some(fu) = face2::upward_faceuse(b, f)? else {
        return Ok(None);
    };
    let mut seam: Option<EdgeId> = None;
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for lu in b.faceuse(fu)?.loopuses.clone() {
        let mut edges: Vec<EdgeId> = Vec::new();
        for u in eu::loopuse_edgeuses(b, lu)? {
            let Some(e) = eu::edge(b, u)? else {
                return Ok(None);
            };
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
        let mut seams = Vec::new();
        for &e in &edges {
            if edge::faces(b, e, None)?.as_slice() == [f] && edge::is_seam(k, b, e)? {
                seams.push(e);
            }
        }
        if seams.is_empty() {
            // an inner loop: the angles it covers
            let mut angles = Vec::new();
            for &e in &edges {
                let Some(points) = control_points(k, b, e)? else {
                    return Ok(None);
                };
                for p in points {
                    let Some(a) = angle_about(&position, p) else {
                        return Ok(None);
                    };
                    angles.push(a);
                }
            }
            let Some(w) = covering_arc(&mut angles) else {
                return Ok(None);
            };
            if w.1 >= PI {
                return Ok(None);
            }
            windows.push(w);
            continue;
        }
        // the seam's loop: seams and closed edges only (no loop crosses the seam yet)
        for &e in &edges {
            if seams.contains(&e) {
                continue;
            }
            let (s, z) = edge::start_end_vertices(b, e)?;
            if s.is_none() || s != z {
                return Ok(None);
            }
        }
        seam = seam.or_else(|| seams.first().copied());
    }
    let Some(seam) = seam else {
        return Ok(None);
    };
    let Some(gap) = widest_gap(&mut windows) else {
        return Ok(None);
    };
    let Some(from) = edge::start_end_vertices(b, seam)?.0 else {
        return Ok(None);
    };
    let Some(now) = angle_about(&position, b.vertex(from)?.point) else {
        return Ok(None);
    };
    let to = gap.0 + 0.5 * gap.1;
    let by = (to - now).rem_euclid(TURN);
    // a seam already within a thousandth of a turn of the gap's middle stays
    let off = by.min(TURN - by);
    if off <= 1.0e-3 * TURN {
        return Ok(None);
    }
    let (sin, cos) = by.sin_cos();
    let x = position.x_axis() * cos + position.y_axis() * sin;
    let turned = Axis2Placement::new(position.origin(), x, position.z_axis().cross(x));
    let mut placed = surface.clone();
    placed.transform(k, &Axis2Placement::from_to(&position, &turned), None)?;
    Ok(Some(placed))
}

/// The Euclidean control points of edge `e`'s curve trimmed to the edge, `None` where it has
/// none or a weight that is not positive (no convex hull to enclose it).
fn control_points(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Option<Vec<Point3>>> {
    let Some(curve) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
        return Ok(None);
    };
    let crate::curve::curve::CurveKind::BSpline(c) = curve.kind() else {
        return Ok(None);
    };
    let (points, weights) = c.control_polygon()?;
    if c.is_rational()? && weights.iter().any(|w| w.is_nan() || *w <= 0.0) {
        return Ok(None);
    }
    Ok(Some(points))
}

/// The angle of `p` about `position`'s Z axis from its X axis, in `[0, 2π)`; `None` for a
/// point within a millionth of a unit of the axis.
fn angle_about(position: &Axis2Placement, p: Point3) -> Option<f64> {
    let d = p - position.origin();
    let (x, y) = (d.dot(position.x_axis()), d.dot(position.y_axis()));
    if x.hypot(y) <= 1.0e-6 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(y.atan2(x).rem_euclid(TURN))
}

/// The shortest arc holding every angle of `angles` (sorted in place), as its start and
/// width; `None` for no angles.
fn covering_arc(angles: &mut [f64]) -> Option<(f64, f64)> {
    angles.sort_by(f64::total_cmp);
    let (first, last) = (*angles.first()?, *angles.last()?);
    // the widest gap between neighbours, the one round past the last angle included
    let (mut gap, mut after) = (first + TURN - last, last);
    for w in angles.windows(2) {
        if let [a, z] = w {
            if z - a > gap {
                gap = z - a;
                after = *a;
            }
        }
    }
    // the arc starts where the gap ends
    let start = (after + gap).rem_euclid(TURN);
    Some((start, TURN - gap))
}

/// The widest gap the arcs `windows` (start, width) leave round the turn, as its start and
/// width; `None` where they cover the turn or there are none.
fn widest_gap(windows: &mut [(f64, f64)]) -> Option<(f64, f64)> {
    windows.sort_by(|a, z| a.0.total_cmp(&z.0));
    let first = windows.first()?.0;
    // walk the arcs from the first start, a turn round: each gap is from the farthest
    // end reached so far to the next start, an arc that runs on past the turn reaching
    // into the walk's start
    let mut reach = windows.iter().fold(first, |r, &(s, w)| r.max(s + w - TURN));
    let mut best: Option<(f64, f64)> = None;
    let mut starts: Vec<(f64, f64)> = windows.to_vec();
    starts.push((first + TURN, 0.0));
    for (s, w) in starts {
        if s > reach && best.is_none_or(|b| s - reach > b.1) {
            best = Some((reach, s - reach));
        }
        reach = reach.max(s + w);
    }
    best.map(|(s, w)| (s.rem_euclid(TURN), w))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::{covering_arc, widest_gap, TURN};

    /// The arc round three angles is the turn less the widest gap between them, from the
    /// angle after the gap; across the start of the turn it starts before it.
    #[test]
    fn the_covering_arc_skips_the_widest_gap() {
        let (s, w) = covering_arc(&mut [1.0, 1.5, 2.0]).unwrap();
        assert!(
            (s - 1.0).abs() < 1e-12 && (w - 1.0).abs() < 1e-12,
            "{s} {w}"
        );
        let (s, w) = covering_arc(&mut [0.1, TURN - 0.2]).unwrap();
        assert!(
            (s - (TURN - 0.2)).abs() < 1e-12 && (w - 0.3).abs() < 1e-12,
            "{s} {w}"
        );
    }

    /// Two arcs leave two gaps: the wider one is found, also when an arc runs past a turn;
    /// arcs that cover the turn leave none.
    #[test]
    fn the_widest_gap_between_arcs() {
        // [1, 1.5] and [3, 3.5] leave [1.5, 3] and [3.5, 1 + 2 pi]
        let (s, w) = widest_gap(&mut [(1.0, 0.5), (3.0, 0.5)]).unwrap();
        assert!(
            (s - 3.5).abs() < 1e-12 && (w - (TURN - 2.5)).abs() < 1e-12,
            "{s} {w}"
        );
        let (s, w) = widest_gap(&mut [(TURN - 0.5, 1.0), (2.0, 0.5)]).unwrap();
        assert!(
            (s - 2.5).abs() < 1e-12 && (w - (TURN - 3.0)).abs() < 1e-12,
            "{s} {w}"
        );
        assert!(widest_gap(&mut [(0.0, 4.0), (3.5, 3.0)]).is_none());
        // an arc from just under the turn on past its start covers what follows the start
        let (s, w) = widest_gap(&mut [(0.5, 0.1), (TURN - 0.1, 2.0)]).unwrap();
        assert!(
            (s - 1.9).abs() < 1e-12 && (w - (TURN - 2.0)).abs() < 1e-12,
            "{s} {w}"
        );
    }
}
