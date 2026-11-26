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
//! The faces turned ([`Placement::Turn`]): a face on a cone or cylinder that closes round its
//! axis, whose seam edges' loops are made of seams and closed edges only (a band between two
//! rims, as an extruded circle makes: no hole already crosses the seam), with one inner loop
//! or more whose angles are enclosed, that leave a gap. Every other face is not one the
//! module turns, and [`Leave`] says why: that is not a failure, and the face is passed by.
//!
//! A face the module meant to turn whose turn fails, or whose placement could not be found
//! (a query on it failed), is a failure, kept as a [`SeamNotTurned`] with its typed cause in
//! the shell's [`SeamReport`] (the turn is tried on a copy, so the face keeps its seam). It
//! does not refuse the shell: a seam turn moves no point of the body, so before the shell the
//! face only keeps the seam it had (the shell builds, or is refused, as without the turn),
//! and after it the result is the same valid body with that face's seam on the turned ruling.
//! A cancellation or an unported step is passed on, as everywhere.

use crate::error::{Result, SolidError};
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

/// Why a face is not one whose seam the module turns: the face does not apply, which is
/// not a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// Not a face on a cone or cylinder that closes round its axis.
    NotAClosedCone,
    /// Not a band: no upward face use, no seam edge, a seam's loop with an edge that is
    /// neither a seam nor closed (a loop already crosses the seam), or a seam whose start
    /// vertex is missing or on the axis.
    NotABand,
    /// An inner loop whose angles are not enclosed: an edge use without an edge, an edge
    /// with no trimmed B-spline or a weight that is not positive, or a control point on
    /// the axis.
    LoopNotEnclosed,
    /// An inner loop whose angles span half a turn or more.
    LoopSpansHalfTurn,
    /// No inner loop: nothing for the seam to clear.
    NoHole,
    /// The inner loops leave no gap round the axis.
    NoGap,
    /// The seam is already within a thousandth of a turn of the widest gap's middle.
    AlreadyClear,
}

/// What [`placement`] makes of a face: the surface to give it, or why it is left.
#[derive(Debug, Clone)]
pub enum Placement {
    /// The face's surface turned about its axis, its seam in the middle of the widest gap
    /// its inner loops leave (boxed: a surface is large).
    Turn(Box<Surface>),
    /// The face is not one the module turns.
    Leave(Leave),
}

/// A face whose seam the module meant to turn and did not: it keeps the seam it had.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamNotTurned {
    /// The face: of the shell's input for a turn before the shell, of the result for a
    /// turn back.
    pub face: FaceId,
    /// Why: the failed query of its placement, or the failed turn.
    pub cause: SolidError,
}

/// The seam turns of one shell that failed (module docs). Each such face is the same point
/// set as it would have been, so the shell stands; the causes are kept here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeamReport {
    /// Faces of the input whose seam was not turned clear of their holes: each kept its
    /// seam, and the shell was built as without the turn.
    pub not_cleared: Vec<SeamNotTurned>,
    /// Faces of the result whose seam was not turned back: each is the input's face with
    /// its seam on the ruling it was turned to, not the input's.
    pub not_restored: Vec<SeamNotTurned>,
}

/// The seams [`seams_clear_of_loops`] turned, and the faces whose turn failed.
#[derive(Debug, Clone, Default)]
pub(crate) struct SeamsCleared {
    /// The turns made, for [`seams_back`].
    pub(crate) turned: Vec<TurnedSeam>,
    /// The faces that kept their seams for a failure, for [`SeamReport::not_cleared`].
    pub(crate) not_cleared: Vec<SeamNotTurned>,
}

/// The seams of `b`'s cone and cylinder faces turned clear of their inner loops (module
/// docs), in place: the turns made, for [`seams_back`], and the faces whose placement or
/// turn failed, each with its cause (it keeps its seam).
///
/// # Errors
///
/// Those of listing `b`'s faces and copying it; a cancellation, or an unported step, of a
/// placement or a turn.
pub(crate) fn seams_clear_of_loops(k: &mut Kernel, b: &mut Brep) -> Result<SeamsCleared> {
    let mut out = SeamsCleared::default();
    for f in bp1::faces(b, None)? {
        let (was, placed) = match placement(k, b, f) {
            Ok(Placement::Turn(placed)) => match b.face(f)?.surface.clone() {
                Some(was) => (was, *placed),
                // `placement` turns only a face with a surface
                None => continue,
            },
            Ok(Placement::Leave(_)) => continue,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(cause) => {
                out.not_cleared.push(SeamNotTurned { face: f, cause });
                continue;
            }
        };
        match turn_on_copy(k, b, f, placed.clone())? {
            None => out.turned.push(TurnedSeam {
                was,
                turned: placed,
            }),
            Some(cause) => out.not_cleared.push(SeamNotTurned { face: f, cause }),
        }
    }
    Ok(out)
}

/// The seams [`seams_clear_of_loops`] turned, put back on `b` (a shell's result): each face
/// whose surface is still one of the turned surfaces is given the surface it had. The faces
/// whose turn back failed, each with its cause: such a face keeps the turned seam (the same
/// face, parameterised from another ruling).
///
/// # Errors
///
/// Those of listing `b`'s faces, reading one and copying `b`; a cancellation, or an unported
/// step, of a turn.
pub(crate) fn seams_back(
    k: &mut Kernel,
    b: &mut Brep,
    turned: &[TurnedSeam],
) -> Result<Vec<SeamNotTurned>> {
    let mut not_restored = Vec::new();
    if turned.is_empty() {
        return Ok(not_restored);
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
        if let Some(cause) = turn_on_copy(k, b, f, t.was.clone())? {
            not_restored.push(SeamNotTurned { face: f, cause });
        }
    }
    b.refresh_cached_topology();
    Ok(not_restored)
}

/// Face `f` of `b` given the surface `to` ([`turn_seam`]) on a copy of `b`, which replaces
/// `b` where the turn builds: `None` then; otherwise the turn's error, and `b` as it was.
///
/// # Errors
///
/// Those of copying `b`; a cancellation, or an unported step, of the turn.
fn turn_on_copy(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    to: Surface,
) -> Result<Option<SolidError>> {
    let mut trial = b.clone_standing_for_self_keeping_definitions()?;
    match turn_seam(k, &mut trial, f, to) {
        Ok(()) => {
            *b = trial;
            Ok(None)
        }
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(cause) => Ok(Some(cause)),
    }
}

/// Face `f`'s surface turned about its axis so that its seam lies in the middle of the
/// widest gap its inner loops leave, or why the face is not one the module turns.
///
/// # Errors
///
/// Those of the queries of `f`'s surface, loops, edges and vertices, of its edges' trimmed
/// B-splines, and of placing the turned surface.
pub fn placement(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Placement> {
    let leave = |why| Ok(Placement::Leave(why));
    let Some(surface) = b.face(f)?.surface.clone() else {
        return leave(Leave::NotAClosedCone);
    };
    let SurfaceKind::Cone(cone) = surface.kind() else {
        return leave(Leave::NotAClosedCone);
    };
    let rev = cone.rev();
    if rev.anal_uv_domain.u_interval().length() < 360.0 {
        return leave(Leave::NotAClosedCone);
    }
    let position = *rev.position();
    let Some(fu) = face2::upward_faceuse(b, f)? else {
        return leave(Leave::NotABand);
    };
    let mut seam: Option<EdgeId> = None;
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for lu in b.faceuse(fu)?.loopuses.clone() {
        let mut edges: Vec<EdgeId> = Vec::new();
        for u in eu::loopuse_edgeuses(b, lu)? {
            let Some(e) = eu::edge(b, u)? else {
                return leave(Leave::LoopNotEnclosed);
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
                    return leave(Leave::LoopNotEnclosed);
                };
                for p in points {
                    let Some(a) = angle_about(&position, p) else {
                        return leave(Leave::LoopNotEnclosed);
                    };
                    angles.push(a);
                }
            }
            let Some(w) = covering_arc(&mut angles) else {
                return leave(Leave::LoopNotEnclosed);
            };
            if w.1 >= PI {
                return leave(Leave::LoopSpansHalfTurn);
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
                return leave(Leave::NotABand);
            }
        }
        seam = seam.or_else(|| seams.first().copied());
    }
    let Some(seam) = seam else {
        return leave(Leave::NotABand);
    };
    if windows.is_empty() {
        return leave(Leave::NoHole);
    }
    let Some(gap) = widest_gap(&mut windows) else {
        return leave(Leave::NoGap);
    };
    let Some(from) = edge::start_end_vertices(b, seam)?.0 else {
        return leave(Leave::NotABand);
    };
    let Some(now) = angle_about(&position, b.vertex(from)?.point) else {
        return leave(Leave::NotABand);
    };
    let to = gap.0 + 0.5 * gap.1;
    let by = (to - now).rem_euclid(TURN);
    // a seam already within a thousandth of a turn of the gap's middle stays
    let off = by.min(TURN - by);
    if off <= 1.0e-3 * TURN {
        return leave(Leave::AlreadyClear);
    }
    let (sin, cos) = by.sin_cos();
    let x = position.x_axis() * cos + position.y_axis() * sin;
    let turned = Axis2Placement::new(position.origin(), x, position.z_axis().cross(x));
    let mut placed = surface.clone();
    placed.transform(k, &Axis2Placement::from_to(&position, &turned), None)?;
    Ok(Placement::Turn(Box::new(placed)))
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
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::{
        covering_arc, placement, seams_back, seams_clear_of_loops, turn_on_copy, widest_gap, Leave,
        Placement, TURN,
    };
    use crate::boolean::api_brep::boolean_difference;
    use crate::kernel::Kernel;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_primitives::create_cylinder;
    use crate::surface::surface::{Surface, SurfaceKind};
    use crate::topology::brep::p1 as bp1;
    use crate::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
    use crate::topology::{Brep, FaceId};

    /// A Ø100 × 120 drum about Z with a Ø6 radial hole from 30 off the axis out through its
    /// side at Z = 20, the hole's axis `at` degrees round from +X.
    fn drum_with_a_hole(k: &mut Kernel, at: f64) -> Brep {
        let drum = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 50.0, 120.0).unwrap();
        let mut drill = create_cylinder(k, Point3::new(30.0, 0.0, 20.0), 3.0, 25.0).unwrap();
        let (y, z) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        let drilled = Some(TransformObject::Brep(&mut drill));
        api_rotate(k, drilled, Point3::new(30.0, 0.0, 20.0), y, 90.0).unwrap();
        let drilled = Some(TransformObject::Brep(&mut drill));
        api_rotate(k, drilled, Point3::new(0.0, 0.0, 0.0), z, at).unwrap();
        let mut part = boolean_difference(k, drum, drill).unwrap();
        part.refresh_cached_topology();
        part
    }

    /// The faces of `b` with what [`placement`] makes of each: `None` for a turn.
    fn placements(k: &Kernel, b: &mut Brep) -> Vec<(FaceId, Option<Leave>)> {
        let mut out = Vec::new();
        for f in bp1::faces(b, None).unwrap() {
            let why = match placement(k, b, f).unwrap() {
                Placement::Turn(_) => None,
                Placement::Leave(why) => Some(why),
            };
            out.push((f, why));
        }
        out
    }

    /// Whether face `f` of `b` is on a cylinder of radius `r`.
    fn on_cylinder(b: &Brep, f: FaceId, r: f64) -> bool {
        matches!(
            b.face(f).unwrap().surface.as_ref().map(Surface::kind),
            Some(SurfaceKind::Cone(c)) if c.is_cylinder() && (c.radius(0.0) - r).abs() < 1e-9
        )
    }

    /// Only the drum's side, a band with a hole, is turned. The caps and the hole's floor are
    /// not cones, the hole's wall and a plain drum's side are bands with no hole: each is
    /// left, for its own reason, and none of that is a failure.
    #[test]
    fn a_band_with_a_hole_is_turned_and_every_other_face_left_for_its_reason() {
        let mut k = Kernel::default();
        let mut b = drum_with_a_hole(&mut k, 40.0);
        let got = placements(&k, &mut b);
        assert_eq!(got.len(), 5, "{got:?}");
        for &(f, why) in &got {
            let want = if on_cylinder(&b, f, 50.0) {
                None
            } else if on_cylinder(&b, f, 3.0) {
                Some(Leave::NoHole)
            } else {
                Some(Leave::NotAClosedCone)
            };
            assert_eq!(why, want, "face {f:?}: {got:?}");
        }
        let mut plain = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 50.0, 120.0).unwrap();
        let got = placements(&k, &mut plain);
        let sides: Vec<_> = got
            .iter()
            .filter(|(_, why)| *why == Some(Leave::NoHole))
            .collect();
        assert_eq!(sides.len(), 1, "{got:?}");
        // the drum's side turned: its seam is then in the middle of the gap, and left
        let cleared = seams_clear_of_loops(&mut k, &mut b).unwrap();
        assert_eq!(cleared.turned.len(), 1);
        assert!(cleared.not_cleared.is_empty(), "{:?}", cleared.not_cleared);
        let side = got_side(&b);
        assert!(matches!(
            placement(&k, &mut b, side).unwrap(),
            Placement::Leave(Leave::AlreadyClear)
        ));
    }

    /// The drum's side face of `b`.
    fn got_side(b: &Brep) -> FaceId {
        bp1::faces(b, None)
            .unwrap()
            .into_iter()
            .find(|&f| on_cylinder(b, f, 50.0))
            .unwrap()
    }

    /// Whether face `f` of `b` is on the cone `c`, placed as it is.
    fn on_cone(b: &Brep, f: FaceId, c: &Surface) -> bool {
        let SurfaceKind::Cone(c) = c.kind() else {
            return false;
        };
        matches!(
            b.face(f).unwrap().surface.as_ref().map(Surface::kind),
            Some(SurfaceKind::Cone(now)) if now.approx_eq(c)
        )
    }

    /// A turn that fails is a failure, not a face left: its typed cause comes back and the
    /// body is as it was (a turn of a face the body does not have). A turn that builds
    /// replaces the body, and the turn back reports no face kept on the turned seam.
    #[test]
    fn a_failed_turn_keeps_its_cause_and_leaves_the_body_as_it_was() {
        let mut k = Kernel::default();
        let mut b = drum_with_a_hole(&mut k, 40.0);
        let side = got_side(&b);
        let input = b.face(side).unwrap().surface.clone().unwrap();
        let faces = bp1::faces(&b, None).unwrap().len();
        let cause = turn_on_copy(&mut k, &mut b, FaceId::from_index(999), input.clone())
            .unwrap()
            .expect("the turn of a face the body lacks fails");
        assert!(!cause.is_unported_or_cancelled(), "{cause}");
        assert_eq!(bp1::faces(&b, None).unwrap().len(), faces);
        assert!(on_cone(&b, side, &input));
        // turned clear of the hole and back
        let cleared = seams_clear_of_loops(&mut k, &mut b).unwrap();
        assert!(!on_cone(&b, side, &input));
        let not_restored = seams_back(&mut k, &mut b, &cleared.turned).unwrap();
        assert!(not_restored.is_empty(), "{not_restored:?}");
        assert!(on_cone(&b, side, &input));
    }

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
