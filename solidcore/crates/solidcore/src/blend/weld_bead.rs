//! Weld beads as bodies of their own (#15129, the welds epic #15132): a bead built from its own
//! faces, apart from the body it is laid on, which is left as it was. No boolean with the
//! body and no copy of it, so the cost does not grow with the body.
//!
//! Between planes along a straight edge (all the bead does so far) the bead's body is its
//! section swept along the edge: the edge's point, the feet at the setbacks on each face, and
//! the arc between them ([`BeadArc`], the exact circle the blend builds the bead face from).
//! Its faces are the bead face, the strips of the two planes between the edge and the feet,
//! and the two section ends. A span ([`BeadSpan`]) sweeps its share of the edge only, so an
//! intermittent weld is one body per span with no cutting.
//!
//! A span ends square to the edge, or tapers, or is rounded ([`BeadEnd`]). Those two are cut
//! from the span's own small body: the span is swept past its end first, so each cut crosses
//! the bead's edges instead of passing through its corners, then
//! - a taper is cut by the plane through the span's end on the edge at 45 degrees to the edge,
//!   rising into the span along the bead's middle direction: a ramp as long as the bead is
//!   high, the strips narrowing to the end's point;
//! - a rounded end is cut by the sphere about the edge one bead reach inside the span's end:
//!   the bead's outline on each plate ends in a round, the section shrinking to the end's
//!   point on the edge.
//!
//! The bead never trims or changes the body (#15132).
//!
//! Not upstream (solidcore's). No recursion.

use crate::boolean::api_brep::boolean_difference;
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use core::f64::consts::TAU;
use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use crate::primitive::api_curves::create_line_segment;
use crate::primitive::api_primitives::create_sphere;
use crate::surface::procedural_blend::bead::{BeadArc, BeadDefinition, BeadSpan};
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::surface::surface::p2::SurfaceKind;
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{Brep, EdgeId, FaceId};

const OP: &str = "blend::weld_bead";

/// How a bead's span ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BeadEnd {
    /// Cut off square to the edge: the bead's section is the end face.
    #[default]
    Square,
    /// A 45-degree ramp down to the end's point on the edge, as long as the bead is high.
    Taper,
    /// Rounded: the bead shrinks to the end's point on the edge within one bead reach, its
    /// outline on each plate ending in a round.
    Round,
}

/// One bead's frame: where its edge runs, its section at the edge's start, and its sizes.
struct Frame {
    /// The edge's start (the start of its curve's interval).
    start: Point3,
    /// The unit direction along the edge.
    dir: Vec3,
    /// The edge's length.
    length: f64,
    /// The feet's offsets from the edge's point, in the plane square to the edge.
    feet: [Vec3; 2],
    /// The arc between the feet, at the edge's start.
    arc: BeadArc,
    /// The unit direction from the edge into the bead, square to it: to the chord's middle.
    up: Vec3,
    /// How far the bead reaches from the edge (its section's farthest point), and how high it
    /// stands along `up`.
    reach: f64,
    height: f64,
}

/// The bead along the straight edge `edge_id` of `target` between two planes, at `size` and
/// `sagitta`: the first of `size`'s distances runs along the face in `along` where one of the
/// edge's faces is there, else along the edge's first face.
fn frame(
    k: &Kernel,
    target: &mut Brep,
    edge_id: EdgeId,
    size: ChamferSize,
    sagitta: f64,
    along: &[FaceId],
) -> Result<Frame> {
    let curve = target
        .edge(edge_id)?
        .curve
        .clone()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Err(SolidError::invalid_input(
            OP,
            "a bead apart is built along a straight edge only so far",
        ));
    }
    let interval = edge::interval(target, edge_id)?;
    let at = |t: f64| -> Result<Point3> {
        curve
            .evaluate(k, interval.evaluate(t), 0, false, false)?
            .first()
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "the edge's curve gave no point"))
    };
    let start = at(0.0)?;
    let run = at(1.0)? - start;
    let length = run.length();
    let dir = run.unitize()?;
    // the edge's two faces: each one's direction into it from the edge and its normal
    let middle = interval.evaluate(0.5);
    let mut sides: Vec<(FaceId, Vec3, Vec3)> = Vec::with_capacity(2);
    // the edge uses on the void's side (the infinite region's shell): their faces' normals
    // point out of the material, whichever way the faces' surfaces run
    let void = target.infinite_region();
    for eu in edge::edgeuses(target, edge_id, None, None)? {
        let Some(face) = eu1::face(target, eu)? else {
            continue;
        };
        let region = match eu1::shell(target, eu)? {
            Some(shell) => target.shell(shell)?.region,
            None => None,
        };
        if void.is_some() && region != void {
            continue;
        }
        if sides.iter().any(|s| s.0 == face) {
            continue;
        }
        let planar = target
            .face(face)?
            .surface
            .as_ref()
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        if !planar {
            return Err(SolidError::invalid_input(
                OP,
                "a bead apart is built between planes only so far",
            ));
        }
        let eval = evaluate_binormal(k, target, eu, middle, true, &BinormalOptions::default())?;
        sides.push((face, eval.binormal.unitize()?, eval.face_normal.unitize()?));
    }
    let [first, second] = <[(FaceId, Vec3, Vec3); 2]>::try_from(sides)
        .map_err(|_| SolidError::invalid_input(OP, "an edge between two faces"))?;
    let [(_, u1, n1), (_, u2, _)] = if along.contains(&second.0) && !along.contains(&first.0) {
        [second, first]
    } else {
        [first, second]
    };
    // an inside corner: the second face rises on the first one's outer side
    if n1.dot(u2) <= EFF_ZERO {
        return Err(SolidError::invalid_input(
            OP,
            "a bead lies in an inside corner (a concave edge) only",
        ));
    }
    let wedge = u1.cross(u2).length().atan2(u1.dot(u2));
    let bead = BeadDefinition::new(ChamferDefinition::new(size, wedge)?, sagitta)?;
    let [d1, d2] = bead.chamfer.setbacks();
    let feet = [u1 * d1, u2 * d2];
    let up_raw = (feet[0] + feet[1]) * 0.5;
    let up = up_raw.unitize()?;
    let arc = BeadArc::new(start + feet[0], start + feet[1], dir, up_raw, sagitta)?;
    let reach = d1.max(d2).max(arc_extreme(&arc, start, None));
    let height = (feet[0].dot(up))
        .max(feet[1].dot(up))
        .max(arc_extreme(&arc, start, Some(up)));
    Ok(Frame {
        start,
        dir,
        length,
        feet,
        arc,
        up,
        reach,
        height,
    })
}

/// The arc's extreme from `from`: its greatest distance from it (`along` `None`) or its
/// greatest extent along the unit `along`, where that is reached inside the arc (else 0: the
/// feet bound it then). On a circle the distance from a point is greatest along the ray from
/// the point through the centre, the extent along a direction at the centre plus the radius
/// along it; each varies one way between the arc's ends otherwise.
fn arc_extreme(arc: &BeadArc, from: Point3, along: Option<Vec3>) -> f64 {
    let towards = match along {
        Some(d) => d,
        None => match (arc.centre - from).unitize() {
            Ok(d) => d,
            Err(_) => return 0.0,
        },
    };
    let mut angle = towards.dot(arc.y).atan2(towards.dot(arc.x));
    if angle < 0.0 {
        angle += TAU;
    }
    if angle > arc.turn {
        return 0.0;
    }
    match along {
        Some(d) => (arc.centre - from).dot(d) + arc.radius,
        None => (arc.centre - from).length() + arc.radius,
    }
}

impl Frame {
    /// The bead swept from `t0` to `t1` along the edge (distances from its start; either may
    /// lie past the edge's ends).
    fn sweep(&self, k: &mut Kernel, t0: f64, t1: f64) -> Result<Brep> {
        // swept counter-clockwise about the sweep (the feet's turn about the edge)
        let ccw = self.feet[0].cross(self.feet[1]).dot(self.dir) > 0.0;
        let (base, way) = if ccw {
            (t0, self.dir)
        } else {
            (t1, -self.dir)
        };
        let shift = self.dir * base;
        let edge = self.start + shift;
        let f1 = edge + self.feet[0];
        let f2 = edge + self.feet[1];
        let mut placement = Axis2Placement::default();
        placement.set_canonical(self.arc.centre + shift, self.arc.x, self.arc.y)?;
        let arc = BSplineCurve::create_circle_segment(
            3,
            &placement,
            self.arc.radius,
            0.0,
            self.arc.turn.to_degrees(),
            NurbCircleParam::Quadratic,
        )?;
        let curves = [
            Curve::from(create_line_segment(edge, f1)?),
            Curve::from(arc),
            Curve::from(create_line_segment(f2, edge)?),
        ];
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        create_linear_sweep(k, &refs, way, t1 - t0, true)
    }

    /// The box past the plane through `at` square to the unit `normal`, on the side `normal`
    /// points to, `half` wide each way and `half` deep: big enough to hold every part of a
    /// span's body past the plane.
    fn cutter(k: &mut Kernel, at: Point3, normal: Vec3, half: f64) -> Result<Brep> {
        let [u, v] = square_to(normal)?;
        let corners = [
            at + u * half + v * half,
            at - u * half + v * half,
            at - u * half - v * half,
            at + u * half - v * half,
        ];
        let sides = corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .map(|(&a, &b)| create_line_segment(a, b).map(Curve::from))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        create_linear_sweep(k, &refs, normal, half, true)
    }

    /// One span `[t0, t1]` (distances along the edge) with `ends` at both of its ends.
    fn span(&self, k: &mut Kernel, t0: f64, t1: f64, ends: BeadEnd) -> Result<Brep> {
        // the sphere of a rounded end, a little past the bead's reach so it never touches the
        // bead's faces tangentially
        let round = 1.05 * self.reach;
        let need = match ends {
            BeadEnd::Square => 0.0,
            BeadEnd::Taper => 2.0 * self.height,
            BeadEnd::Round => 2.0 * round,
        };
        if t1 - t0 < need {
            return Err(SolidError::invalid_input(
                OP,
                "a span shorter than its two ends need (two bead heights tapered, two bead reaches rounded)",
            ));
        }
        if ends == BeadEnd::Square {
            return self.sweep(k, t0, t1);
        }
        let past = 1.5 * self.reach.max(self.height) + 1.0;
        let mut body = self.sweep(k, t0 - past, t1 + past)?;
        let half = 4.0 * (self.reach + past) + 1.0;
        for (t, out) in [(t0, -self.dir), (t1, self.dir)] {
            let tip = self.start + self.dir * t;
            let cutter = if ends == BeadEnd::Taper {
                let normal = (out + self.up).unitize()?;
                Self::cutter(k, tip, normal, half)?
            } else {
                let centre = tip - out * round;
                let outside = Self::cutter(k, centre, out, half)?;
                let sphere = create_sphere(k, centre, round)?;
                boolean_difference(k, outside, sphere)?
            };
            body = boolean_difference(k, body, cutter)?;
        }
        Ok(body)
    }
}

/// The weld beads along `edges` of `target`, each as a body of its own built from its own
/// faces, in the order of `edges`; `target` is left as it was. Each edge is straight between
/// two planes in an inside corner; `sizes` (one per edge), `sagitta` as
/// [`crate::blend::fillet_executive::session::FilletSession::prepare_beads_along`]'s, and the
/// first distance of a size runs along the face in `along` where one of the edge's faces is.
/// A convex bead overhanging its toe is built: no trimming is involved.
///
/// `target` is mutable only for evaluating its edges' faces (a missing trim curve may be
/// cached); its geometry and topology are left as they were.
///
/// # Errors
///
/// Native invalid input for sizes not one per edge, an edge that is not straight, not between
/// two planes or not in an inside corner, and for a bead past its bound
/// ([`BeadDefinition::new`]); those of the evaluations and the sweeps.
pub fn beads_apart(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[FaceId],
) -> Result<Vec<Brep>> {
    if sizes.len() != edges.len() {
        return Err(SolidError::invalid_input(OP, "one size per edge"));
    }
    edges
        .iter()
        .zip(sizes)
        .map(|(&e, &size)| {
            let f = frame(k, target, e, size, sagitta, along)?;
            f.sweep(k, 0.0, f.length)
        })
        .collect()
}

/// The bead along the straight edge `edge_id` of `target` ([`beads_apart`], with `size`,
/// `sagitta` and `along`) as one body per span of `spans`, in their order (fractions of the
/// edge's length from the start of its curve's interval), each with `ends` at both of its
/// ends. The end shapes lie inside the span, so a span's length stays the weld's length.
///
/// # Errors
///
/// Those of [`beads_apart`]; native invalid input for no span, spans out of order or
/// overlapping ([`BeadSpan`]), and a span too short for its ends (a taper needs the bead's
/// height at each end, a rounded end its reach); those of the cuts.
#[allow(clippy::too_many_arguments)]
pub fn beads_apart_in_spans(
    k: &mut Kernel,
    target: &mut Brep,
    edge_id: EdgeId,
    size: ChamferSize,
    sagitta: f64,
    along: &[FaceId],
    spans: &[BeadSpan],
    ends: BeadEnd,
) -> Result<Vec<Brep>> {
    let spans = BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?;
    let f = frame(k, target, edge_id, size, sagitta, along)?;
    spans
        .iter()
        .map(|s| f.span(k, s.from * f.length, s.to * f.length, ends))
        .collect()
}

/// Two unit vectors square to the unit `dir` and to each other, `u x v = dir`.
fn square_to(dir: Vec3) -> Result<[Vec3; 2]> {
    let pick = if dir.x.abs() <= dir.y.abs() && dir.x.abs() <= dir.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if dir.y.abs() <= dir.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let u = pick.cross(dir).unitize()?;
    let v = dir.cross(u);
    Ok([u, v])
}

/// The length of the bead along `edge_id` of `target` in each of `spans`, in their order:
/// the edge's arc length times the span's share (spans are fractions of that length). For a
/// weld table; a bead's volume is its body's ([`crate::mass::integrator::properties::brep_volume`] of a body from
/// [`beads_apart`] or [`beads_apart_in_spans`]).
///
/// # Errors
///
/// Native invalid input for spans out of order or overlapping ([`BeadSpan`]) or an edge with
/// no curve; those of the edge's arc length.
pub fn span_lengths(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    spans: &[BeadSpan],
) -> Result<Vec<f64>> {
    let spans = BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?;
    let curve = target
        .edge(edge_id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    let length = curve.length(k, &edge::interval(target, edge_id)?, LENGTH_ACCURACY)?;
    Ok(spans.iter().map(|s| length * s.share()).collect())
}

/// The relative accuracy [`span_lengths`] asks of an edge's arc length (a line's and a
/// circle's are exact whatever is asked).
const LENGTH_ACCURACY: f64 = 1e-12;
