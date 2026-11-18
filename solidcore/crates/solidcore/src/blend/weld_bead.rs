//! Weld beads as bodies of their own (#15129, the welds epic #15132): a bead built from its own
//! faces, apart from the body it is laid on, which is left as it was. No boolean with the
//! body and no copy of it, so the cost does not grow with the body.
//!
//! This is the exact bead: between two planes along a straight edge, the bead's body is its
//! section swept along the edge: the edge's point, the feet at the setbacks on each face, and
//! the arc between them ([`BeadArc`], the exact circle the bead face is built from). Its faces
//! are the bead face, the strips of the two planes between the edge and the feet, and the two
//! section ends, square to the edge.
//!
//! The bead face keeps the generic blend's definition: the chord (order 0) of the bead's sizes
//! on the edge's two planes, with its sagitta
//! ([`crate::surface::procedural_blend::exact::ExactRound::with_sagitta`]), re-certified on the
//! cylinder through the feet's lines: exact, bound 0. The definition is the kernel's only; no
//! record writes a sagitta yet
//! ([`crate::surface::procedural_blend::convert::exact_to_record`] refuses one).
//!
//! Refused before anything is built, the part left as it was, as
//! [`BlendFailure::WeldNotBuilt`] (class `Unsupported`) naming the edge or vertex: a joint
//! that is not a straight edge between two planes ([`WeldGap::CurvedJoint`]: the bead on any
//! joint, the foot blend, comes with its certificate), beads that meet at a vertex
//! ([`WeldGap::BeadsMeetAtVertex`]: their corner) and a bead with an obtuse toe
//! ([`WeldGap::ObtuseToe`]). As invalid input: an edge that is not an inside corner (an
//! outside or a tangent edge), an edge listed twice, sizes not one per edge.
//!
//! The bead never trims or changes the body (#15132).
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::fillet_exact::ClassInputs;
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::blend::{BlendFailure, WeldGap};
use crate::error::{Failure, Result, Site, SolidError, TargetEdge, TargetVertex};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_RAD;
use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use crate::nurbs::knots::Side;
use crate::primitive::api_curves::create_line_segment;
use crate::surface::procedural_blend::bead::{BeadArc, BeadDefinition, BeadSpan};
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::surface::procedural_blend::exact::ExactRound;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::weld_bead";

/// The refusal of a bead the kernel does not build yet, for `gap`, at `site`.
#[track_caller]
fn not_built(gap: WeldGap, site: Site) -> SolidError {
    Failure::new(OP, BlendFailure::WeldNotBuilt(gap))
        .or_at(site)
        .into_error()
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
    /// The bead's sizes and sagitta.
    bead: BeadDefinition,
    /// The edge's two planes, the first the one `size`'s first distance runs along.
    supports: [Surface; 2],
    /// What the chord's class is read with: the planes at the edge, the ball's centre and its
    /// signed offsets.
    inputs: ClassInputs,
}

/// The bead along the straight edge `edge_id` of `target` between two planes, at `size` and
/// `sagitta`: the first of `size`'s distances runs along the face in `along` where one of the
/// edge's faces is there, else along the edge's first face.
///
/// # Errors
///
/// [`WeldGap::CurvedJoint`] naming the edge for an edge that is not straight or a face on it
/// that is not a plane; [`WeldGap::ObtuseToe`] naming the edge for legs whose toe is obtuse;
/// native invalid input for an edge with no curve, not between two faces, or not an inside
/// corner; those of the evaluations and of [`BeadDefinition::new`].
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
    let at_edge = Site::edge(TargetEdge::of_target(edge_id));
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Err(not_built(WeldGap::CurvedJoint, at_edge));
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
    let mut sides: Vec<(FaceId, Vec3, Vec3, Surface, crate::geom::vector2d::Point2)> =
        Vec::with_capacity(2);
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
            return Err(not_built(WeldGap::CurvedJoint, at_edge));
        }
        let eval = evaluate_binormal(k, target, eu, middle, true, &BinormalOptions::default())?;
        let surface = target
            .face(face)?
            .surface
            .clone()
            .ok_or_else(|| SolidError::invalid_input(OP, "a face with no surface"))?;
        sides.push((
            face,
            eval.binormal.unitize()?,
            eval.face_normal.unitize()?,
            surface,
            eval.uv,
        ));
    }
    let [first, second] = <[_; 2]>::try_from(sides)
        .map_err(|_| SolidError::invalid_input(OP, "an edge between two faces"))?;
    let [(_, u1, n1, s1, uv1), (_, u2, _, s2, uv2)] =
        if along.contains(&second.0) && !along.contains(&first.0) {
            [second, first]
        } else {
            [first, second]
        };
    // an inside corner: the second face rises on the first one's outer side. `n1 . u2` is the
    // sine of the second face's rise off the first one's plane (both unit), an angle: at or
    // below the angle zero the edge is an outside corner (negative) or tangent (zero), where
    // the chord would run through the part or through the edge
    if n1.dot(u2) <= EFF_ZERO_RAD {
        return Err(SolidError::invalid_input(
            OP,
            "a weld bead lies in an inside corner (a concave edge) only, not on an outside or a tangent edge",
        ));
    }
    let wedge = u1.cross(u2).length().atan2(u1.dot(u2));
    let bead = BeadDefinition::new(ChamferDefinition::new(size, wedge)?, sagitta)?;
    let [d1, d2] = bead.chamfer.setbacks();
    let feet = [u1 * d1, u2 * d2];
    let up = (feet[0] + feet[1]) * 0.5;
    let arc = BeadArc::new(start + feet[0], start + feet[1], dir, up, sagitta)?;
    // the ball's centre at the edge's start: its offsets from the planes along the normals
    // into the wedge, its feet the bead's (the chamfer's offsets, positive). Legs with an
    // obtuse toe fold the chamfer's ball (its offsets are not both positive), and the
    // cylinder's side is read from that ball, so they are refused here
    // (`ChamferDefinition::offsets` refuses nothing else)
    let Ok(offsets) = bead.chamfer.offsets() else {
        return Err(not_built(WeldGap::ObtuseToe, at_edge));
    };
    let into = (u2 - u1 * u1.dot(u2)).unitize()?;
    let centre = start + feet[0] + into * offsets[0];
    // signed along each plane's own normal, as the blend reads them
    let mut signed = [0.0; 2];
    for ((o, (surface, uv)), (foot, r)) in signed
        .iter_mut()
        .zip([(&s1, uv1), (&s2, uv2)])
        .zip(feet.iter().zip(offsets))
    {
        let normal = surface
            .evaluate_normal(k, uv, Side::Left, Side::Left)?
            .unitize()?;
        *o = (centre - (start + *foot)).dot(normal).signum() * r;
    }
    let inputs = ClassInputs {
        uv: [uv1, uv2],
        offsets: signed,
        centre,
        tol: edge::tolerance(target, edge_id)?,
        ball_on_collapsed: false,
        radii_can_change: false,
    };
    Ok(Frame {
        start,
        dir,
        length,
        feet,
        arc,
        bead,
        supports: [s1, s2],
        inputs,
    })
}

impl Frame {
    /// The bead swept from `t0` to `t1` along the edge (distances from its start; either may
    /// lie past the edge's ends).
    fn sweep(&self, k: &mut Kernel, t0: f64, t1: f64) -> Result<Brep> {
        // swept counter-clockwise about the sweep (the feet's turn about the edge)
        let ccw = self.feet[0].cross(self.feet[1]).dot(self.dir) > 0.0;
        let (base, way) = if ccw { (t0, self.dir) } else { (t1, -self.dir) };
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
        let mut body = create_linear_sweep(k, &refs, way, t1 - t0, true)?;
        self.define_bead_face(k, &mut body)?;
        Ok(body)
    }

    /// Gives the swept body's bead face (its one face that is not a plane) the chord's
    /// definition with the bead's sagitta, re-certified on it.
    ///
    /// # Errors
    ///
    /// Native invalid input for a body with no such face or more than one; those of the
    /// definition and its re-certification.
    fn define_bead_face(&self, k: &Kernel, body: &mut Brep) -> Result<()> {
        let curved: Vec<FaceId> = body
            .face_list()
            .iter()
            .copied()
            .filter(|&f| {
                body.face(f)
                    .ok()
                    .and_then(|face| face.surface.as_ref())
                    .is_some_and(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
            })
            .collect();
        let [face] = curved.as_slice() else {
            return Err(SolidError::invalid_input(
                OP,
                "a bead body has one bead face beside its planes",
            ));
        };
        let [a, b] = &self.supports;
        let round = ExactRound::chord(k, [a, b], self.bead.chamfer, None, self.inputs)?
            .with_sagitta(self.bead.sagitta)?;
        let surface = body
            .face_mut(*face)?
            .surface
            .as_mut()
            .ok_or_else(|| SolidError::invalid_input(OP, "a bead face with no surface"))?;
        round.recertify(k, surface)?;
        surface.set_exact_round(Some(round));
        Ok(())
    }
}

/// The weld beads along `edges` of `target`, each as a body of its own built from its own
/// faces, in the order of `edges`; `target` is left as it was, whether the beads are built or
/// refused. Each edge is straight between two planes in an inside corner; `sizes` are one per
/// edge (the chamfer's setbacks, the bead's toes), `sagitta` is the arc's height over the
/// chord (`> 0` convex, `< 0` concave, `0` flat, [`BeadDefinition`]), and the first distance of
/// a size runs along the face in `along` where one of the edge's faces is. A convex bead
/// overhanging its toe is built: no trimming is involved.
///
/// `target` is mutable only for evaluating its edges' faces (a missing trim curve may be
/// cached); its geometry and topology are left as they were.
///
/// # Errors
///
/// Native invalid input for sizes not one per edge, an edge listed twice, an edge not in an
/// inside corner (an outside or a tangent edge), and a bead past its bound
/// ([`BeadDefinition::new`]); [`BlendFailure::WeldNotBuilt`] for two of `edges` that meet at a
/// vertex ([`WeldGap::BeadsMeetAtVertex`], naming it), an edge that is not straight between
/// two planes ([`WeldGap::CurvedJoint`]) or legs with an obtuse toe ([`WeldGap::ObtuseToe`]),
/// naming the edge; those of the evaluations and the sweeps. Every refusal comes before any
/// bead is built.
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
    // an edge listed twice would be two beads in one place
    for (i, e) in edges.iter().enumerate() {
        if edges.get(..i).is_some_and(|before| before.contains(e)) {
            return Err(SolidError::invalid_input(OP, "each edge is listed once"));
        }
    }
    // beads meeting at a vertex need a corner piece
    let mut seen: std::collections::BTreeMap<VertexId, EdgeId> = std::collections::BTreeMap::new();
    for &e in edges {
        let (start, end) = edge::start_end_vertices(target, e)?;
        for v in [start, end].into_iter().flatten() {
            if seen.insert(v, e).is_some_and(|other| other != e) {
                return Err(not_built(
                    WeldGap::BeadsMeetAtVertex,
                    Site::vertex(TargetVertex::of_target(v)),
                ));
            }
        }
    }
    // every frame first, so a refused edge leaves nothing built
    let frames = edges
        .iter()
        .zip(sizes)
        .map(|(&e, &size)| frame(k, target, e, size, sagitta, along))
        .collect::<Result<Vec<_>>>()?;
    frames.iter().map(|f| f.sweep(k, 0.0, f.length)).collect()
}

/// The length of the bead along `edge_id` of `target` in each of `spans`, in their order:
/// the edge's arc length times the span's share (spans are fractions of that length). For a
/// weld table; a bead's volume is its body's ([`crate::mass::integrator::properties::brep_volume`] of a body from
/// [`beads_apart`]).
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
