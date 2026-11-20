//! Weld beads as bodies of their own (#15129, the welds epic #15132): a bead built from its own
//! faces, apart from the body it is laid on, which is left as it was. No boolean with the
//! body and no copy of it, so the cost does not grow with the body.
//!
//! Between planes along a straight edge the exact bead's body ([`beads_apart`]) is its
//! section swept along the edge: the edge's point, the feet at the setbacks on each face, and
//! the arc between them ([`BeadArc`], the exact circle the blend builds the bead face from).
//! Its faces are the bead face, the strips of the two planes between the edge and the feet,
//! and the two section ends. On any joint the bead is built on the foot blend
//! ([`bead_body`], [`crate::blend::foot_blend`]); a span ([`BeadSpan`]) builds its share of
//! the edge only, so an intermittent weld is one body per span.
//!
//! A span ends square to the edge, or tapers, or is rounded ([`BeadEnd`]); the last two only
//! straight between two planes so far, cut from the span's own small planar body: the span is swept past its end first, so each cut crosses
//! the bead's edges instead of passing through its corners, then
//! - a taper is cut by the plane through the span's end on the edge at 45 degrees to the edge,
//!   rising into the span along the bead's middle direction: a ramp as long as the bead is
//!   high, the strips narrowing to the end's point;
//! - a rounded end is cut by the sphere about the edge one bead reach inside the span's end:
//!   the bead's outline on each plate ends in a round, the section shrinking to the end's
//!   point on the edge.
//!
//! The bead face keeps the generic blend's definition, as the blend's own bead face does: the
//! chord (order 0) of the bead's sizes on the edge's two planes, with its sagitta
//! ([`crate::surface::procedural_blend::exact::ExactRound::with_sagitta`]), re-certified on
//! the cylinder through the feet's lines. So a bead apart is exact and saved as one.
//!
//! The bead never trims or changes the body (#15132).
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::fillet_exact::ClassInputs;
use crate::boolean::api_brep::boolean_difference;
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
    let chamfer = ChamferDefinition::new(size, wedge)?;
    // a sagitta past its bound: refused with the bound, the size that builds
    if let Err(refused) = (BeadDefinition { chamfer, sagitta }).check() {
        return Err(sagitta_refusal(edge_id, refused.bound));
    }
    let bead = BeadDefinition::new(chamfer, sagitta)?;
    let [d1, d2] = bead.chamfer.setbacks();
    let feet = [u1 * d1, u2 * d2];
    let up_raw = (feet[0] + feet[1]) * 0.5;
    let arc = BeadArc::new(start + feet[0], start + feet[1], dir, up_raw, sagitta)?;
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

/// The refusal of a bead whose sagitta is past `bound` along `edge` (the open bound, so the
/// fit is a hair below it).
fn sagitta_refusal(edge: EdgeId, bound: f64) -> SolidError {
    use crate::blend::fillet_executive::NoRoomReason;
    use crate::error::{Fit, FitMeasure};
    let edge = TargetEdge::of_target(edge);
    Failure::new(OP, BlendFailure::NoRoom(NoRoomReason::BeadCutsFace))
        .or_at(Site::edge(edge))
        .with_fit(Fit::new(
            (bound * (1.0 - 1e-9)).max(0.0),
            FitMeasure::Sagitta,
            Some(edge),
        ))
        .into_error()
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

/// Refuses, before anything is built, an edge listed twice in `edges` (native invalid input)
/// and two of `edges` of `target` that meet at a vertex (a bead not built yet, naming the
/// vertex: their corner).
///
/// # Errors
///
/// Those named; those of the edges' vertices.
fn refuse_repeats_and_corners(target: &Brep, edges: &[EdgeId]) -> Result<()> {
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
    Ok(())
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
    refuse_repeats_and_corners(target, edges)?;
    // every frame first, so a refused edge leaves nothing built
    let frames = edges
        .iter()
        .zip(sizes)
        .map(|(&e, &size)| frame(k, target, e, size, sagitta, along))
        .collect::<Result<Vec<_>>>()?;
    frames.iter().map(|f| f.sweep(k, 0.0, f.length)).collect()
}

/// The weld bead along `edges` of `target` as one body of its own, built on the foot blend
/// (#15129, [`crate::blend::foot_blend`]): along each edge, at every station, each toe lies on
/// its face at its leg's distance from the root, measured straight in the plane square to the
/// edge (what a leg gauge measures), on flat and curved faces alike, with the bead's section
/// (the chord, or the arc at the sagitta) between the toes. Edges picked together make one
/// body (a lump per bead or span where they do not touch). The one entry point of the welds:
/// each section face the foot blend builds keeps its definition with the weld's spans and end
/// shapes ([`crate::blend::foot_blend::FootBlendDefinition`]).
///
/// `sizes` are one per edge, as the two legs ([`ChamferSize::Distances`]: support 1's,
/// support 2's), `sagitta` is the arc's height over the chord (`> 0` convex, `< 0` concave,
/// `0` flat), and the first leg runs along the face in `along` where one of the edge's faces
/// is. A convex bead overhanging its toe is built: nothing is trimmed. `spans` are where the
/// bead runs along each edge, fractions of the edge's length from the start of its curve's
/// interval ([`BeadSpan`]; none: the whole edge), and `ends` the shape at each span's start
/// and end, on any joint: square (a planar cap), or tapered or rounded, a leg law over the
/// end's zone inside the span (the section scaled down to the root at the tip:
/// [`crate::blend::foot_blend::FootRamp`]; a taper's zone as long as the bead is high, a
/// rounded end's one bead reach). Nothing is cut: the planar sweep's shaped ends
/// ([`beads_apart_in_spans`]) are another shape, not this one's.
///
/// Beads whose edges meet at a vertex are joined there (#17916): at an inside corner as they
/// are, below 90 degrees each cut at the other's wall first; round an outside corner by the
/// first bead's end section turned to the second's (a wrap-around). See `generic-blend.md`
/// §2.5, Corners.
///
/// A weld never changes the part it lies on: `target`'s geometry and topology are left as
/// they were whether the bead is built or refused (it is mutable for the edges' binormals,
/// asked in 3D only, and for the classification of the toes on their faces, which builds
/// nothing on it).
///
/// # Errors
///
/// Native invalid input for no edge, sizes not one per edge, an edge listed twice, a size
/// that is not two legs, or spans out of order, overlapping or too short for their ends;
/// [`crate::error::blend::BlendFailure::WeldNotBuilt`]
/// naming the vertex where `edges` meet in a corner not built yet (spans or a shaped end
/// there, a third bead at an outside corner, faces not planar where needed, a wall not square
/// to the face both beads lie on, different sections round an outside corner); the foot blend's refusals, typed and with the size that builds (a toe off
/// its face, no toe at the leg, an arc that would cut into a face); those of the closing.
#[allow(clippy::too_many_arguments)]
pub fn bead_body(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[FaceId],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
) -> Result<Brep> {
    use crate::error::blend::{BlendFailure, WeldGap};
    use crate::error::{Failure, Site, TargetVertex};
    if edges.is_empty() || sizes.len() != edges.len() {
        return Err(SolidError::invalid_input(
            OP,
            "a weld bead takes edges and one size per edge",
        ));
    }
    // An edge listed twice would be two beads in one place. Check this before collecting
    // corners: a closed edge has the same vertex at both ends, but is still one bead.
    for (i, e) in edges.iter().enumerate() {
        if edges.get(..i).is_some_and(|before| before.contains(e)) {
            return Err(SolidError::invalid_input(OP, "each edge is listed once"));
        }
    }
    // beads meeting at a vertex: built one by one and joined at their corners
    let mut seen: std::collections::BTreeMap<VertexId, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, &e) in edges.iter().enumerate() {
        let (start, end) = edge::start_end_vertices(target, e)?;
        let mut ends_of = vec![start, end];
        ends_of.dedup();
        for v in ends_of.into_iter().flatten() {
            seen.entry(v).or_default().push(i);
        }
    }
    seen.retain(|_, at| at.len() > 1);
    if !seen.is_empty() {
        let refuse = |v: VertexId| {
            Failure::new(OP, BlendFailure::WeldNotBuilt(WeldGap::BeadsMeetAtVertex))
                .or_at(Site::vertex(TargetVertex::of_target(v)))
                .into_error()
        };
        return corner_beads(
            k, target, edges, sizes, sagitta, along, spans, ends, &seen, &refuse,
        );
    }
    beads_together(k, target, edges, sizes, sagitta, along, spans, ends, &[])
}

/// The beads along `edges` in one body, none of them meeting at a vertex ([`bead_body`]);
/// `behind` per edge the planes (a point, the unit normal into a neighbour's wall) behind
/// which its toes may leave their faces: it is cut there afterwards.
///
/// # Errors
///
/// Those of [`bead_body`].
#[allow(clippy::too_many_arguments)]
fn beads_together(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[FaceId],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
    behind: &[Vec<(Point3, Vec3)>],
) -> Result<Brep> {
    use crate::blend::foot_blend::{close_bead_body, BeadWeld, FootBlend, WELD_FIT_TOLERANCE};
    let spans = if spans.is_empty() {
        vec![BeadSpan::new(0.0, 1.0)?]
    } else {
        BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?
    };
    let tol = WELD_FIT_TOLERANCE;
    let mut body = Brep::new_with_infinite_region()?;
    body.set_editing_enabled(true);
    let mut defined = Vec::new();
    for (&e, size) in edges.iter().zip(sizes) {
        let ChamferSize::Distances(legs) = *size else {
            return Err(SolidError::invalid_input(
                OP,
                "a weld bead's size is its two legs",
            ));
        };
        let first = edge_faces(target, e)?
            .into_iter()
            .find(|f| along.contains(f));
        let blend = FootBlend::of_edge(k, target, e, first)?;
        let planes = behind.get(defined.len()).map_or(&[][..], Vec::as_slice);
        let reach = 1e-9 * (1.0 + legs[0].max(legs[1]));
        blend.toes_on_faces_except(k, target, legs, &|p: Point3| {
            planes.iter().any(|&(o, n)| (p - o).dot(n) > -reach)
        })?;
        let params = span_parameters(k, &blend, &spans)?;
        let mut made = Vec::new();
        for &p in &params {
            made.extend(blend.add_span(k, &mut body, legs, sagitta, tol, p, ends)?);
        }
        let weld = BeadWeld {
            legs,
            sagitta,
            spans: params,
            ends,
            tol,
        };
        defined.push((blend, made, weld));
    }
    close_bead_body(k, &mut body, tol)?;
    for (blend, made, weld) in &defined {
        blend.define_faces(&mut body, made, weld)?;
    }
    Ok(body)
}

/// `spans` (fractions of the joint's length from the start of its interval) as the foot
/// blend's leader parameters, by arc length; the ends of the interval exactly.
///
/// # Errors
///
/// A failure where a parameter at a length is not found; those of the lengths.
fn span_parameters(
    k: &Kernel,
    blend: &crate::blend::foot_blend::FootBlend,
    spans: &[BeadSpan],
) -> Result<Vec<[f64; 2]>> {
    let (lo, hi) = (blend.interval.min(), blend.interval.max());
    let curve = Curve::from(blend.leader.clone());
    let length = curve.length(k, &blend.interval, LENGTH_ACCURACY)?;
    let at = |share: f64| -> Result<f64> {
        if share <= 0.0 {
            return Ok(lo);
        }
        if share >= 1.0 {
            return Ok(hi);
        }
        let found = curve.find_parameter_at_arc_length(k, lo, share * length)?;
        if !found.converged || found.past_end {
            return Err(SolidError::blend(OP, BlendFailure::StageFailed));
        }
        Ok(found.end_param.clamp(lo, hi))
    };
    spans.iter().map(|s| Ok([at(s.from)?, at(s.to)?])).collect()
}

/// The faces on `edge_id` of `target`, through its edge uses.
///
/// # Errors
///
/// Those of the edge uses.
fn edge_faces(target: &Brep, edge_id: EdgeId) -> Result<Vec<FaceId>> {
    let mut faces = Vec::new();
    for eu in edge::edgeuses(target, edge_id, None, None)? {
        if let Some(f) = crate::topology::edgeuse::p1::face(target, eu)? {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
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
/// [`beads_apart`] or [`bead_body`]).
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

/// A bead's end at a corner: its foot blend, the leader's parameter there, the station there,
/// and the leader's unit direction away from the vertex.
struct CornerEnd {
    blend: crate::blend::foot_blend::FootBlend,
    t: f64,
    station: crate::blend::foot_blend::FootStation,
    away: Vec3,
}

impl CornerEnd {
    /// Which support of this bead is on `face`.
    fn side_on(&self, face: FaceId) -> Option<usize> {
        self.blend.sides.iter().position(|s| s.face == Some(face))
    }

    /// The unit direction from the root to the foot on support `i`.
    fn toward(&self, i: usize) -> Result<Vec3> {
        let foot = self.station.feet.get(i).copied().ok_or_else(stage_failed)?;
        (foot - self.station.root).unitize()
    }

    /// Whether support `i` is a plane.
    fn planar(&self, i: usize) -> bool {
        self.blend
            .sides
            .get(i)
            .is_some_and(|s| matches!(s.surface.kind(), SurfaceKind::Plane(_)))
    }
}

fn stage_failed() -> SolidError {
    SolidError::blend(OP, BlendFailure::StageFailed)
}

/// The beads along `edges` where some meet at the vertices of `corners` (each with the
/// indices of its edges): each built alone, an inside corner's beads joined as they are
/// (below 90° each cut at the other's wall first, its toes behind that wall excused), an
/// outside corner closed by the first bead's end section turned about the face both lie on,
/// to the second's (a wrap-around). Everything else at a corner is refused by `refuse`.
///
/// # Errors
///
/// `refuse`'s for a corner not built yet; those of [`bead_body`] and of the booleans.
#[allow(clippy::too_many_arguments)]
fn corner_beads(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[FaceId],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
    corners: &std::collections::BTreeMap<VertexId, Vec<usize>>,
    refuse: &dyn Fn(VertexId) -> SolidError,
) -> Result<Brep> {
    use crate::blend::foot_blend::FootBlend;
    let mut behind: Vec<Vec<(Point3, Vec3)>> = vec![Vec::new(); edges.len()];
    let mut cuts: Vec<Vec<(Point3, Vec3, f64)>> = vec![Vec::new(); edges.len()];
    let mut pieces: Vec<Brep> = Vec::new();
    for (&v, at) in corners {
        let mut wraps = Vec::new();
        if !spans.is_empty() {
            return Err(refuse(v));
        }
        // Each bead's end at this topological vertex. Do not infer it from distances between
        // bead ends: two edges can share both vertices (a D-shaped loop), and then proximity
        // does not identify which common vertex this corner is for.
        let vertex = target.vertex(v)?.point;
        let mut tips = Vec::new();
        for &i in at {
            let (Some(&e), Some(size)) = (edges.get(i), sizes.get(i)) else {
                return Err(stage_failed());
            };
            let ChamferSize::Distances(legs) = *size else {
                return Err(SolidError::invalid_input(
                    OP,
                    "a weld bead's size is its two legs",
                ));
            };
            let first = edge_faces(target, e)?
                .into_iter()
                .find(|f| along.contains(f));
            let blend = FootBlend::of_edge(k, target, e, first)?;
            let (lo, hi) = (blend.interval.min(), blend.interval.max());
            let (start, end) = edge::start_end_vertices(target, e)?;
            let at_start = match (start == Some(v), end == Some(v)) {
                (true, false) => true,
                (false, true) => false,
                // A closed edge has both ends at this vertex. It contributes two bead ends,
                // which this corner construction does not represent yet.
                _ => return Err(refuse(v)),
            };
            tips.push((blend, legs, lo, hi, at_start));
        }
        let mut corner = Vec::new();
        for (blend, legs, lo, hi, at_start) in tips {
            let t = if at_start { lo } else { hi };
            // a square end only: ends[0] at the start of the span, ends[1] at its end
            if !matches!(if at_start { ends[0] } else { ends[1] }, BeadEnd::Square) {
                return Err(refuse(v));
            }
            let station = blend.station(k, t, legs, None)?;
            let away = if at_start {
                station.tangent
            } else {
                -station.tangent
            };
            corner.push((
                CornerEnd {
                    blend,
                    t,
                    station,
                    away,
                },
                legs,
            ));
        }
        let pairs: Vec<(usize, usize)> = (0..corner.len())
            .flat_map(|a| ((a + 1)..corner.len()).map(move |b| (a, b)))
            .collect();
        for (a, b) in pairs {
            let (Some((ea, legs_a)), Some((eb, _))) = (corner.get(a), corner.get(b)) else {
                return Err(stage_failed());
            };
            // the face both beads lie on, and each bead's other face (its wall)
            let faces_a: Vec<FaceId> = ea.blend.sides.iter().filter_map(|s| s.face).collect();
            let common: Vec<FaceId> = eb
                .blend
                .sides
                .iter()
                .filter_map(|s| s.face)
                .filter(|f| faces_a.contains(f))
                .collect();
            let [face] = common[..] else {
                return Err(refuse(v));
            };
            let (Some(ja), Some(jb)) = (ea.side_on(face), eb.side_on(face)) else {
                return Err(refuse(v));
            };
            let (wa, wb) = (1 - ja, 1 - jb);
            let (ua, ub) = (ea.toward(ja)?, eb.toward(jb)?);
            let (wa_dir, wb_dir) = (ea.toward(wa)?, eb.toward(wb)?);
            let eps = 1e-9;
            let tangent = ea.away.cross(eb.away).length() <= EFF_ZERO_RAD;
            if tangent {
                // A smooth line-to-arc join, or two collinear edge pieces, already shares
                // one square end. Join the bead bodies as they are; a zero-angle outside
                // revolution is not a corner piece. Coincident directions mean overlapping
                // edges rather than a smooth continuation and remain unsupported.
                if ea.away.dot(eb.away) < 0.0 {
                    continue;
                }
                return Err(refuse(v));
            }
            let inside_ab = eb.away.dot(ua) > eps;
            let inside_ba = ea.away.dot(ub) > eps;
            if inside_ab != inside_ba {
                return Err(refuse(v));
            }
            let (ia, ib) = (at.get(a).copied(), at.get(b).copied());
            let (Some(ia), Some(ib)) = (ia, ib) else {
                return Err(stage_failed());
            };
            if inside_ab {
                // below 90 degrees each bead runs into the other's wall: cut it there
                if ea.away.dot(eb.away) > eps {
                    if !(ea.planar(wa) && eb.planar(wb)) {
                        return Err(refuse(v));
                    }
                    let reach = 4.0 * legs_a[0].max(legs_a[1])
                        / (1.0 - ea.away.dot(eb.away).powi(2)).sqrt().max(1e-3);
                    for (me, other, w, u) in [(ia, eb, wb_dir, ub), (ib, ea, wa_dir, ua)] {
                        let mut n = other.away.cross(w).unitize()?;
                        if n.dot(u) > 0.0 {
                            n = -n;
                        }
                        if let (Some(bh), Some(c)) = (behind.get_mut(me), cuts.get_mut(me)) {
                            bh.push((vertex, n));
                            c.push((vertex, n, reach));
                        }
                    }
                }
            } else {
                // an outside corner: two beads only, walls square to the face, one section
                if corner.len() != 2 || !ea.planar(ja) {
                    return Err(refuse(v));
                }
                let axis = ea.away.cross(ua).unitize()?;
                if wa_dir.dot(axis).abs() < 1.0 - 1e-9 || wb_dir.dot(axis).abs() < 1.0 - 1e-9 {
                    return Err(refuse(v));
                }
                let angle = ua.dot(ub).clamp(-1.0, 1.0).acos();
                let turn = |ax: Vec3, u: Vec3| u * angle.cos() + ax.cross(u) * angle.sin();
                let fit = |ax: Vec3| turn(ax, ua).dot(ub) > 1.0 - 1e-9;
                let ax = if fit(axis) {
                    axis
                } else if fit(-axis) {
                    -axis
                } else {
                    return Err(refuse(v));
                };
                // the turned section must be the second bead's
                let (sa, sb) = (&ea.station, &eb.station);
                let gap = |i: usize, j: usize| -> Option<f64> {
                    let fa = sa.feet.get(i)?;
                    let fb = sb.feet.get(j)?;
                    let d = *fa - sa.root;
                    let along = ax * ax.dot(d);
                    let turned = sa.root + along + turn(ax, d - along);
                    Some((turned - *fb).length())
                };
                let scale = 1e-7 * (1.0 + legs_a[0].max(legs_a[1]));
                if !(gap(ja, jb).is_some_and(|g| g <= scale)
                    && gap(wa, wb).is_some_and(|g| g <= scale))
                {
                    return Err(refuse(v));
                }
                wraps.push((a, ax, angle));
            }
        }
        // the outside corners' pieces: the first bead's end section turned to the second's
        for (a, ax, angle) in wraps {
            let Some((ea, legs)) = corner.get(a) else {
                return Err(stage_failed());
            };
            let (_, _, section) = ea.blend.cut_curves(k, ea.t, *legs, sagitta)?;
            let s = &ea.station;
            let [f0, f1] = s.feet;
            let r0 = Curve::from(create_line_segment(s.root, f0)?);
            let r1 = Curve::from(create_line_segment(f1, s.root)?);
            let arc = Curve::from(section);
            pieces.push(crate::sweep::api_brep::create_rotational_sweep(
                k,
                &[Some(&r0), Some(&arc), Some(&r1)],
                s.root,
                ax,
                angle.to_degrees(),
                true,
            )?);
        }
    }
    // each bead alone, cut at the walls it runs into, then all joined
    let mut joined: Option<Brep> = None;
    for (i, (&e, size)) in edges.iter().zip(sizes).enumerate() {
        let planes = behind.get(i).cloned().unwrap_or_default();
        let mut bead = beads_together(
            k,
            target,
            &[e],
            core::slice::from_ref(size),
            sagitta,
            along,
            &[],
            ends,
            &[planes],
        )?;
        for &(o, n, reach) in cuts.get(i).map_or(&[][..], Vec::as_slice) {
            let block = wall_block(k, o, n, reach)?;
            bead = boolean_difference(k, bead, block)?;
        }
        joined = Some(match joined {
            None => bead,
            Some(body) => crate::boolean::api_brep::boolean_union(k, body, bead)?,
        });
    }
    let mut body = joined.ok_or_else(stage_failed)?;
    for piece in pieces {
        body = crate::boolean::api_brep::boolean_union(k, body, piece)?;
    }
    Ok(body)
}

/// A block behind the plane through `o` with unit normal `n` (into the wall): a square of
/// side `4 reach` in the plane, centred on `o`, swept `reach` along `n`.
///
/// # Errors
///
/// Those of the frame and the sweep.
fn wall_block(k: &mut Kernel, o: Point3, n: Vec3, reach: f64) -> Result<Brep> {
    let [x, y] = square_to(n)?;
    let h = 2.0 * reach;
    let corners = [
        o - x * h - y * h,
        o + x * h - y * h,
        o + x * h + y * h,
        o - x * h + y * h,
    ];
    let mut sides = Vec::with_capacity(4);
    for (i, &p) in corners.iter().enumerate() {
        let q = corners.get((i + 1) % 4).copied().ok_or_else(stage_failed)?;
        sides.push(Curve::from(create_line_segment(p, q)?));
    }
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, n, reach, true)
}
