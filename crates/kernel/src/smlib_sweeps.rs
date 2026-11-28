//! Extrusions, revolutions, sweeps, lofts and screw sweeps through SMLib: the profile's
//! segments go to SMLib as 3D curves (lines, circular arcs, Béziers and conics as NURBS), SMLib
//! sweeps them, and the result comes back through its schema 4 record as a kernel body. A result
//! SMLib's own check refuses is an error, never a body. An option SMLib has no sweep for is "not
//! supported yet".
//!
//! A sweep that twists, scales, keeps an orientation direction or follows guide rails, and a
//! loft that follows guide rails, place the profile at stations by the laws the kernel's own
//! sweeps and lofts define ([`crate::sweep_path`], [`crate::loft_rails`]); each section is the
//! profile's exact image, and SMLib skins the walls through them.
//!
//! History is in the builders' vocabulary ([`FaceOrigin::Profile`], [`FaceOrigin::Cap`]), found
//! from the result's geometry ([`crate::provenance`] does the same for operations without exact
//! history): a wall comes from the profile segments whose curves its edge at the start runs
//! along (a wall SMLib makes over several tangent segments comes from all of them), and from
//! those whose middles it holds half-way along the sweep where that is known (extrusions,
//! revolutions); the face in the start plane is the start cap, the one in the end plane (or,
//! where the sweep has no end plane, the planar face left over) the end cap.

use solidcore_record::{Plane, Region2, Seg2, Vec3};
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::numbered::{NumberedLoop, NumberedRegion};
use crate::{KernelError, Result};

/// A profile segment as SMLib gets it, with the profile segments it stands for.
#[derive(Clone)]
pub(crate) struct Piece {
    pub seg: Seg2,
    pub numbers: Vec<usize>,
    /// The segment as drawn that names it: itself, or for collinear lines joined into one, the
    /// first of them.
    pub drawn: Seg2,
    /// It carries on the piece before it as one curve (pieces of one ellipse or spline): SMLib
    /// gets the run as one B-spline and sweeps one wall over it.
    pub joins_previous: bool,
}

/// The segments of `loops` as pieces, loop after loop.
pub(crate) fn pieces<'a>(loops: impl IntoIterator<Item = &'a NumberedLoop>) -> Vec<Piece> {
    loops
        .into_iter()
        .flat_map(|l| l.lp.segs.iter().enumerate().map(|(i, s)| Piece { seg: *s, numbers: l.numbers(i).to_vec(), drawn: *s, joins_previous: false }))
        .collect()
}

/// The pieces of `joined` (`drawn` with collinear lines joined): outer loop, then holes, each
/// joined line named by the first line drawn under it.
pub(crate) fn region_pieces(joined: &NumberedRegion, drawn: &NumberedRegion) -> Vec<Piece> {
    let firsts: Vec<Seg2> = drawn.loops().flat_map(|l| l.lp.segs.iter().copied()).collect();
    pieces(joined.loops())
        .into_iter()
        .map(|mut x| {
            if x.numbers.len() > 1
                && let Some(first) = firsts.iter().find(|d| d.start() == x.seg.start())
            {
                x.drawn = *first;
            }
            x
        })
        .collect()
}

fn not_supported(what: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {what}"))
}

/// How an extrusion's walls lean.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Draft {
    /// Radians; positive grows the profile (as [`crate::extrude_tapered`] takes it).
    pub taper: f64,
    /// A height (less than the distance) the walls first stand straight for, before they slope
    /// on to the same far end ([`crate::extrude_tapered_between`]'s foot).
    pub foot: f64,
    /// How the sharp corners close.
    pub corner: crate::DraftCorner,
}

#[cfg(test)]
pub(crate) use imp::loft_piece_by_piece;
pub(crate) use imp::{
    body_from_record, curve, curve_tolerance, cylinder, extrude, extrude_drafted_with_holes, face_section, loft, loft_curves, loft_many_rails,
    loft_to_point, loft_to_point_with_rails, path_curve, project_onto_face, rail_curves, railed_skin, revolve, screw, screw_radial, skin_sections,
    sphere, sweep, sweep_controlled, torus,
};
#[cfg(feature = "backend-occt")]
pub(crate) use imp::{
    cylinder_circle, extrusion_curves, loft_curves_of, name_cylinder, name_extrusion, name_loft, name_loft_to_point, name_revolution,
    name_revolved_primitive, revolution_curves, section_curves,
};

/// `parts` joined in order in a balanced tree: each part with its neighbour, then each pair with
/// the next pair, and so on, a part left over at the end of a level carried up as it is. Every
/// join is of two neighbouring stretches, the left one first, and every part takes part in about
/// log2(n) joins; joining each part onto all those before it instead hands the growing whole to
/// every join, n²/2 parts' worth of work in all. None for no parts.
fn union_tree<T>(parts: Vec<T>, mut join: impl FnMut(T, T) -> Result<T>) -> Result<Option<T>> {
    let mut level = parts;
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            next.push(match it.next() {
                Some(b) => join(a, b)?,
                None => a,
            });
        }
        level = next;
    }
    Ok(level.pop())
}

#[cfg(test)]
thread_local! {
    /// The runs' pieces the unions joining a cornered sweep's runs have taken in on this thread,
    /// both operands counted: how the joining's work grows with the number of corners.
    pub(crate) static UNION_LOAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Refuses what no SMLib sweep does, before any work.
pub(crate) fn refuse(what: &str) -> KernelError {
    not_supported(&format!("SMLib {what}"))
}

mod imp {
    use std::collections::BTreeSet;

    use solidcore_ops::sweep::Curve as SkinCurve;
    use solidcore_ops::sweep::railed::{RailFit as SkinFit, RailLoft, SpaceRailSection as SkinSection};
    use solidcraft_smlib::Brep;
    use solidcraft_smlib::sweeps::{Affine, Curve, DraftCorner, Ends, SweepFrame};

    use solidcore_record::{Loop2, Vec2};

    use super::*;
    use crate::history::FaceProv;
    use crate::record;

    fn failed(what: &str, e: impl crate::Cause) -> KernelError {
        KernelError::smlib(format!("SMLib {what}"), e)
    }

    fn corner(c: crate::DraftCorner) -> DraftCorner {
        match c {
            crate::DraftCorner::PreserveCurvature => DraftCorner::Curvature,
            crate::DraftCorner::Tangent => DraftCorner::Tangent,
            crate::DraftCorner::Round => DraftCorner::Round,
        }
    }

    fn p(v: Vec3) -> [f64; 3] {
        [v.x, v.y, v.z]
    }

    /// The segment in 3D on `plane`, as SMLib's curve.
    pub(crate) fn curve(plane: &Plane, s: &Seg2) -> Result<Curve> {
        let w = |q: Vec2| p(plane.to_world(q));
        let n = plane.x.cross(plane.y).normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))?;
        let bezier = |pts: &[Vec2], weights: &[f64]| {
            let degree = pts.len().saturating_sub(1);
            let points = pts.iter().zip(weights).map(|(q, wt)| {
                let [x, y, z] = w(*q);
                [x, y, z, *wt]
            });
            let knots = std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect();
            Curve::Nurbs { degree, points: points.collect(), knots }
        };
        Ok(match *s {
            Seg2::Line { a, b } => Curve::Line { start: w(a), end: w(b) },
            Seg2::Arc { center, radius, start, sweep } => {
                // A full circle re-split for a loft may come out a rounding over one turn.
                if !(sweep.abs() <= std::f64::consts::TAU * (1.0 + 1e-9)) {
                    return Err(KernelError::Invalid("an arc turns more than once".into()));
                }
                let sweep = sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                // Clockwise in the plane is counter-clockwise about the other normal, where a
                // plane angle t is the angle -t.
                let (axis, a0) = if sweep >= 0.0 { (n, start) } else { (-n, -start) };
                Curve::Arc { center: w(center), axis: p(axis), reference: p(plane.x), radius, start: a0, end: a0 + sweep.abs() }
            }
            Seg2::Cubic { p0, p1, p2, p3 } => bezier(&[p0, p1, p2, p3], &[1.0; 4]),
            Seg2::Conic { a, apex, b, w: wt } => bezier(&[a, apex, b], &[1.0, wt, 1.0]),
            Seg2::Bezier { n: degree, p: pts } => {
                let pts = pts.get(..=usize::from(degree)).ok_or_else(|| KernelError::Invalid("Bézier degree".into()))?;
                bezier(pts, &vec![1.0; pts.len()])
            }
        })
    }

    /// The pieces as SMLib's curves: one each, but one B-spline for each run of pieces that join
    /// the one before them ([`Piece::joins_previous`]). A Bézier piece whose control points turn
    /// back on themselves goes in as its halves ([`unfolded`]): the same curve.
    fn curves(plane: &Plane, pieces: &[Piece]) -> Result<Vec<Curve>> {
        let mut out = Vec::new();
        let mut run = Vec::new();
        for x in pieces {
            if !x.joins_previous && !run.is_empty() {
                out.push(joined(std::mem::take(&mut run))?);
            }
            run.extend(unfolded(curve(plane, &x.seg)?, UNFOLD_DEPTH));
        }
        if !run.is_empty() {
            out.push(joined(run)?);
        }
        Ok(out)
    }

    /// How sharply a Bézier piece's control polygon may turn back between neighbouring steps
    /// before the piece goes in as its halves (cosine of the turn, 160°). SMLib's check refuses
    /// a swept wall whose control net turns back by more than 170° ("Bad NonMonotonic CPts",
    /// #13243, #12107), though the curve itself may be smooth: a lettering outline drawn with
    /// one handle pulled past the other. Its halves are the same curve on control polygons
    /// closer to it, which turn less; 160° leaves the halves room below SMLib's 170°.
    const UNFOLD_TURN: f64 = -0.939_692_620_785_908_4;

    /// How many times a piece is halved at most: a curve that really turns back on itself (a
    /// cusp) has a control polygon that does at any depth, and goes in as it is.
    const UNFOLD_DEPTH: usize = 4;

    /// `c` as it goes into a sweep: a Bézier piece whose control polygon turns back by more than
    /// [`UNFOLD_TURN`] as its two halves at the middle of its parameter (de Casteljau, in
    /// homogeneous points, so a rational piece is split exactly too), each again so up to
    /// `depth` times; anything else as it is. [`joined`] takes the halves back as one B-spline:
    /// one curve, one wall.
    fn unfolded(c: Curve, depth: usize) -> Vec<Curve> {
        let Curve::Nurbs { degree, points, knots } = &c else { return vec![c] };
        if depth == 0 || points.len() != degree + 1 || knots.len() != 2 * (degree + 1) || !turns_back(points) {
            return vec![c];
        }
        let (left, right) = halves(points);
        let piece = |points: Vec<[f64; 4]>| Curve::Nurbs { degree: *degree, points, knots: knots.clone() };
        let mut out = unfolded(piece(left), depth - 1);
        out.extend(unfolded(piece(right), depth - 1));
        out
    }

    /// Whether the control polygon `points` (`[x, y, z, weight]`) turns back by more than
    /// [`UNFOLD_TURN`] between two steps, a step too short to have a direction passed over.
    fn turns_back(points: &[[f64; 4]]) -> bool {
        let at = |q: &[f64; 4]| Vec3::new(q[0], q[1], q[2]);
        let size = points.iter().map(|q| at(q).len()).fold(1.0, f64::max);
        let mut last: Option<Vec3> = None;
        for w in points.windows(2) {
            let [a, b] = w else { continue };
            let d = at(b) - at(a);
            let Some(step) = d.normalized().filter(|_| d.len() > size * 1e-12) else { continue };
            if last.is_some_and(|l| l.dot(step) < UNFOLD_TURN) {
                return true;
            }
            last = Some(step);
        }
        false
    }

    /// The control points of the two halves of the Bézier `points` (`[x, y, z, weight]`) split
    /// at the middle of its parameter, by de Casteljau's construction on the homogeneous points.
    fn halves(points: &[[f64; 4]]) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
        let hom = |q: &[f64; 4]| [q[0] * q[3], q[1] * q[3], q[2] * q[3], q[3]];
        let euclid = |h: [f64; 4]| if h[3] != 0.0 { [h[0] / h[3], h[1] / h[3], h[2] / h[3], h[3]] } else { h };
        let mut row: Vec<[f64; 4]> = points.iter().map(hom).collect();
        let (mut left, mut right) = (Vec::with_capacity(row.len()), Vec::with_capacity(row.len()));
        while let (Some(first), Some(last)) = (row.first().copied(), row.last().copied()) {
            left.push(euclid(first));
            right.push(euclid(last));
            let mid = |a: &[f64; 4], b: &[f64; 4]| [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2]), 0.5 * (a[3] + b[3])];
            row = row.iter().zip(row.iter().skip(1)).map(|(a, b)| mid(a, b)).collect();
        }
        right.reverse();
        (left, right)
    }

    /// How sharply a closed run of pieces of one curve may turn back where it meets its own start
    /// before it is broken (cosine of the turn, 169.9°). SMLib's check refuses a closed wall
    /// whose control net turns back by more than 170° across its seam (#13243 is the same check
    /// within a piece): a section drawn as one closed spline round a trailing edge sharper than
    /// 10°. Just under 170°, so a run SMLib takes as it is stays one wall.
    const SEAM_TURN: f64 = -0.984_503_179_974_436_7;

    /// `pieces` with each closed run of pieces of one curve ([`Piece::joins_previous`]) that turns
    /// back by more than [`SEAM_TURN`] where it meets its own start broken half-way round: the
    /// same curve as two walls SMLib's check takes, each named by its first piece. A run that
    /// turns back between two of its pieces is taken as it is.
    fn broken_at_seams(plane: &Plane, pieces: &[Piece]) -> Result<Vec<Piece>> {
        // The directions a free-form piece leaves its start and reaches its end in, along its
        // control polygon as SMLib's control net has it.
        let legs = |x: &Piece| -> Result<Option<(Vec3, Vec3)>> {
            let Curve::Nurbs { points, .. } = curve(plane, &x.seg)? else { return Ok(None) };
            let at = |q: &[f64; 4]| Vec3::new(q[0], q[1], q[2]);
            let (Some(first), Some(last)) = (points.first().map(at), points.last().map(at)) else { return Ok(None) };
            let tiny = 1e-12 * first.len().max(last.len()).max(1.0);
            let leaves = points.iter().map(at).find(|q| q.dist(first) > tiny).and_then(|q| (q - first).normalized());
            let reaches = points.iter().rev().map(at).find(|q| q.dist(last) > tiny).and_then(|q| (last - q).normalized());
            Ok(leaves.zip(reaches))
        };
        let mut out = pieces.to_vec();
        let mut i = 0;
        while i < out.len() {
            let j = i + 1 + out.iter().skip(i + 1).take_while(|x| x.joins_previous).count();
            if j - i > 1
                && let (Some(x), Some(y)) = (out.get(j - 1), out.get(i))
                && x.seg.end().dist(y.seg.start()) <= 1e-9 * (1.0 + y.seg.start().len())
                && matches!((legs(x)?, legs(y)?), (Some((_, end)), Some((start, _))) if end.dot(start) < SEAM_TURN)
                && let Some(half) = out.get_mut(i + (j - i) / 2)
            {
                half.joins_previous = false;
            }
            i = j;
        }
        Ok(out)
    }

    /// Bézier curves of one degree, each starting where the one before ends, as the one B-spline
    /// they are, exactly: their control points in a row (each joint once) on knots repeated the
    /// degree times at each joint. A rational piece's weights are scaled (which leaves it the same
    /// curve) to meet the weight the piece before ends with. Each piece's knot span is as long as
    /// keeps the speed through the joint the same on both sides, so pieces meeting tangentially
    /// (the pieces of one ellipse or spline) join with a continuous first derivative: SMLib makes
    /// one wall over a curve that is, and splits it where the derivative jumps.
    fn joined(run: Vec<Curve>) -> Result<Curve> {
        if run.len() == 1 {
            return run.into_iter().next().ok_or_else(|| KernelError::Invalid("an empty run of curves".into()));
        }
        let not_one = || KernelError::Invalid("the pieces of one curve are not Béziers of one degree".into());
        let xyz = |q: &[f64; 4]| Vec3::new(q[0], q[1], q[2]);
        let mut degree = None;
        let mut points: Vec<[f64; 4]> = Vec::new();
        let mut knots = Vec::new();
        // The knot at the last joint, and the length of the span before it.
        let (mut at, mut span) = (0.0, 1.0);
        for c in &run {
            let Curve::Nurbs { degree: d, points: ps, .. } = c else { return Err(not_one()) };
            if degree.is_some_and(|p| p != *d) || ps.len() != d + 1 || *d == 0 {
                return Err(not_one());
            }
            degree = Some(*d);
            let (Some(first), Some(second)) = (ps.first(), ps.get(1)) else { return Err(not_one()) };
            let (Some(last), Some(before)) = (points.last(), points.len().checked_sub(2).and_then(|i| points.get(i))) else {
                points.extend(ps.iter().copied());
                knots.extend(std::iter::repeat_n(0.0, d + 1));
                continue;
            };
            let scale = last[3] / first[3];
            if !(scale.is_finite() && scale > 0.0) {
                return Err(not_one());
            }
            // The derivative of a rational Bézier on a span of length h at its end is
            // d (w[d-1] / w[d]) (P[d] - P[d-1]) / h, at its start d (w[1] / w[0]) (P[1] - P[0]) / h.
            let ending = (xyz(last) - xyz(before)) * (before[3] / last[3]) * (1.0 / span);
            let starting = (xyz(second) - xyz(first)) * (second[3] / first[3]);
            let next = starting.len() / ending.len();
            at += span;
            span = if next.is_finite() && next > 0.0 { next } else { 1.0 };
            points.extend(ps.iter().skip(1).map(|q| [q[0], q[1], q[2], q[3] * scale]));
            knots.extend(std::iter::repeat_n(at, *d));
        }
        let degree = degree.ok_or_else(not_one)?;
        knots.extend(std::iter::repeat_n(at + span, degree + 1));
        Ok(Curve::Nurbs { degree, points, knots })
    }

    /// A chain of sketch segments on `plane` as SMLib's curves, one each, exactly (lines, arcs,
    /// Béziers and conics as NURBS): what SMLib's sweeps take as a profile or a path.
    #[allow(dead_code)] // For the sheet builders, which still build their own wires.
    pub(crate) fn chain_curves(plane: &Plane, segs: &[Seg2]) -> Result<Vec<Curve>> {
        segs.iter().map(|s| curve(plane, s)).collect()
    }

    /// The kernel body of an SMLib result: checked here when the evaluation's validation mode
    /// checks each step, left with the body for the feature's end when it checks each feature,
    /// and not checked otherwise (`crate::validation`).
    fn body_of(brep: Brep, what: &str) -> Result<Body> {
        use crate::validation::Step;
        let step = crate::validation::step();
        let body = crate::native::body_from_smlib(&brep)?;
        drop(brep);
        if step == Step::Check
            && let crate::validation::DecodedCheck::Refused(detail) = crate::validation::check_decoded(&format!("SMLib {what}"), body.record())?
        {
            return Err(failed(what, format!("SMLib's check refuses the result: {detail}")));
        }
        Ok(if step == Step::Defer { body.left_unchecked(&format!("SMLib {what}"), &[]) } else { body })
    }

    /// The body (a solid or a sheet) a schema 4 record describes, taken by SMLib as a
    /// modeller's own record (no healing) and checked there.
    pub(crate) fn body_from_record(record: &solidcore_record::brep::Brep, what: &str) -> Result<Body> {
        let brep = Brep::from_record(record, true).map_err(|e| failed(what, e))?;
        body_of(brep, what)
    }

    /// The sketch segments `segs` on `plane` projected along `direction` onto the face of
    /// `body` at `face_at`: SMLib's projection (smlib_project_curves), each projected curve the
    /// B-spline SMLib made, as a record curve on its interval, within a millionth of the body's
    /// size of the true projection.
    pub(crate) fn project_onto_face(
        body: &Body,
        face_at: Vec3,
        plane: &Plane,
        segs: &[Seg2],
        direction: Vec3,
    ) -> Result<Vec<(solidcore_record::brep::Curve, [f64; 2])>> {
        let c = segs.iter().map(|s| curve(plane, s)).collect::<Result<Vec<_>>>()?;
        let out = body
            .smlib()?
            .project_curves(p(face_at), pick_reach(body), &c, p(direction), curve_tolerance(body))
            .map_err(|e| curves_error("curve projection", e))?;
        record_curves(out, "curve projection")
    }

    /// The face of `body` at `face_at` cut by the plane through `plane_point` square to `normal`,
    /// SMLib's section curves on that face, as record curves on their intervals.
    pub(crate) fn face_section(
        body: &Body,
        face_at: Vec3,
        plane_point: Vec3,
        normal: Vec3,
    ) -> Result<Vec<(solidcore_record::brep::Curve, [f64; 2])>> {
        let out = body
            .smlib()?
            .face_section(p(face_at), pick_reach(body), p(plane_point), p(normal), curve_tolerance(body))
            .map_err(|e| curves_error("face section", e))?;
        record_curves(out, "face section")
    }

    /// How far a point may be from the face it picks: the kernel's picking reach.
    fn pick_reach(body: &Body) -> f64 {
        body.size().max(1e-9) * 1e-3 + 1e-6
    }

    /// How close a curve SMLib makes on a body's faces keeps to the true one: a millionth of the
    /// body's size.
    pub(crate) fn curve_tolerance(body: &Body) -> f64 {
        (body.size() * 1e-6).max(1e-6)
    }

    fn curves_error(what: &str, e: solidcraft_smlib::Error) -> KernelError {
        match e {
            solidcraft_smlib::Error::Empty => KernelError::Invalid(format!("{what}: no curve on the face")),
            solidcraft_smlib::Error::NoMatch { .. } => KernelError::Invalid(format!("{what}: no face of the body at the point")),
            e => failed(what, e),
        }
    }

    /// SMLib's NURBS curves as record curves (rational splines) on their intervals.
    fn record_curves(out: Vec<(Curve, [f64; 2])>, what: &str) -> Result<Vec<(solidcore_record::brep::Curve, [f64; 2])>> {
        out.into_iter()
            .map(|(c, interval)| match c {
                Curve::Nurbs { points, knots, .. } => {
                    let homogeneous = points.iter().map(|q| [q[0] * q[3], q[1] * q[3], q[2] * q[3], q[3]]).collect();
                    Ok((solidcore_record::brep::Curve::RationalSpline { knots, homogeneous }, interval))
                }
                _ => Err(failed(what, "a curve SMLib made is not a B-spline")),
            })
            .collect()
    }

    /// Cylinder built via `Brep::extrude` of the single circle arc on `plane` spanning `[lo, hi]`
    /// along `plane.normal()` with its seam at [`crate::build::SEAM_ANGLE`]. Produces the exact
    /// same B-rep as general `extrude` (with `solidcore`'s ray-free `stitch_solid` for a single
    /// right cylinder) while skipping `number_regions`, `broken_at_seams`, and `roles()` curve
    /// sampling. Carries the exact same cap/wall history and construction sources.
    pub(crate) fn cylinder(plane: &Plane, radius: f64, lo: f64, hi: f64, region: &Region2) -> Result<Body> {
        let (circle, n) = cylinder_circle(plane, radius, lo);
        let brep = Brep::extrude(&[circle], n, hi - lo, 0.0, DraftCorner::default(), true).map_err(|e| failed("extrude", e))?;
        let body = body_of(brep, "extrude")?;
        name_cylinder(body, plane, lo, hi, region)
    }

    /// The circle [`cylinder`] extrudes, at `lo` along the normal of `plane`, and the unit normal
    /// it goes along.
    pub(crate) fn cylinder_circle(plane: &Plane, radius: f64, lo: f64) -> (Curve, [f64; 3]) {
        let base = plane.offset(lo);
        let n = plane.normal();
        let circle = Curve::Arc {
            center: p(base.origin),
            axis: p(n),
            reference: p(plane.x),
            radius,
            start: crate::build::SEAM_ANGLE,
            end: crate::build::SEAM_ANGLE + std::f64::consts::TAU,
        };
        (circle, p(n))
    }

    /// `body`, a cylinder from `lo` to `hi` along `plane`'s normal, with the history and
    /// construction sources [`cylinder`] gives its own: the wall swept from the circle, the caps.
    pub(crate) fn name_cylinder(body: Body, plane: &Plane, lo: f64, hi: f64, region: &Region2) -> Result<Body> {
        let distance = hi - lo;
        let base = plane.offset(lo);
        let top = plane.offset(hi);
        let r = body.record();
        let size = body.size().max(1e-9);
        let tol = (size * 1e-5 + 1e-9).min(0.25 * distance);
        let seg = region.outer.segs.first().copied().ok_or_else(|| KernelError::Failed("cylinder profile segment".into()))?;
        let low_plane = Plane { origin: base.origin, x: plane.x, y: -plane.y };
        let cap_points = |surface: &solidcore_record::brep::Surface| -> Option<[Vec3; 3]> {
            match surface {
                solidcore_record::brep::Surface::Plane { origin, u_axis, v_axis } => {
                    let fo = crate::body::v3(*origin);
                    Some([fo, fo + crate::body::v3(*u_axis), fo + crate::body::v3(*v_axis)])
                }
                solidcore_record::brep::Surface::PlaneThrough { origin, u_point, v_point } => {
                    Some([crate::body::v3(*origin), crate::body::v3(*u_point), crate::body::v3(*v_point)])
                }
                _ => None,
            }
        };
        let mut prov = Vec::with_capacity(r.faces.len());
        let mut sources = Vec::with_capacity(r.faces.len());
        let (mut starts, mut ends, mut walls) = (0usize, 0usize, 0usize);
        for face in &r.faces {
            if matches!(face.surface, solidcore_record::brep::Surface::Cylinder { .. }) {
                walls += 1;
                prov.push(record::profile(&[0usize]));
                sources.push(Some(crate::FaceSource::ExtrudeSide { segment: seg, plane: None, coalesced: false }));
            } else if let Some(pts) = cap_points(&face.surface) {
                if pts.iter().all(|pt| base.height(*pt).abs() <= tol) {
                    starts += 1;
                    prov.push(record::cap(false));
                    sources.push(Some(crate::FaceSource::ExtrudeCap { high: false, region: region.clone(), plane: low_plane }));
                } else if pts.iter().all(|pt| top.height(*pt).abs() <= tol) {
                    ends += 1;
                    prov.push(record::cap(true));
                    sources.push(Some(crate::FaceSource::ExtrudeCap { high: true, region: region.clone(), plane: top }));
                } else {
                    return Err(failed("cylinder", "unexpected planar face on cylinder"));
                }
            } else {
                return Err(failed("cylinder", "unexpected surface on cylinder"));
            }
        }
        if (starts, ends, walls) != (1, 1, 1) {
            return Err(failed("cylinder", format!("unexpected cylinder face roles: start={starts}, end={ends}, wall={walls}")));
        }
        let body = body.with_face_sources(Some(std::sync::Arc::new(sources)));
        Ok(record::attach(body, &[], Some(prov), &[]))
    }

    /// SMLib's sphere about `center`, each face swept from the arc (segment 0) of the half disc
    /// a sphere revolves.
    pub(crate) fn sphere(center: Vec3, radius: f64) -> Result<Body> {
        let brep = Brep::make_sphere(p(center), radius).map_err(|e| failed("sphere", e))?;
        name_revolved_primitive(body_of(brep, "sphere")?)
    }

    /// A sphere's or torus's history as [`sphere`] and [`torus`] give it: every face swept from
    /// the one profile segment the solid revolves.
    pub(crate) fn name_revolved_primitive(body: Body) -> Result<Body> {
        let faces = body.face_count();
        Ok(record::attach(body, &[], Some(vec![record::profile(&[0usize]); faces]), &[]))
    }

    /// SMLib's torus about the Z axis through `center`, each face swept from the circle
    /// (segment 0) a torus revolves.
    pub(crate) fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
        let brep = Brep::make_torus(p(center), major, minor).map_err(|e| failed("torus", e))?;
        name_revolved_primitive(body_of(brep, "torus")?)
    }

    /// A point a wall is known by: on its edge in the start plane, or inside it; and the piece
    /// (by index) that sweeps it.
    struct Seed {
        at: Vec3,
        piece: usize,
    }

    fn seeds(plane: &Plane, pieces: &[Piece], to: impl Fn(Vec3) -> Vec3) -> Vec<Seed> {
        pieces.iter().enumerate().map(|(piece, x)| Seed { at: to(plane.to_world(x.seg.point_at(0.5))), piece }).collect()
    }

    /// What a face of a swept body is.
    #[derive(Clone, Debug, PartialEq)]
    enum Role {
        Start,
        End,
        /// A wall, swept from these pieces.
        Wall(BTreeSet<usize>),
        Unknown,
    }

    /// The role of each face of `body` swept from the profile in `start` (see the module notes),
    /// from the body's exact geometry: a face on the start plane is the start cap where a point
    /// of `edges` lies on it, one on `end` the end cap (with `end` unset and `rest_is_end`, a
    /// planar face nothing else claims); a wall comes from each piece whose point in `edges` lies
    /// on its edge in the start plane, or whose point in `inside` lies on it. Each point names
    /// the nearest edges or faces: one where SMLib split a curve or a wall lies on both.
    fn roles(body: &Body, start: &Plane, end: Option<&Plane>, edges: &[Seed], inside: &[Seed], rest_is_end: bool) -> Result<Vec<Role>> {
        use solidcore_record::brep::{Curve as RecordCurve, Surface};
        use solidcraft_geom::brep::eval::CurveEval;
        let r = body.record();
        let size = body.size().max(1e-9);
        // A cap lies on its plane to the evaluation's precision. Sized to the body, that can exceed
        // the gap between parallel start and end planes (a box 0.1 µm thick in a 14 mm body), which
        // would put each cap on both: it stays under a quarter of that gap.
        let gap = end
            .map(|e| start.height(e.origin).abs())
            .filter(|g| end.is_some_and(|e| e.normal().cross(start.normal()).len() < 1e-9) && *g > size * 1e-9)
            .unwrap_or(f64::INFINITY);
        let tol = (size * 1e-5 + 1e-9).min(0.25 * gap);
        let vertex = |v: u32| r.vertices.get(v as usize).map(|v| crate::body::v3(v.point));
        // A planar face's point and unit normal.
        let flat = |f: &solidcore_record::brep::Face| {
            let (o, u, v) = match &f.surface {
                Surface::Plane { origin, u_axis, v_axis } => (crate::body::v3(*origin), crate::body::v3(*u_axis), crate::body::v3(*v_axis)),
                Surface::PlaneThrough { origin, u_point, v_point } => {
                    let o = crate::body::v3(*origin);
                    (o, crate::body::v3(*u_point) - o, crate::body::v3(*v_point) - o)
                }
                _ => return None,
            };
            u.cross(v).normalized().map(|n| (o, n))
        };
        let on = |f: &solidcore_record::brep::Face, pl: &Plane| {
            flat(f).is_some_and(|(o, n)| n.dot(pl.normal()).abs() >= 1.0 - 1e-9 && pl.height(o).abs() <= tol)
        };
        // The edges in the start plane: their ends and middle on it.
        let mut base = Vec::new();
        for (i, e) in r.edges.iter().enumerate() {
            let (Some(a), Some(b)) = (vertex(e.vertices[0]), vertex(e.vertices[1])) else { continue };
            // An edge SMLib made within its own tolerance of the curve it stands for (a screw's
            // start) is taken to that tolerance.
            let reach = tol.max(e.tolerance);
            if start.height(a).abs() > reach || start.height(b).abs() > reach {
                continue;
            }
            let curve = CurveEval::new(e.parameters()).map_err(|err| KernelError::Failed(format!("extrude history: edge {i}: {err}")))?;
            let [t0, t1] = e.interval;
            let mid = curve.point(0.5 * (t0 + t1)).ok_or_else(|| KernelError::Failed(format!("extrude history: edge {i} has no middle")))?;
            if start.height(mid).abs() > reach {
                continue;
            }
            // The curve's control points bound it.
            let mut bounds = solidcraft_geom::Aabb3::EMPTY;
            match e.parameters() {
                RecordCurve::Line { .. } => [a, b].into_iter().for_each(|q| bounds.add(q)),
                RecordCurve::Spline { points, .. } => points.iter().for_each(|q| bounds.add(crate::body::v3(*q))),
                RecordCurve::RationalSpline { homogeneous, .. } if homogeneous.iter().all(|h| h[3] > 0.0) => {
                    homogeneous.iter().for_each(|h| bounds.add(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])))
                }
                _ => {
                    bounds = solidcraft_geom::Aabb3 { min: Vec3::new(-f64::MAX, -f64::MAX, -f64::MAX), max: Vec3::new(f64::MAX, f64::MAX, f64::MAX) }
                }
            }
            // A line by its ends; a curve searched from eight samples a knot span.
            let samples = match e.parameters() {
                RecordCurve::Line { .. } => None,
                RecordCurve::Spline { knots, .. } | RecordCurve::RationalSpline { knots, .. } => {
                    Some(8 * knots.windows(2).filter(|w| w[1] > w[0]).count().max(8))
                }
                _ => Some(64),
            };
            base.push((i, curve, e.interval, [a, b], samples, reach, bounds.inflated(reach)));
        }
        // Each seed's nearest edges, and the faces on them.
        let mut matched: Vec<(usize, usize)> = Vec::new();
        for s in edges {
            let ds: Vec<(usize, f64, f64)> = base
                .iter()
                .filter(|(.., bounds)| bounds.contains(s.at))
                .map(|(i, curve, interval, [a, b], samples, reach, _)| {
                    let Some(samples) = samples else { return (*i, s.at.dist_to_segment(*a, *b), *reach) };
                    let along = crate::surfaces::nearest_parameter_in(curve, *interval, s.at, *samples).map_or(f64::INFINITY, |(_, d)| d);
                    (*i, along.min(s.at.dist(*a)).min(s.at.dist(*b)), *reach)
                })
                .collect();
            let nearest = ds.iter().map(|(_, d, _)| *d).fold(f64::INFINITY, f64::min);
            matched.extend(ds.iter().filter(|(_, d, reach)| *d <= *reach && *d <= nearest + tol).map(|(i, ..)| (*i, s.piece)));
        }
        let faces_on = |e: usize| r.edges.get(e).into_iter().flat_map(|e| e.uses.iter().map(|u| u.face as usize));
        let touched: BTreeSet<usize> = matched.iter().flat_map(|(e, _)| faces_on(*e)).collect();
        let n = r.faces.len();
        // How far a point may lie off a face SMLib made to its own tolerance.
        let reach = r.edges.iter().map(|e| e.tolerance).fold(tol, f64::max);
        let ops = || body.ops_body().ok_or_else(|| KernelError::Failed("sweep history: the body is no SMLib body".into()));
        let points = |seeds: &[Seed]| seeds.iter().map(|s| p(s.at)).collect::<Vec<_>>();
        // A face in the start plane is the start cap where a profile point lies on it: on its edges
        // (an extrusion's), or inside it (a screw's start section, SMLib's approximation of the profile).
        let mut start_cap = Vec::with_capacity(r.faces.len());
        for (f, face) in r.faces.iter().enumerate() {
            let on_it = on(face, start)
                && (edges.is_empty() || touched.contains(&f) || {
                    let form = ops()?;
                    let (ops, order) = &*form;
                    let ds =
                        ops.face_distances(order, f, &points(edges)).map_err(|e| KernelError::Failed(format!("sweep history: face {f}: {e}")))?;
                    ds.iter().any(|d| d.is_some_and(|d| d <= reach))
                });
            start_cap.push(on_it);
        }
        let end_cap: Vec<bool> = r.faces.iter().map(|face| end.is_some_and(|e| on(face, e))).collect();
        let mut swept: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
        for (e, piece) in &matched {
            for f in faces_on(*e) {
                if !start_cap.get(f).copied().unwrap_or(false)
                    && !end_cap.get(f).copied().unwrap_or(false)
                    && let Some(set) = swept.get_mut(f)
                {
                    set.insert(*piece);
                }
            }
        }
        // Inside the walls: the faces nearest each point, as trimmed (a point on the edge between
        // two faces of one wall lies on both).
        if !inside.is_empty() {
            let form = ops()?;
            let (ops, order) = &*form;
            let points = points(inside);
            // Only a face within `reach` of a point can claim it below or be its nearest, so the
            // seam is asked about those alone and answers `None` for the rest: every face against
            // every point grew with the square of the profile's segment count (#15134).
            let ds =
                ops.face_distances_within(order, &points, reach).map_err(|e| KernelError::Failed(format!("sweep history: face distances: {e}")))?;
            for (j, s) in inside.iter().enumerate() {
                let d = |f: usize| ds.get(f).and_then(|d| d.get(j).copied().flatten()).unwrap_or(f64::INFINITY);
                let nearest = (0..n).map(d).fold(f64::INFINITY, f64::min);
                for f in (0..n).filter(|f| d(*f) <= reach && d(*f) <= nearest + tol) {
                    if !start_cap.get(f).copied().unwrap_or(false)
                        && !end_cap.get(f).copied().unwrap_or(false)
                        && let Some(set) = swept.get_mut(f)
                    {
                        set.insert(s.piece);
                    }
                }
            }
        }
        Ok(swept
            .into_iter()
            .enumerate()
            .map(|(f, set)| {
                if start_cap.get(f).copied().unwrap_or(false) {
                    Role::Start
                } else if end_cap.get(f).copied().unwrap_or(false) {
                    Role::End
                } else if !set.is_empty() {
                    Role::Wall(set)
                } else if end.is_none() && rest_is_end && r.faces.get(f).is_some_and(|face| flat(face).is_some()) {
                    Role::End
                } else {
                    log::debug!(target: "smlib", "sweep history: face {f} comes from no profile segment");
                    Role::Unknown
                }
            })
            .collect())
    }

    /// The history the roles give: caps, and walls from their pieces' profile segments.
    fn history(roles: &[Role], pieces: &[Piece]) -> Vec<FaceProv> {
        roles
            .iter()
            .map(|r| match r {
                Role::Start => record::cap(false),
                Role::End => record::cap(true),
                Role::Wall(set) => record::profile(set.iter().filter_map(|i| pieces.get(*i)).flat_map(|x| x.numbers.iter())),
                Role::Unknown => record::generated(Vec::new()),
            })
            .collect()
    }

    fn with_history(body: Body, roles: &[Role], pieces: &[Piece]) -> Result<Body> {
        Ok(record::attach(body, &[], Some(history(roles, pieces)), &[]))
    }

    /// The profile `pieces` on `plane`, moved `from` along its normal, extruded `distance`
    /// along `sign` (±1) times the normal, straight or drafted.
    pub(crate) fn extrude(
        plane: &Plane,
        pieces: &[Piece],
        region: Option<&Region2>,
        from: f64,
        distance: f64,
        sign: f64,
        draft: Option<Draft>,
    ) -> Result<Body> {
        let base = plane.offset(from);
        let n = profile_normal(plane)?;
        let dir = n * sign.signum();
        // Only extrusions take runs of pieces as one curve (`crate::build`).
        let pieces = &broken_at_seams(&base, pieces)?;
        let c = curves(&base, pieces)?;
        let top = plane.offset(from + sign.signum() * distance);
        let edges = seeds(&base, pieces, |q| q);
        let foot = draft.map_or(0.0, |d| d.foot);
        if let Some(d) = draft.filter(|d| d.foot > 0.0) {
            // A straight foot, then the walls sloping from its top to the far end the taper gives
            // from the base, joined face to face where they meet.
            if !(foot < distance) {
                return Err(KernelError::Invalid(format!("taper foot = {foot}")));
            }
            let knee = plane.offset(from + sign.signum() * foot);
            let lean = -(distance * d.taper.tan() / (distance - foot)).atan();
            let prism = Brep::extrude(&c, p(dir), foot, 0.0, DraftCorner::default(), true).map_err(|e| failed("extrude", e))?;
            let slope =
                Brep::extrude(&curves(&knee, pieces)?, p(dir), distance - foot, lean, corner(d.corner), true).map_err(|e| failed("extrude", e))?;
            let brep = prism.boolean(&slope, solidcraft_smlib::BooleanOp::Union).map_err(|e| failed("extrude", e))?;
            let brep = brep.ok_or_else(|| failed("extrude", "nothing is left"))?;
            let body = body_of(brep, "extrude")?;
            // The caps and the foot's walls from the base; the sloping walls (and the foot's again)
            // from the edges at the knee, which both stand on.
            let low = roles(&body, &base, Some(&top), &edges, &[], false)?;
            let high = roles(&body, &knee, Some(&top), &seeds(&knee, pieces, |q| q), &[], false)?;
            let roles: Vec<Role> = low
                .into_iter()
                .zip(high)
                .map(|(a, b)| match (a, b) {
                    (Role::Wall(mut x), Role::Wall(y)) => {
                        x.extend(y);
                        Role::Wall(x)
                    }
                    (Role::Unknown, w @ Role::Wall(_)) => w,
                    (a, _) => a,
                })
                .collect();
            return with_history(body, &roles, pieces);
        }
        // SMLib's draft is positive inward; the kernel's taper grows the profile.
        let lean = draft.map_or(0.0, |d| -d.taper);
        let rule = draft.map_or(DraftCorner::default(), |d| corner(d.corner));
        let brep = Brep::extrude(&c, p(dir), distance, lean, rule, true).map_err(|e| failed("extrude", e))?;
        let body = body_of(brep, "extrude")?;
        named_extrusion(body, plane, pieces, region, &base, &top, &edges, draft.is_none() && sign > 0.0)
    }

    /// A straight extrusion's faces given their roles, construction sources (`sources`) and
    /// history, from the profile `pieces` as swept (broken at their seams).
    #[allow(clippy::too_many_arguments)]
    fn named_extrusion(
        body: Body,
        plane: &Plane,
        pieces: &[Piece],
        region: Option<&Region2>,
        base: &Plane,
        top: &Plane,
        edges: &[Seed],
        sources: bool,
    ) -> Result<Body> {
        let roles = roles(&body, base, Some(top), edges, &[], false)?;
        let sources = sources.then(|| extrude_sources(&roles, pieces, region, plane, base, top));
        let body = body.with_face_sources(sources.map(std::sync::Arc::new));
        with_history(body, &roles, pieces)
    }

    /// The curves another kernel extrudes for [`extrude`] without a draft along the normal
    /// (`sign` 1): the profile `pieces` moved `from` along `plane`'s normal and broken at their
    /// seams, each run of one curve as one curve, loop after loop, and the unit direction.
    #[cfg(feature = "backend-occt")]
    pub(crate) fn extrusion_curves(plane: &Plane, pieces: &[Piece], from: f64) -> Result<(Vec<Curve>, [f64; 3])> {
        let base = plane.offset(from);
        let n = profile_normal(plane)?;
        Ok((curves(&base, &broken_at_seams(&base, pieces)?)?, p(n)))
    }

    /// The unit normal of a profile's plane.
    fn profile_normal(plane: &Plane) -> Result<Vec3> {
        plane.x.cross(plane.y).normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))
    }

    /// `body`, another kernel's extrusion of [`extrusion_curves`] by `distance`, with the roles,
    /// construction sources and history [`extrude`] gives its own.
    #[cfg(feature = "backend-occt")]
    pub(crate) fn name_extrusion(body: Body, plane: &Plane, pieces: &[Piece], region: Option<&Region2>, from: f64, distance: f64) -> Result<Body> {
        let base = plane.offset(from);
        let pieces = &broken_at_seams(&base, pieces)?;
        let top = plane.offset(from + distance);
        let edges = seeds(&base, pieces, |q| q);
        named_extrusion(body, plane, pieces, region, &base, &top, &edges, true)
    }

    /// The solid `pieces` (one closed loop) on `plane` sweep, moved `from` along its normal and
    /// drafted `d` over `distance` along `sign` × the normal, as [`extrude`] builds it, without
    /// its roles.
    fn drafted_loop(plane: &Plane, pieces: &[Piece], from: f64, distance: f64, sign: f64, d: Draft) -> Result<Brep> {
        let base = plane.offset(from);
        let n = profile_normal(plane)?;
        let dir = n * sign.signum();
        if d.foot > 0.0 {
            if !(d.foot < distance) {
                return Err(KernelError::Invalid(format!("taper foot = {}", d.foot)));
            }
            let knee = plane.offset(from + sign.signum() * d.foot);
            let lean = -(distance * d.taper.tan() / (distance - d.foot)).atan();
            let prism =
                Brep::extrude(&curves(&base, pieces)?, p(dir), d.foot, 0.0, DraftCorner::default(), true).map_err(|e| failed("extrude", e))?;
            let slope =
                Brep::extrude(&curves(&knee, pieces)?, p(dir), distance - d.foot, lean, corner(d.corner), true).map_err(|e| failed("extrude", e))?;
            let brep = prism.boolean(&slope, solidcraft_smlib::BooleanOp::Union).map_err(|e| failed("extrude", e))?;
            return brep.ok_or_else(|| failed("extrude", "nothing is left"));
        }
        Brep::extrude(&curves(&base, pieces)?, p(dir), distance, -d.taper, corner(d.corner), true).map_err(|e| failed("extrude", e))
    }

    /// A drafted extrusion of a region with holes: SMLib's draft extrusion takes one loop, so the
    /// outer loop is drafted and each hole, turned counter-clockwise, is drafted the other way
    /// and cut from it. Every wall leans by the taper on the material's side, as a single
    /// drafted sweep would: a taper that narrows the outer boundary widens the holes. `outer` and
    /// `holes` are the loops' pieces at `from` as [`extrude`] takes them, `all` the region's
    /// pieces in their numbering (outer, then the holes as they bound the material), which the
    /// walls' roles name.
    pub(crate) fn extrude_drafted_with_holes(
        plane: &Plane,
        outer: &[Piece],
        holes: &[Vec<Piece>],
        all: &[Piece],
        from: f64,
        distance: f64,
        sign: f64,
        draft: Draft,
    ) -> Result<Body> {
        let mut brep = drafted_loop(plane, outer, from, distance, sign, draft)?;
        for h in holes {
            let cutter = drafted_loop(plane, h, from, distance, sign, Draft { taper: -draft.taper, ..draft })?;
            brep = brep
                .boolean(&cutter, solidcraft_smlib::BooleanOp::Subtract)
                .map_err(|e| failed("extrude", e))?
                .ok_or_else(|| KernelError::Invalid("the draft leaves nothing of the profile".into()))?;
        }
        let body = body_of(brep, "extrude")?;
        let base = plane.offset(from);
        let top = plane.offset(from + sign.signum() * distance);
        let low = roles(&body, &base, Some(&top), &seeds(&base, all, |q| q), &[], false)?;
        let roles: Vec<Role> = if draft.foot > 0.0 {
            // The sloping walls stand on their edges at the knee, as in [`extrude`].
            let knee = plane.offset(from + sign.signum() * draft.foot);
            let high = roles(&body, &knee, Some(&top), &seeds(&knee, all, |q| q), &[], false)?;
            low.into_iter()
                .zip(high)
                .map(|(a, b)| match (a, b) {
                    (Role::Wall(mut x), Role::Wall(y)) => {
                        x.extend(y);
                        Role::Wall(x)
                    }
                    (Role::Unknown, w @ Role::Wall(_)) => w,
                    (a, _) => a,
                })
                .collect()
        } else {
            low
        };
        with_history(body, &roles, all)
    }

    /// The construction sources of a straight extrusion's faces along the normal of `sketch`, as
    /// [`crate::extrude`] gives them: each cap the region at its end (both caps found once
    /// each), each wall swept from one piece that piece as drawn, a line's with the wall's plane.
    fn extrude_sources(
        roles: &[Role],
        pieces: &[Piece],
        region: Option<&Region2>,
        sketch: &Plane,
        base: &Plane,
        top: &Plane,
    ) -> Vec<Option<crate::FaceSource>> {
        let once = |role: &Role| roles.iter().filter(|r| *r == role).count() == 1;
        let caps = region.filter(|r| once(&Role::Start) && once(&Role::End) && numbered_segments(r) <= 4096);
        roles
            .iter()
            .map(|role| match role {
                Role::Start => caps.map(|r| crate::FaceSource::ExtrudeCap {
                    high: false,
                    region: r.clone(),
                    plane: Plane { origin: base.origin, x: sketch.x, y: -sketch.y },
                }),
                Role::End => caps.map(|r| crate::FaceSource::ExtrudeCap { high: true, region: r.clone(), plane: *top }),
                Role::Wall(set) if set.len() == 1 => set.first().and_then(|i| pieces.get(*i)).map(|x| {
                    let plane = match x.seg {
                        Seg2::Line { a, b } => (b - a).normalized().and_then(|d| {
                            let foot = a - d * a.dot(d);
                            Plane::new(sketch.to_world(foot), sketch.dir_to_world(d), sketch.normal())
                        }),
                        _ => None,
                    };
                    crate::FaceSource::ExtrudeSide { segment: x.drawn, plane, coalesced: false }
                }),
                // The one wall over a whole run of pieces of one curve: named by its first piece.
                Role::Wall(set) if run_starts(pieces, set).is_some() => run_starts(pieces, set)
                    .and_then(|i| pieces.get(i))
                    .map(|x| crate::FaceSource::ExtrudeSide { segment: x.drawn, plane: None, coalesced: false }),
                _ => None,
            })
            .collect()
    }

    /// The first piece of the run of joined pieces `set` is exactly (all of it, and nothing else).
    fn run_starts(pieces: &[Piece], set: &BTreeSet<usize>) -> Option<usize> {
        let (&first, &last) = (set.first()?, set.last()?);
        let run = pieces.get(first..=last)?;
        let whole = last - first + 1 == set.len()
            && run.first().is_some_and(|x| !x.joins_previous)
            && run.iter().skip(1).all(|x| x.joins_previous)
            && pieces.get(last + 1).is_none_or(|x| !x.joins_previous);
        (set.len() > 1 && whole).then_some(first)
    }

    fn numbered_segments(r: &Region2) -> usize {
        r.outer.segs.len() + r.holes.iter().map(|h| h.segs.len()).sum::<usize>()
    }

    /// `v` turned by `a` right-handed about the unit `k` (Rodrigues).
    fn turned(v: Vec3, k: Vec3, a: f64) -> Vec3 {
        v * a.cos() + k.cross(v) * a.sin() + k * (k.dot(v) * (1.0 - a.cos()))
    }

    /// The profile `pieces` on `plane` turned `angle` radians (signed, right-handed about `axis`)
    /// about the axis through `origin` (both in the plane). Segments on the axis sweep nothing.
    pub(crate) fn revolve(plane: &Plane, pieces: &[Piece], origin: Vec3, axis: Vec3, angle: f64, join_arcs: bool) -> Result<Body> {
        let (c, o, k, turn) = revolution_curves(plane, pieces, origin, axis, angle)?;
        let revolve = if join_arcs { Brep::revolve_joining_arcs } else { Brep::revolve };
        let brep = revolve(&c, o, k, turn, true).map_err(|e| failed("revolve", e))?;
        let body = body_of(brep, "revolve")?;
        name_revolution(body, plane, pieces, origin, Vec3::new(k[0], k[1], k[2]), turn)
    }

    /// The curves another kernel turns for [`revolve`] (arcs not joined), with the axis through
    /// `origin` along the unit axis and the angle (0 < angle <= 2π) it turns them by.
    pub(crate) fn revolution_curves(
        plane: &Plane,
        pieces: &[Piece],
        origin: Vec3,
        axis: Vec3,
        angle: f64,
    ) -> Result<(Vec<Curve>, [f64; 3], [f64; 3], f64)> {
        let k = axis.normalized().ok_or_else(|| KernelError::Invalid("revolve axis".into()))?;
        let (k, turn) = if angle < 0.0 { (-k, -angle) } else { (k, angle) };
        let turn = turn.min(std::f64::consts::TAU);
        Ok((curves(plane, pieces)?, p(origin), p(k), turn))
    }

    /// `body`, a revolution of `pieces` by `turn` about the unit axis `k` through `origin`, with
    /// the roles and history [`revolve`] gives its own.
    pub(crate) fn name_revolution(body: Body, plane: &Plane, pieces: &[Piece], origin: Vec3, k: Vec3, turn: f64) -> Result<Body> {
        let size = body.size().max(1e-9);
        let off_axis = |q: Vec3| {
            let w = q - origin;
            (w - k * w.dot(k)).len() > size * 1e-7
        };
        let walls: Vec<Piece> = pieces
            .iter()
            .filter(|x| [0.0, 0.5, 1.0].iter().any(|t| off_axis(plane.to_world(x.seg.point_at(*t)))))
            .map(|x| Piece { seg: x.seg, numbers: x.numbers.clone(), drawn: x.drawn, joins_previous: false })
            .collect();
        let about = |q: Vec3, a: f64| origin + turned(q - origin, k, a);
        let full = turn >= std::f64::consts::TAU - 1e-9;
        let edges = seeds(plane, &walls, |q| q);
        let inside = seeds(plane, &walls, |q| about(q, turn * 0.5));
        let end = (!full).then(|| Plane { origin: about(plane.origin, turn), x: turned(plane.x, k, turn), y: turned(plane.y, k, turn) });
        // A full turn has no caps, and its profile plane holds seams only.
        let roles = if full {
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            roles(&body, plane, end.as_ref(), &edges, &inside, false)?
        };
        with_history(body, &roles, &walls)
    }

    /// A path segment as SMLib's curve, exactly.
    pub(crate) fn path_curve(s: &crate::PathSeg) -> Result<Curve> {
        Ok(match *s {
            crate::PathSeg::Line { a, b } => Curve::Line { start: p(a), end: p(b) },
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized().ok_or_else(|| KernelError::Invalid("path arc axis".into()))?;
                let (k, turn) = if angle < 0.0 { (-k, -angle) } else { (k, angle) };
                let r = a - center;
                let radius = r.len();
                let reference = r.normalized().ok_or_else(|| KernelError::Invalid("path arc radius".into()))?;
                Curve::Arc { center: p(center), axis: p(k), reference: p(reference), radius, start: 0.0, end: turn }
            }
            crate::PathSeg::Curve { plane, seg } => curve(&plane, &seg)?,
            crate::PathSeg::Bezier(b) => {
                let degree = b.degree();
                let points = b.weighted_points().ok_or_else(|| KernelError::Invalid("a path span does not evaluate".into()))?;
                Curve::Nurbs { degree, points, knots: std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect() }
            }
        })
    }

    /// The profile `pieces` on `plane` swept along `path`, turning with it, or only moving along
    /// it when `translate`. SMLib's sweep follows a smooth path; at a corner (the direction
    /// jumping) the path is swept piece by piece ([`cornered`]). A profile drawn away from the
    /// path's start is first carried there ([`at_path_start`]).
    pub(crate) fn sweep(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<Body> {
        sweep_from_start(&at_path_start(plane, pieces, path, translate)?, pieces, path, translate)
    }

    /// [`sweep`] of a profile already where the sweep starts.
    fn sweep_from_start(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<Body> {
        if translate {
            // Every segment's direction of travel, at its ends and between them.
            let bad = || KernelError::Invalid("a path segment has no direction".into());
            let tangents = path
                .iter()
                .flat_map(|s| (0..=CROSSING_SAMPLES).map(move |i| seg_at(s, i as f64 / CROSSING_SAMPLES as f64).map(|(_, t)| t)))
                .collect::<Option<Vec<Vec3>>>()
                .ok_or_else(bad)?;
            crate::sweep_path::crossing_check(plane, &tangents, false)?;
        } else {
            turning_check(plane, pieces, path)?;
        }
        let runs = smooth_runs(path)?;
        if runs.len() > 1 {
            return cornered(plane, pieces, path, &runs, translate);
        }
        let (brep, [_, middle, last]) = swept_run(plane, pieces, path, translate)?;
        let body = body_of(brep, "sweep")?;
        // The walls known by the profile carried half way (past the start's edges: a wall the
        // start touches by no edge, as round a closed path), and half way along each segment of
        // the path (SMLib makes a wall along each segment of a path of several).
        let mut inside = seeds(plane, pieces, |q| apply(&middle, q));
        inside.extend(segment_seeds(plane, pieces, path, 0..path.len(), translate));
        let start = path.first().map(crate::PathSeg::start);
        let end = path.last().map(seg_ends).transpose()?.map(|(_, b)| b);
        let roles = match (start, end) {
            // Round a closed path there are no caps, and the start holds seams only.
            (Some(a), Some(b)) if a.dist(b) <= 1e-6 => roles(&body, &plane.offset(body.size().max(1e-9) * 10.0), None, &[], &inside, false)?,
            _ => roles(&body, plane, Some(&placed(plane, &last)), &seeds(plane, pieces, |q| q), &inside, false)?,
        };
        with_history(body, &roles, pieces)
    }

    /// [`crate::sweep_path::crossing_check`] with `turning` of the profile on `plane`, already where the
    /// sweep starts, against the path's direction there. It applies only to a profile the path
    /// starts in (the start point lies inside the profile or on its outline): that profile lies
    /// along the path. One that sits off the path, though its plane holds the direction, still
    /// sweeps a volume and is left to the sweep.
    fn turning_check(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg]) -> Result<()> {
        let (t0, _) =
            path.first().and_then(crate::PathSeg::tangents).ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
        let Some(start) = path.first().map(crate::PathSeg::start) else { return Ok(()) };
        if path_starts_in(plane, pieces, start) {
            crate::sweep_path::crossing_check(plane, &[t0], true)?;
        }
        Ok(())
    }

    /// Whether `start`, a point of `plane` (to the sweep's tolerance), lies inside the profile
    /// `pieces` by the even-odd rule on their outlines sampled at 64 points a piece, or within a
    /// millionth of the profile's size of one of those points.
    fn path_starts_in(plane: &Plane, pieces: &[Piece], start: Vec3) -> bool {
        const SAMPLES: usize = 64;
        let p = plane.to_local(start);
        let outline: Vec<Vec<Vec2>> = pieces.iter().map(|x| (0..=SAMPLES).map(|i| x.seg.point_at(i as f64 / SAMPLES as f64)).collect()).collect();
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for q in outline.iter().flatten() {
            (lo, hi) = (Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y)));
        }
        let near = 1e-6 * (hi - lo).len();
        if outline.iter().flatten().any(|q| (*q - p).len() <= near) {
            return true;
        }
        let mut crossings = 0;
        for chain in &outline {
            for w in chain.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) {
                    crossings += 1;
                }
            }
        }
        crossings % 2 == 1
    }

    /// The sweep of the profile on `plane` along `path`, with the rigid motions SMLib reports
    /// carrying the profile onto its first, middle and last sections.
    fn swept(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<(Brep, [Affine; 3])> {
        let c = curves(plane, pieces)?;
        let route = path.iter().map(path_curve).collect::<Result<Vec<_>>>()?;
        let frame = if translate { SweepFrame::Translate } else { SweepFrame::Follow };
        Brep::sweep_placed(&c, &route, frame, true).map_err(|e| sweep_failed(path, e))
    }

    /// SMLib's refusal of a sweep along `path`, in words where its status says why: an arc of the
    /// path is swept as a turn about the arc's axis, and `SM_ERR_AXIS_INSIDE_FACE` (1025) is the
    /// profile reaching across that axis, past the centre of the bend (#12126).
    fn sweep_failed(path: &[crate::PathSeg], e: solidcraft_smlib::Error) -> KernelError {
        let tightest = path
            .iter()
            .filter_map(|s| match *s {
                crate::PathSeg::Arc { a, center, .. } => Some(a.dist(center)),
                _ => None,
            })
            .min_by(f64::total_cmp);
        match (&e, tightest) {
            (solidcraft_smlib::Error::Status(1025) | solidcraft_smlib::Error::Refused(solidcore_ops::Error::Status(1025)), Some(radius)) => {
                KernelError::Invalid(format!(
                    "the profile reaches past the centre of a bend in the path: it must stay inside the radius of every bend it turns towards (the tightest bend here is {} mm)",
                    crate::blend::trimmed_mm(radius)
                ))
            }
            _ => failed("sweep", e),
        }
    }

    /// `q` moved by the motion `a`.
    fn apply(a: &Affine, q: Vec3) -> Vec3 {
        let col = |i: usize| a.m.get(i).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
        col(0) * q.x + col(1) * q.y + col(2) * q.z + Vec3::new(a.t[0], a.t[1], a.t[2])
    }

    /// `pl` carried by the motion `a` (a rigid motion: its directions turn with it).
    fn placed(pl: &Plane, a: &Affine) -> Plane {
        let o = apply(a, Vec3::ZERO);
        Plane { origin: apply(a, pl.origin), x: apply(a, pl.x) - o, y: apply(a, pl.y) - o }
    }

    /// The path's segments in runs along which its direction carries on (within a thousandth of
    /// a radian across each joint): a new run starts at each corner.
    fn smooth_runs(path: &[crate::PathSeg]) -> Result<Vec<std::ops::Range<usize>>> {
        let ends: Vec<(Vec3, Vec3)> = path
            .iter()
            .map(crate::PathSeg::tangents)
            .collect::<Option<_>>()
            .ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
        let mut runs = Vec::new();
        let mut from = 0;
        for (i, w) in ends.windows(2).enumerate() {
            if w[0].1.dot(w[1].0) < (1e-3f64).cos() {
                runs.push(from..i + 1);
                from = i + 1;
            }
        }
        runs.push(from..path.len());
        Ok(runs)
    }

    /// `pl` carried rigidly by a turn of `angle` about the unit `k` through `center`.
    fn turned_plane(pl: &Plane, center: Vec3, k: Vec3, angle: f64) -> Plane {
        Plane { origin: center + turned(pl.origin - center, k, angle), x: turned(pl.x, k, angle), y: turned(pl.y, k, angle) }
    }

    fn moved_plane(pl: &Plane, by: Vec3) -> Plane {
        Plane { origin: pl.origin + by, ..*pl }
    }

    /// Where `s` starts and ends.
    fn seg_ends(s: &crate::PathSeg) -> Result<(Vec3, Vec3)> {
        Ok(match *s {
            crate::PathSeg::Line { a, b } => (a, b),
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized().ok_or_else(|| KernelError::Invalid("path arc axis".into()))?;
                (a, crate::build::rotate_about(a, center, k, angle))
            }
            crate::PathSeg::Curve { plane, seg } => (plane.to_world(seg.start()), plane.to_world(seg.end())),
            crate::PathSeg::Bezier(b) => (b.start(), b.end()),
        })
    }

    /// The mitre plane at a sharp corner between two smooth runs: the corner point `at`, the
    /// unit `normal` halving the corner (`(t1 + t2).normalized()`, pointing from the incoming run
    /// into the outgoing run), the unit corner `axis` (`(t1 × t2).normalized()`, perpendicular to
    /// `normal`, `t1`, and `t2`), the in-plane `binormal` (`normal × axis`), the cosine of half
    /// the turn (`t1 · normal = t2 · normal`), and the turn angle in radians.
    #[derive(Clone, Copy)]
    struct CornerMitre {
        at: Vec3,
        normal: Vec3,
        axis: Vec3,
        binormal: Vec3,
        cos_half: f64,
        turn: f64,
    }

    /// The affine frame `(origin, x, y)` of `pl` carried by `map`.
    fn mapped_frame(pl: &Plane, map: &Affine) -> (Vec3, Vec3, Vec3) {
        (affine_apply(map, pl.origin), affine_turn(map, pl.x), affine_turn(map, pl.y))
    }

    /// How far a section (`pieces` in the affine frame `(origin, x, y)`) must extend straight
    /// along the run tangent past the corner `m` so every point crosses onto the cut side of the
    /// mitre plane (`+m.normal` at an exit corner, `-m.normal` at an entry corner). Only the
    /// component along `cut_normal` enters the lead, so an out-of-plane profile offset along the
    /// corner axis adds nothing (#23185).
    fn corner_mitre_lead(origin: Vec3, x: Vec3, y: Vec3, pieces: &[Piece], m: CornerMitre, is_exit: bool) -> f64 {
        let cut_normal = if is_exit { m.normal } else { -m.normal };
        let behind = reach_along(origin, x, y, pieces, m.at, -cut_normal).max(0.0);
        let ahead = reach_along(origin, x, y, pieces, m.at, cut_normal).max(0.0);
        let u_span = (reach_along(origin, x, y, pieces, m.at, m.axis) + reach_along(origin, x, y, pieces, m.at, -m.axis)).max(0.0);
        let v_span = (reach_along(origin, x, y, pieces, m.at, m.binormal) + reach_along(origin, x, y, pieces, m.at, -m.binormal)).max(0.0);
        let section_span = (ahead + behind).max(u_span).max(v_span);
        (behind / m.cos_half) * 1.1 + section_span * 1e-3 + 1e-6
    }

    /// What a mitre cuts away: a box on the `+cut_normal` side of the plane through `at` square
    /// to the unit `cut_normal`, fitted tightly in the corner's orthonormal frame
    /// `(cut_normal, axis, cut_normal × axis)` around the corner section (`pieces` in
    /// `(origin, x, y)`) swept along `dir` over both the forward `lead` and the inner-side
    /// crossing back to the mitre plane. Sizing in the corner frame keeps an out-of-plane profile
    /// offset along `axis` from bloating the box across the path's plane (#13543, #23185).
    #[allow(clippy::too_many_arguments)]
    fn mitre_cutter(
        origin: Vec3,
        x: Vec3,
        y: Vec3,
        pieces: &[Piece],
        at: Vec3,
        dir: Vec3,
        cut_normal: Vec3,
        axis: Vec3,
        lead: f64,
        adjacent_arc: Option<(Vec3, Vec3, f64)>,
    ) -> Result<Brep> {
        let n = cut_normal;
        let u = axis;
        let v = n.cross(u);
        let cos_half = dir.dot(n).max(1e-6);
        let ahead = reach_along(origin, x, y, pieces, at, n).max(0.0);
        let back = ahead / cos_half;

        let u_min = -reach_along(origin, x, y, pieces, at, -u);
        let u_max = reach_along(origin, x, y, pieces, at, u);
        let dv = dir.dot(v);
        let s_v_min = (-back * dv).min(lead * dv);
        let s_v_max = (-back * dv).max(lead * dv);
        let v_min = -reach_along(origin, x, y, pieces, at, -v) + s_v_min;
        let v_max = reach_along(origin, x, y, pieces, at, v) + s_v_max;
        let n_max = ahead + lead * cos_half;

        // When the adjacent segment is a circular arc, bound how far rotating the corner section
        // around the arc's axis over `back / radius` can move it in each cutter coordinate. For a
        // coplanar arc (`arc_axis ∥ u`), `u_perp` and `u × arc_axis` are both zero, so `arc_pad_u`
        // stays zero and straight/coplanar runs remain tightly bounded along `u`.
        let (arc_pad_u, arc_pad_v, arc_pad_n) = match adjacent_arc {
            Some((center, arc_axis, arc_angle)) if back > 0.0 => {
                let radius = at.dist(center);
                match arc_axis.normalized() {
                    Some(k) if radius > 0.0 => {
                        let theta = ((back / radius) * 1.1).min(arc_angle.abs());
                        let one_minus_cos = if theta >= std::f64::consts::PI { 2.0 } else { 1.0 - theta.cos() };
                        let sin_bound = if theta >= std::f64::consts::FRAC_PI_2 { 1.0 } else { theta.sin() };
                        let arc_disp = |w: Vec3| -> f64 {
                            let w_perp = w - k * w.dot(k);
                            let w_cross = w.cross(k);
                            let r_w = if w_perp.len() > 1e-12 {
                                reach_along(origin, x, y, pieces, center, w_perp).abs().max(reach_along(origin, x, y, pieces, center, -w_perp).abs())
                            } else {
                                0.0
                            };
                            let c_w = if w_cross.len() > 1e-12 {
                                reach_along(origin, x, y, pieces, at, w_cross).abs().max(reach_along(origin, x, y, pieces, at, -w_cross).abs())
                            } else {
                                0.0
                            };
                            one_minus_cos * r_w + sin_bound * c_w
                        };
                        (arc_disp(u), arc_disp(v), arc_disp(n))
                    }
                    _ => (0.0, 0.0, 0.0),
                }
            }
            _ => (0.0, 0.0, 0.0),
        };
        let pad_u = 0.05 * (u_max - u_min).max(0.0) + arc_pad_u + 1e-3;
        let pad_v = 0.05 * (v_max - v_min).max(0.0) + 0.05 * (lead + back) + arc_pad_v + 1e-3;
        let pad_n = 0.05 * n_max.max(0.0) + arc_pad_n + 1e-3;

        let (u0, u1) = (u_min - pad_u, u_max + pad_u);
        let (v0, v1) = (v_min - pad_v, v_max + pad_v);
        let depth = n_max + pad_n;
        let c = [at + u * u0 + v * v0, at + u * u1 + v * v0, at + u * u1 + v * v1, at + u * u0 + v * v1];
        let rect: Vec<Curve> = (0..4).map(|i| Curve::Line { start: p(c[i]), end: p(c[(i + 1) % 4]) }).collect();
        Brep::extrude(&rect, p(n), depth, 0.0, DraftCorner::default(), true).map_err(|e| failed("mitre", e))
    }

    /// Center, axis, and sweep angle of `s` when it is a circular arc, else `None`.
    fn arc_spec(s: &crate::PathSeg) -> Option<(Vec3, Vec3, f64)> {
        match *s {
            crate::PathSeg::Arc { center, axis, angle, .. } => Some((center, axis, angle)),
            _ => None,
        }
    }

    fn combined(a: &Brep, b: &Brep, op: solidcraft_smlib::BooleanOp, what: &str) -> Result<Brep> {
        a.boolean(b, op).map_err(|e| failed(what, e))?.ok_or_else(|| failed(what, "nothing is left"))
    }

    /// The union of the runs' pieces, in a balanced tree ([`union_tree`]) of the plain union.
    fn joined_runs(parts: Vec<Brep>) -> Result<Brep> {
        let held: Vec<(Brep, usize)> = parts.into_iter().map(|b| (b, 1)).collect();
        let joined = union_tree(held, |(a, m), (b, n)| {
            #[cfg(test)]
            UNION_LOAD.with(|l| l.set(l.get() + m + n));
            Ok((combined(&a, &b, solidcraft_smlib::BooleanOp::Union, "sweep corner")?, m + n))
        })?;
        joined.map(|(b, _)| b).ok_or_else(|| KernelError::Invalid("empty path".into()))
    }

    /// How far the profile `pieces` in the affine frame `(origin, x, y)` reaches from `from`
    /// along the unit `d`: exactly for lines and arcs, and at most as far as its control points
    /// for a free-form piece (a Bézier or conic lies within them), so never less than it does.
    fn reach_along(origin: Vec3, x: Vec3, y: Vec3, pieces: &[Piece], from: Vec3, d: Vec3) -> f64 {
        let base = (origin - from).dot(d);
        let d2 = Vec2::new(d.dot(x), d.dot(y));
        let along = |q: Vec2| base + q.x * d2.x + q.y * d2.y;
        pieces
            .iter()
            .map(|p| match p.seg {
                Seg2::Line { a, b } => along(a).max(along(b)),
                Seg2::Arc { center, radius, start, sweep } => {
                    let ends = along(p.seg.start()).max(along(p.seg.end()));
                    let at = d2.y.atan2(d2.x);
                    let delta =
                        if sweep >= 0.0 { (at - start).rem_euclid(std::f64::consts::TAU) } else { (start - at).rem_euclid(std::f64::consts::TAU) };
                    match d2.normalized() {
                        Some(u) if delta <= sweep.abs() => ends.max(along(center + u * radius)),
                        _ => ends,
                    }
                }
                Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().map(along).fold(f64::MIN, f64::max),
                Seg2::Conic { a, apex, b, .. } => [a, apex, b].into_iter().map(along).fold(f64::MIN, f64::max),
                Seg2::Bezier { n, ref p } => p.iter().take(usize::from(n) + 1).copied().map(along).fold(f64::MIN, f64::max),
            })
            .fold(f64::MIN, f64::max)
    }

    /// Which segments of `route` are bends the profile on `plane`, carried along the route turning
    /// with it, reaches past the centre of: arcs whose axis the section crosses. SMLib sweeps an
    /// arc as a turn about its axis, and a section across that axis makes a body that passes
    /// through itself; a section that only touches the axis (a tube as wide as its bend) it sweeps
    /// whole. The section's place at each arc is exact past lines and arcs ([`carried_along`]);
    /// past another curve it is unknown, and an arc there counts as reached when the profile
    /// reaches further than its radius from the route's start in any direction.
    fn bends_reached(plane: &Plane, pieces: &[Piece], route: &[crate::PathSeg]) -> Vec<bool> {
        let start = route.first().map(crate::PathSeg::start).unwrap_or(plane.origin);
        let reach = pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start))
            .fold(0.0f64, f64::max);
        let mut along = 0.0;
        route
            .iter()
            .map(|s| {
                let reached = match *s {
                    crate::PathSeg::Arc { a, center, .. } => {
                        let radius = a.dist(center);
                        let tol = 1e-9 * (1.0 + radius);
                        match ((center - a).normalized(), carried_along(route, along)) {
                            (Some(u), Some(m)) => {
                                let pl = placed(plane, &m);
                                reach_along(pl.origin, pl.x, pl.y, pieces, a, u) > radius + tol
                            }
                            _ => reach > radius + tol,
                        }
                    }
                    _ => false,
                };
                along += seg_len(s);
                reached
            })
            .collect()
    }

    /// The pieces of the profile on `plane` on each side of the line through `center` square to
    /// the unit `toward` in the plane: first the side away from `toward`, then the side it points
    /// to; either may be empty. Each piece keeps the profile segments it is a part of; a piece
    /// along the line is no profile segment's.
    fn halves_across(plane: &Plane, pieces: &[Piece], center: Vec3, toward: Vec3) -> Result<[Vec<Piece>; 2]> {
        let bad = || KernelError::Invalid("the profile does not close".into());
        // The profile's loops, outer first, as it was split into pieces loop after loop.
        let mut loops: Vec<Loop2> = Vec::new();
        let mut open: Vec<Seg2> = Vec::new();
        for x in pieces {
            open.push(x.seg);
            if let Some(first) = open.first()
                && x.seg.end().dist(first.start()) <= 1e-9 * (1.0 + first.start().len())
            {
                loops.push(Loop2 { segs: std::mem::take(&mut open) });
            }
        }
        if !open.is_empty() || loops.is_empty() {
            return Err(bad());
        }
        let mut loops = loops.into_iter();
        let outer = loops.next().ok_or_else(bad)?;
        let region = Region2 { outer, holes: loops.collect() };
        // How far the profile reaches from the plane's origin, whole segments not only their
        // starts: an arc's circle, a curve's control points (it lies in their hull).
        let size = pieces
            .iter()
            .map(|x| match x.seg {
                Seg2::Line { a, b } => a.len().max(b.len()),
                Seg2::Arc { center, radius, .. } => center.len() + radius.abs(),
                Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().map(Vec2::len).fold(0.0, f64::max),
                Seg2::Conic { a, apex, b, .. } => [a, apex, b].into_iter().map(Vec2::len).fold(0.0, f64::max),
                Seg2::Bezier { n, ref p } => p.iter().take(usize::from(n) + 1).map(|q| q.len()).fold(0.0, f64::max),
            })
            .filter(|s| s.is_finite())
            .fold(1.0, f64::max);
        let c = plane.to_local(center);
        let u = Vec2::new(toward.dot(plane.x), toward.dot(plane.y)).normalized().ok_or_else(|| KernelError::Invalid("bend axis".into()))?;
        let v = Vec2::new(-u.y, u.x);
        let far = 4.0 * (size + c.len()) + 1.0;
        let mut out: [Vec<Piece>; 2] = [Vec::new(), Vec::new()];
        for (side, sign) in out.iter_mut().zip([-1.0, 1.0]) {
            let w = u * (sign * far);
            let mut corners = [c + v * far, c - v * far, c - v * far + w, c + v * far + w];
            let mut half = Loop2::polygon(&corners);
            if half.signed_area() < 0.0 {
                corners.reverse();
                half = Loop2::polygon(&corners);
            }
            let cut = crate::prism::planar_regions_boolean(
                std::slice::from_ref(&region),
                &[Region2 { outer: half, holes: Vec::new() }],
                crate::BoolOp::Intersect,
                1e-7,
            )?;
            for seg in cut.iter().flat_map(|r| r.loops()).flat_map(|l| l.segs.iter().copied()) {
                let mid = seg.point_at(0.5);
                let from = pieces
                    .iter()
                    .map(|x| (x, x.seg.point_at(x.seg.closest_param(mid)).dist(mid)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .filter(|(_, d)| *d <= 1e-6 * size);
                side.push(match from {
                    Some((x, _)) => Piece { seg, numbers: x.numbers.clone(), drawn: x.drawn, joins_previous: false },
                    None => Piece { seg, numbers: Vec::new(), drawn: seg, joins_previous: false },
                });
            }
        }
        Ok(out)
    }

    /// [`swept`] turning with `route`, made whole where the profile reaches past the centre of a
    /// bend ([`bends_reached`]): there the route is swept in parts and the parts joined. Each
    /// stretch between such bends is swept as one; at each such bend the profile is cut along the
    /// bend's axis and each side turned about the axis on its own, which SMLib takes, the
    /// sides wrapping round the axis from opposite ends. A side touches the axis only along a
    /// line, so the sides join the stretch before them first (whose end is the whole profile),
    /// then the stretches join one another. The sweep along a closed path whose sides meet comes
    /// out filled. Moved only (`translate`), the profile never turns about a bend.
    fn swept_run(plane: &Plane, pieces: &[Piece], route: &[crate::PathSeg], translate: bool) -> Result<(Brep, [Affine; 3])> {
        let reached = if translate { Vec::new() } else { bends_reached(plane, pieces, route) };
        if !reached.iter().any(|r| *r) {
            return swept(plane, pieces, route, translate);
        }
        let mut pose = *plane;
        let mut total = IDENTITY;
        // The motion half way along the first stretch, else half way round the first bend.
        let (mut middle, mut bend_middle) = (None, None);
        // Each stretch, then the sides of the bends after it; the sides of bends before the first.
        // Each part ranked: 0 a stretch, 1 a bend's side nearer the path, 2 its other side.
        let mut groups: Vec<Vec<(u8, Brep)>> = Vec::new();
        let mut lead: Vec<(u8, Brep)> = Vec::new();
        let mut i = 0;
        while let Some(seg) = route.get(i) {
            if reached.get(i).copied().unwrap_or(false) {
                let crate::PathSeg::Arc { a, center, axis, angle } = *seg else { return Err(KernelError::Internal("a bend is not an arc".into())) };
                let k = axis.normalized().ok_or_else(|| KernelError::Invalid("path arc axis".into()))?;
                let toward = (center - a).normalized().ok_or_else(|| KernelError::Invalid("path arc radius".into()))?;
                let (k_turn, turn) = if angle < 0.0 { (-k, -angle) } else { (k, angle) };
                let mut sides = Vec::with_capacity(2);
                for (rank, side) in (1u8..).zip(halves_across(&pose, pieces, center, toward)?) {
                    if !side.is_empty() {
                        let c = curves(&pose, &side)?;
                        let turned =
                            Brep::revolve(&c, p(center), p(k_turn), turn.min(std::f64::consts::TAU), true).map_err(|e| failed("sweep", e))?;
                        sides.push((rank, turned));
                    }
                }
                match groups.last_mut() {
                    Some(g) => g.extend(sides),
                    None => lead.extend(sides),
                }
                bend_middle.get_or_insert_with(|| affine_after(&affine_turned(center, k, angle * 0.5), &total));
                pose = turned_plane(&pose, center, k, angle);
                total = affine_after(&affine_turned(center, k, angle), &total);
                i += 1;
            } else {
                let j = i + reached.iter().skip(i).take_while(|r| !**r).count();
                let stretch = route.get(i..j).ok_or_else(|| KernelError::Internal("path stretch".into()))?;
                let (brep, [_, mid, last]) = swept(&pose, pieces, stretch, false)?;
                middle.get_or_insert_with(|| affine_after(&mid, &total));
                groups.push(vec![(0, brep)]);
                pose = placed(&pose, &last);
                total = affine_after(&last, &total);
                i = j;
            }
        }
        // Sides of bends the route starts with join the stretch they meet: the last, round a
        // closed route, else the first.
        let closed = match (route.first(), route.last()) {
            (Some(f), Some(l)) => l.end().dist(f.start()) <= 1e-6,
            _ => false,
        };
        match (closed, groups.last_mut()) {
            (true, Some(g)) => g.append(&mut lead),
            _ => match groups.first_mut() {
                Some(g) => g.append(&mut lead),
                None => groups.push(lead),
            },
        }
        // In each group the stretch first, then the sides nearer the path, then the others: each
        // part joins what it meets face to face (the sides of one bend meet only along its axis).
        let mut joined = Vec::with_capacity(groups.len());
        for mut group in groups {
            group.sort_by_key(|(rank, _)| *rank);
            let mut it = group.into_iter().map(|(_, b)| b);
            let Some(mut acc) = it.next() else { continue };
            for b in it {
                acc = combined(&acc, &b, solidcraft_smlib::BooleanOp::Union, "sweep bend")?;
            }
            joined.push(acc);
        }
        let brep = joined_runs(joined)?;
        let middle = middle.or(bend_middle).unwrap_or(IDENTITY);
        Ok((brep, [IDENTITY, middle, total]))
    }

    /// A sweep along a path with corners: each smooth run of it swept on its own, and the runs
    /// joined at each corner. Turning with the path, the profile turns at a corner about the
    /// line square to both directions, and the runs meet in a mitre: each run is swept on
    /// straight past the corner (along its direction there) and cut back to the plane that halves
    /// the corner, where the next run, cut back the same way, meets it face to face. Only moving
    /// along the path, the profile is the same at the corner in both runs, which meet there.
    ///
    /// History: the start cap is the face in the profile's plane, the end cap the face in its
    /// plane at the path's end; each wall comes from the profile segment it is swept from, found
    /// on the first run's start or half-way along each run.
    fn cornered(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], runs: &[std::ops::Range<usize>], translate: bool) -> Result<Body> {
        let start = path.first().map(seg_ends).transpose()?.map(|(a, _)| a).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        // How far the profile reaches from the path.
        let reach = pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start))
            .fold(0.0f64, f64::max);
        let tangent = |s: Option<&crate::PathSeg>| {
            s.and_then(crate::PathSeg::tangents).ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))
        };
        let (first_t, _) = tangent(path.first())?;
        let offsets = section_offsets(plane, pieces, start, first_t, 1.0);
        let (_, last_t) = tangent(path.last())?;
        let end = path.last().map(seg_ends).transpose()?.map(|(_, b)| b).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        // A closed path ends where it starts, with no caps; at a corner there the last run and the
        // first meet in a mitre too.
        let closed = end.dist(start) <= 1e-6;
        let closing = (closed && last_t.dot(first_t) < (1e-3f64).cos()).then_some(first_t);
        let mut pose = *plane;
        let mut parts: Vec<Brep> = Vec::with_capacity(runs.len());
        let mut inside = Vec::new();
        // The mitre the next run starts from: its point, its normal (along the run), its corner
        // axis and the turn.
        let mut entry: Option<CornerMitre> = match closing {
            Some(_) if !translate => Some(mitre_of(start, last_t, first_t)?),
            _ => None,
        };
        for (ri, run) in runs.iter().enumerate() {
            let segs = path.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?;
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(KernelError::Invalid("path run".into())) };
            let (t0, _) = first.tangents().ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
            let (_, t1) = last.tangents().ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?;
            let (a0, _) = seg_ends(first)?;
            let (_, b1) = seg_ends(last)?;
            let next = match runs.get(ri + 1).and_then(|r| path.get(r.start)) {
                Some(s) => Some(tangent(Some(s))?.0),
                None => closing,
            };
            // The plane halving the corner at the run's end, its corner axis, and the turn there.
            let exit = next.map(|t2| mitre_of(b1, t1, t2)).transpose()?;
            // Each piece's sweep says where it left the profile: half-way along it each wall lies
            // over its piece, and at its end (less any straight lead-out past the corner) the
            // profile is where the next run takes it on.
            let (piece, at_end) = if translate {
                let (piece, [_, middle, last]) = swept(&pose, pieces, segs, true)?;
                inside.extend(seeds(&pose, pieces, |q| apply(&middle, q)));
                inside.extend(segment_seeds(&pose, pieces, segs, 0..segs.len(), true));
                (piece, placed(&pose, &last))
            } else {
                // On past each mitre, far enough to take every point of the section to the
                // mitre's plane. Use the corner's own frame whenever the pose there is known
                // analytically (lines and arcs), so an out-of-plane profile offset along the
                // corner axis does not inflate the lead (#23185).
                let run_length: f64 = segs.iter().map(seg_len).sum();
                let pre = entry.map_or(0.0, |m| corner_mitre_lead(pose.origin, pose.x, pose.y, pieces, m, false));
                let predicted_exit = carried_along(segs, run_length).map(|m| placed(&pose, &m));
                let post = exit.map_or(0.0, |m| match predicted_exit {
                    Some(end_pose) => corner_mitre_lead(end_pose.origin, end_pose.x, end_pose.y, pieces, m, true),
                    None => mitre_lead(&offsets, reach, m.turn, true),
                });
                let mut route = Vec::with_capacity(segs.len() + 2);
                if pre > 0.0 {
                    route.push(crate::PathSeg::Line { a: a0 - t0 * pre, b: a0 });
                }
                route.extend_from_slice(segs);
                if post > 0.0 {
                    route.push(crate::PathSeg::Line { a: b1, b: b1 + t1 * post });
                }
                let from = moved_plane(&pose, t0 * -pre);
                let (mut piece, [_, middle, last_affine]) = swept_run(&from, pieces, &route, false)?;
                let exit_pose = moved_plane(&placed(&from, &last_affine), t1 * -post);
                // Each wall is known by its piece's middle half way along the stretch of the run
                // that point keeps between the mitres: a point standing `s` ahead of the path
                // keeps the stretch `s` back from the run's (a profile at a slant to a run shorter
                // than its slant reaches past the mitres from the run's middle section).
                let origin = a0 - t0 * pre;
                inside.extend(pieces.iter().enumerate().map(|(piece, x)| {
                    let q = from.to_world(x.seg.point_at(0.5));
                    let ahead = (q - origin).dot(t0);
                    let at = carried_along(&route, pre + 0.5 * run_length - ahead).map_or_else(|| apply(&middle, q), |m| apply(&m, q));
                    Seed { at, piece }
                }));
                let lead = usize::from(pre > 0.0);
                inside.extend(segment_seeds(&from, pieces, &route, lead..lead + segs.len(), false));
                if let Some(m) = entry {
                    let cut = mitre_cutter(pose.origin, pose.x, pose.y, pieces, m.at, -t0, -m.normal, m.axis, pre, arc_spec(first))?;
                    piece = combined(&piece, &cut, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                if let Some(m) = exit {
                    let cut = mitre_cutter(exit_pose.origin, exit_pose.x, exit_pose.y, pieces, m.at, t1, m.normal, m.axis, post, arc_spec(last))?;
                    piece = combined(&piece, &cut, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                (piece, exit_pose)
            };
            parts.push(piece);
            // The profile at the run's end, then turned through the corner onto the next run.
            pose = at_end;
            entry = None;
            if let (Some(_), Some(m), false) = (next, exit, translate) {
                pose = turned_plane(&pose, m.at, m.axis, m.turn);
                entry = Some(m);
            }
        }
        let brep = joined_runs(parts)?;
        let body = body_of(brep, "sweep")?;
        let roles = roles(&body, plane, (!closed).then_some(&pose), &seeds(plane, pieces, |q| q), &inside, false)?;
        with_history(body, &roles, pieces)
    }

    /// Each piece's middle carried half way along each segment of `route` in `segments` (the
    /// profile on `plane` where the route starts): on the wall the piece sweeps along that
    /// segment, wherever it is carried to, since the motion is the sweep's own (moved only, when
    /// `translate`; else as [`carried_along`] carries it, so none past a segment that is not a
    /// line or an arc). A route of one segment has its walls known by its middle already.
    fn segment_seeds(plane: &Plane, pieces: &[Piece], route: &[crate::PathSeg], segments: std::ops::Range<usize>, translate: bool) -> Vec<Seed> {
        let Some((origin, t0)) = route.first().and_then(|s| seg_at(s, 0.0)) else { return Vec::new() };
        if route.len() < 2 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut before = 0.0;
        for (j, s) in route.iter().enumerate() {
            let length = seg_len(s);
            let middle = before + 0.5 * length;
            before += length;
            if !segments.contains(&j) || !middle.is_finite() {
                continue;
            }
            for (piece, x) in pieces.iter().enumerate() {
                let q = plane.to_world(x.seg.point_at(0.5));
                // A point standing ahead of the path's start keeps that lead along it.
                let motion = if translate {
                    seg_at(s, 0.5).map(|(at, _)| affine_moved(&IDENTITY, at - origin))
                } else {
                    carried_along(route, middle - solidcore_ops::affine::distance_along(origin.into(), t0.into(), q.into()))
                };
                out.extend(motion.map(|m| Seed { at: apply(&m, q), piece }));
            }
        }
        out
    }

    /// The rigid motion carrying a section from the start of `route` to `along` (clamped to the
    /// route) along it, as SMLib's sweep turning with a path of lines and arcs carries it: moved
    /// along each line, turned about each arc's axis. None for a route with another curve before
    /// that point, whose frame only the sweep knows.
    fn carried_along(route: &[crate::PathSeg], along: f64) -> Option<Affine> {
        let mut m = IDENTITY;
        let mut left = along.max(0.0);
        for s in route {
            if left <= 0.0 {
                break;
            }
            let length = seg_len(s);
            let d = left.min(length);
            m = match *s {
                crate::PathSeg::Line { a, b } => affine_moved(&m, (b - a).normalized()? * d),
                crate::PathSeg::Arc { center, axis, angle, .. } if length > 0.0 => {
                    affine_after(&affine_turned(center, axis.normalized()?, angle * d / length), &m)
                }
                _ => return None,
            };
            left -= d;
        }
        Some(m)
    }

    /// The profile's `plane` where the sweep along `path` starts. A profile drawn part way along
    /// the path (its plane missing the path's start but crossing the path) is carried back from
    /// the crossing nearest it to the start, the way the sweep would carry it forward (moved
    /// only, when `translate`), so the swept body follows the whole path and passes through the
    /// profile where it lies. SMLib sweeps from the path's start only. A profile whose plane misses
    /// the path altogether is carried back the same way from the point of the path nearest it: it
    /// travels beside the whole path, square to it, keeping its distance and side.
    fn at_path_start(plane: &Plane, pieces: &[Piece], path: &[crate::PathSeg], translate: bool) -> Result<Plane> {
        let Some(start) = path.first().map(crate::PathSeg::start) else { return Ok(*plane) };
        let lens: Vec<f64> = path.iter().map(seg_len).collect();
        let total: f64 = lens.iter().sum();
        if !(total > 0.0 && total.is_finite()) || plane.height(start).abs() <= (total * 1e-9).max(1e-9) {
            return Ok(*plane);
        }
        let mids: Vec<Vec3> = pieces.iter().map(|x| plane.to_world(x.seg.point_at(0.5))).collect();
        let centre = mids.iter().fold(Vec3::ZERO, |s, q| s + *q) * (1.0 / mids.len().max(1) as f64);
        // Each crossing as (its point, the length along the path to it).
        let mut crossings: Vec<(Vec3, f64)> = Vec::new();
        let mut before = 0.0;
        for (s, len) in path.iter().zip(&lens) {
            let height = |f: f64| seg_at(s, f).map(|(q, _)| plane.height(q));
            for i in 0..CROSSING_SAMPLES {
                let (mut lo, mut hi) = (i as f64 / CROSSING_SAMPLES as f64, (i + 1) as f64 / CROSSING_SAMPLES as f64);
                let (Some(h_lo), Some(h_hi)) = (height(lo), height(hi)) else { continue };
                if h_lo * h_hi > 0.0 || (h_lo == 0.0 && i > 0) {
                    continue;
                }
                let rising = h_lo < h_hi;
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    match height(mid) {
                        Some(h) if (h < 0.0) == rising => lo = mid,
                        Some(_) => hi = mid,
                        None => break,
                    }
                }
                let f = 0.5 * (lo + hi);
                if let Some((q, _)) = seg_at(s, f) {
                    crossings.push((q, before + f * len));
                }
            }
            before += len;
        }
        let nearest = crossings.into_iter().min_by(|a, b| a.0.dist(centre).total_cmp(&b.0.dist(centre)));
        let Some((at, along)) = nearest.or_else(|| nearest_on_path(path, &lens, centre)) else {
            return Ok(*plane);
        };
        let forward = if translate {
            affine_moved(&IDENTITY, at - start)
        } else {
            carried_through(path, along).ok_or_else(|| KernelError::Invalid("a path segment has no direction".into()))?
        };
        Ok(placed(plane, &affine_inverse(&forward)))
    }

    /// The point of `path` (its segments `lens` long) nearest `q`, and the length along the path
    /// to it: the nearest of points read along each segment, then narrowed between its neighbours.
    fn nearest_on_path(path: &[crate::PathSeg], lens: &[f64], q: Vec3) -> Option<(Vec3, f64)> {
        let mut best: Option<(Vec3, f64, f64)> = None;
        let mut before = 0.0;
        for (s, len) in path.iter().zip(lens) {
            let dist = |f: f64| seg_at(s, f).map_or(f64::INFINITY, |(p, _)| p.dist(q));
            let n = CROSSING_SAMPLES as f64;
            let Some(i) = (0..=CROSSING_SAMPLES).min_by(|a, b| dist(*a as f64 / n).total_cmp(&dist(*b as f64 / n))) else { continue };
            let (mut lo, mut hi) = (((i as f64) - 1.0).max(0.0) / n, ((i as f64) + 1.0).min(n) / n);
            for _ in 0..60 {
                let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                if dist(a) < dist(b) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            let f = 0.5 * (lo + hi);
            if let Some((p, _)) = seg_at(s, f)
                && best.is_none_or(|(_, _, d)| p.dist(q) < d)
            {
                best = Some((p, before + f * len, p.dist(q)));
            }
            before += len;
        }
        best.map(|(p, along, _)| (p, along))
    }

    /// The rigid motion carrying a section from the start of `path` to `along` (clamped to the
    /// path) along it, turning with it: [`carried_along`] on lines and arcs, turned through each
    /// joint by the angle between the directions there (as [`cornered`] turns it round a corner),
    /// and along another curve turned through its directions of travel step by step.
    fn carried_through(path: &[crate::PathSeg], along: f64) -> Option<Affine> {
        const STEPS: usize = 256;
        let mut m = IDENTITY;
        let mut left = along.max(0.0);
        let mut last: Option<Vec3> = None;
        for s in path {
            if left <= 0.0 {
                break;
            }
            let (t0, _) = s.tangents()?;
            if let Some(t) = last {
                m = affine_after(&turn_between(s.start(), t, t0), &m);
            }
            let length = seg_len(s);
            let d = left.min(length);
            m = match *s {
                crate::PathSeg::Line { .. } | crate::PathSeg::Arc { .. } => affine_after(&carried_along(std::slice::from_ref(s), d)?, &m),
                _ if length > 0.0 => {
                    let end = d / length;
                    let (mut q, mut t) = seg_at(s, 0.0)?;
                    for i in 1..=STEPS {
                        let (q1, t1) = seg_at(s, end * i as f64 / STEPS as f64)?;
                        m = affine_after(&turn_between(q1, t, t1), &affine_moved(&m, q1 - q));
                        (q, t) = (q1, t1);
                    }
                    m
                }
                _ => m,
            };
            last = seg_at(s, if length > 0.0 { d / length } else { 1.0 }).map(|(_, t)| t);
            left -= d;
        }
        Some(m)
    }

    /// The least turn about a line through `at` taking the unit direction `from` to `to`.
    fn turn_between(at: Vec3, from: Vec3, to: Vec3) -> Affine {
        let k = from.cross(to);
        match k.normalized() {
            Some(axis) if k.len() > 1e-12 => affine_turned(at, axis, k.len().atan2(from.dot(to))),
            _ => IDENTITY,
        }
    }

    /// The inverse of the rigid motion `a`.
    fn affine_inverse(a: &Affine) -> Affine {
        let t = Vec3::new(a.t[0], a.t[1], a.t[2]);
        let col = |i: usize| Vec3::new(a.m[i][0], a.m[i][1], a.m[i][2]);
        // The inverse turn is the transpose: its columns are the turn's rows.
        let m = [0, 1, 2].map(|j| [a.m[0][j], a.m[1][j], a.m[2][j]]);
        Affine { m, t: [-col(0).dot(t), -col(1).dot(t), -col(2).dot(t)] }
    }

    /// The length of a path segment.
    fn seg_len(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { a, b } => a.dist(b),
            crate::PathSeg::Arc { a, center, angle, .. } => angle.abs() * a.dist(center),
            crate::PathSeg::Curve { seg, .. } => seg.length(),
            crate::PathSeg::Bezier(b) => b.length().unwrap_or(f64::NAN),
        }
    }

    /// The point `f` (0…1) of the way along a path segment, and the unit direction of travel there.
    fn seg_at(s: &crate::PathSeg, f: f64) -> Option<(Vec3, Vec3)> {
        match *s {
            crate::PathSeg::Line { a, b } => Some((a.lerp(b, f), (b - a).normalized()?)),
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized()?;
                let q = crate::build::rotate_about(a, center, k, angle * f);
                Some((q, (k.cross(q - center) * angle.signum()).normalized()?))
            }
            // The parameter whose length from the start is that share of the curve's.
            crate::PathSeg::Curve { plane, seg } => {
                let want = seg.length() * f;
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for _ in 0..48 {
                    let t = 0.5 * (lo + hi);
                    if seg.split_at(t).0.length() < want {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                let t = 0.5 * (lo + hi);
                Some((plane.to_world(seg.point_at(t)), plane.dir_to_world(seg.derivative(t)).normalized()?))
            }
            crate::PathSeg::Bezier(b) => b.along(f),
        }
    }

    /// `n + 1` points evenly spaced by length along `path` (exact on its lines and arcs), with the
    /// unit directions of travel there.
    fn path_samples(path: &[crate::PathSeg], n: usize) -> Result<(Vec<Vec3>, Vec<Vec3>)> {
        let lens: Vec<f64> = path.iter().map(seg_len).collect();
        let total: f64 = lens.iter().sum();
        if !(total > 1e-9 && total.is_finite()) || n == 0 {
            return Err(KernelError::Invalid("the sweep path has no length".into()));
        }
        let bad = || KernelError::Invalid("a path segment has no direction".into());
        let (mut pts, mut tangents) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
        let (mut k, mut before) = (0usize, 0.0f64);
        for i in 0..=n {
            let at = total * i as f64 / n as f64;
            while k + 1 < path.len() && lens.get(k).is_some_and(|l| before + l < at) {
                before += lens.get(k).copied().unwrap_or(0.0);
                k += 1;
            }
            let (seg, len) = path.get(k).zip(lens.get(k)).ok_or_else(bad)?;
            let f = if *len > 0.0 { ((at - before) / len).clamp(0.0, 1.0) } else { 0.0 };
            let (q, t) = seg_at(seg, f).ok_or_else(bad)?;
            pts.push(q);
            tangents.push(t);
        }
        Ok((pts, tangents))
    }

    /// How far the direction of travel turns along a path segment (radians, all told).
    fn path_turn(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { .. } => 0.0,
            crate::PathSeg::Arc { angle, .. } => angle.abs(),
            crate::PathSeg::Curve { seg, .. } => {
                let pts = seg.polyline(1e-3);
                pts.windows(3)
                    .filter_map(|w| Some(((w[1] - w[0]).normalized()?, (w[2] - w[1]).normalized()?)))
                    .map(|(u, v)| u.cross(v).atan2(u.dot(v)).abs())
                    .sum()
            }
            crate::PathSeg::Bezier(b) => {
                let pts: Vec<Vec3> = std::iter::once(b.start()).chain(b.polyline(1e-3)).collect();
                pts.windows(3)
                    .filter_map(|w| Some(((w[1] - w[0]).normalized()?, (w[2] - w[1]).normalized()?)))
                    .map(|(u, v)| u.cross(v).len().atan2(u.dot(v)))
                    .sum()
            }
        }
    }

    /// Fewest and most stations of a sweep that twists, scales or follows rails.
    const MIN_STATIONS: usize = 16;
    /// Directions of travel read along each path segment for the parallel sweep's crossing check.
    const CROSSING_SAMPLES: usize = 32;
    const MAX_STATIONS: usize = 2048;

    /// The profile `region` on `plane` swept along the smooth `path` the way `ctl` asks: twisted,
    /// scaled, kept at an orientation or guided by rails. The profile is placed at stations along
    /// the path by [`crate::sweep_path`]'s laws (the same the polyline sweep uses), each section
    /// exactly the profile's image, and SMLib skins the walls through them. Round a closed path the
    /// last section has to land on the first, and the ends are joined.
    ///
    /// Along a straight path square to the profile, with no end scale or rails, a loop that is a
    /// circle centred on the path's line maps onto itself at every station, and its wall is built
    /// as the exact cylinder: a profile of such circles alone is swept untwisted, and such holes
    /// in another outer loop are cut from the sweep of the rest. Other loops are skinned, and an
    /// arc centred on the line within a loop of other curves still is.
    pub(crate) fn sweep_controlled(plane: &Plane, region: &NumberedRegion, path: &[crate::PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
        let pieces = pieces(region.loops());
        // A profile drawn away from the path's start is carried there as it is: it is the start
        // section, and the twist and the scale run from there.
        let parallel = ctl.orientation == crate::SweepOrientation::Parallel;
        let plane = &at_path_start(plane, &pieces, path, parallel)?;
        if !parallel {
            turning_check(plane, &pieces, path)?;
        }
        let runs = smooth_runs(path)?;
        if runs.len() > 1 {
            return cornered_controlled(plane, region, path, &runs, ctl);
        }
        let start = path.first().map(crate::PathSeg::start).ok_or_else(|| KernelError::Invalid("the path needs 1…1000 segments".into()))?;
        // A circle centred on the twist axis, swept straight along that axis, turns onto itself:
        // its wall is exactly the cylinder, built as one rather than skinned (solidcore#2184: a
        // skin lying within 4e-9 of a cylinder another body shares fails the boolean).
        let axial = straight_axis(plane, path, ctl, start).map(|(k, length)| {
            let round: Vec<Option<(Vec3, f64)>> = region.loops().map(|l| circle_on_axis(plane, &l.lp, start, k)).collect();
            (k, length, round)
        });
        let mut bores: Vec<(Vec3, f64)> = Vec::new();
        let mut kept = region.clone();
        if let Some((_, _, round)) = &axial {
            if round.iter().all(Option::is_some) {
                // Every loop a circle about the axis: the twist changes nothing.
                match sweep_from_start(plane, &pieces, path, false) {
                    Ok(body) => return Ok(body),
                    Err(e) => log::debug!(target: "smlib", "twisted sweep of circles about its path, swept untwisted: {e}; skinned instead"),
                }
            } else if round.first().is_some_and(Option::is_none) {
                // The holes that are circles about the axis are cut as exact cylinders from the
                // sweep of the rest, through and a little past both ends.
                let (mut holes, mut cut) = (Vec::new(), Vec::new());
                for (h, r) in region.holes.iter().zip(round.iter().skip(1)) {
                    match r {
                        Some(circle) => cut.push(*circle),
                        None => holes.push(h.clone()),
                    }
                }
                if !cut.is_empty() {
                    bores = cut;
                    kept = NumberedRegion { outer: region.outer.clone(), holes };
                }
            }
        }
        let skinned = skinned_controlled(plane, &kept, path, ctl, start, &bores, axial.as_ref().map(|(k, l, _)| (*k, *l)));
        let (body, st, size, closed) = match skinned {
            Ok(r) => r,
            Err(e) if !bores.is_empty() => {
                log::debug!(target: "smlib", "twisted sweep with its bores cut as cylinders: {e}; all skinned instead");
                skinned_controlled(plane, region, path, ctl, start, &[], None)?
            }
            Err(e) => return Err(e),
        };
        let (Some(first), Some(last)) = (st.first(), st.last()) else { return Err(KernelError::Invalid("sweep path".into())) };
        let s0 = first.at;
        let mid = st.get(st.len() / 2).copied().unwrap_or(*last);
        let inside = seeds(plane, &pieces, |q| mid.at + mid.apply(q - s0));
        let roles = if closed {
            // Round a closed path there are no caps, and the start holds seams only.
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            let end = Plane::new(last.at + last.apply(plane.origin - s0), last.apply(plane.x), last.apply(plane.y))
                .ok_or_else(|| KernelError::Invalid("the sweep's end section is degenerate".into()))?;
            roles(&body, plane, Some(&end), &seeds(plane, &pieces, |q| q), &inside, false)?
        };
        with_history(body, &roles, &pieces)
    }

    /// The unit direction of a path of lines running straight along the profile plane's normal,
    /// and its length, when the sweep's laws only turn the profile about that line through the
    /// path's start (no end scale, no rails): each station is then the profile turned about the
    /// axis and moved along it.
    fn straight_axis(plane: &Plane, path: &[crate::PathSeg], ctl: &crate::SweepControl, start: Vec3) -> Option<(Vec3, f64)> {
        if ctl.scale != 1.0 || !ctl.rails.is_empty() {
            return None;
        }
        let n = plane.normal().normalized()?;
        let mut at = start;
        let mut length = 0.0;
        let mut k: Option<Vec3> = None;
        for s in path {
            let crate::PathSeg::Line { a, b } = *s else { return None };
            let d = b - a;
            let t = d.normalized()?;
            let scale = 1.0 + a.dist(start) + b.dist(start);
            if a.dist(at) > 1e-9 * scale || t.dot(n).abs() < 1.0 - 1e-12 || k.is_some_and(|k| t.dot(k) < 1.0 - 1e-12) {
                return None;
            }
            k.get_or_insert(t);
            length += d.len();
            at = b;
        }
        k.filter(|_| length.is_finite() && length > 0.0).map(|k| (k, length))
    }

    /// The centre and radius of `lp` when it is one circle (in one or several arcs) centred on the
    /// line through `start` along the unit `k`.
    fn circle_on_axis(plane: &Plane, lp: &solidcore_record::Loop2, start: Vec3, k: Vec3) -> Option<(Vec3, f64)> {
        let mut circle: Option<(Vec3, f64)> = None;
        let mut turn = 0.0;
        for s in &lp.segs {
            let Seg2::Arc { center, radius, sweep, .. } = *s else { return None };
            let c = plane.to_world(center);
            let tol = 1e-9 * (1.0 + radius.abs());
            let w = c - start;
            if !(radius.is_finite() && radius > 0.0) || (w - k * w.dot(k)).len() > tol {
                return None;
            }
            if let Some((c0, r0)) = circle
                && (c0.dist(c) > tol || (r0 - radius).abs() > tol)
            {
                return None;
            }
            circle.get_or_insert((c, radius));
            turn += sweep.abs();
        }
        circle.filter(|_| (turn - std::f64::consts::TAU).abs() <= 1e-9 * std::f64::consts::TAU)
    }

    /// The controlled sweep of `region` skinned through its stations, with each of `bores` (a
    /// centre and radius on the straight path's axis `straight`: its direction and length) cut
    /// from it as an exact cylinder; the stations, the sweep's size and whether the path closes.
    fn skinned_controlled(
        plane: &Plane,
        region: &NumberedRegion,
        path: &[crate::PathSeg],
        ctl: &crate::SweepControl,
        start: Vec3,
        bores: &[(Vec3, f64)],
        straight: Option<(Vec3, f64)>,
    ) -> Result<(Body, Vec<crate::sweep_path::Station>, f64, bool)> {
        let c = curves(plane, &pieces(region.loops()))?;
        // Each point of the profile turns with the path and the twist, and SMLib's skin is a cubic
        // interpolation through the stations with free ends, whose error near the ends falls only
        // as the square of the step: a square of reach 7.1 twisted a quarter turn over 40 mm is
        // 2e-7 off in volume at 1.4 degree steps and 2e-6 at 2.8 (measured against V = A L). A
        // station per degree of turn.
        let turn: f64 = path.iter().map(path_turn).sum::<f64>() + ctl.twist.abs();
        let step = std::f64::consts::PI / 180.0;
        let rail_points = ctl.rails.iter().map(Vec::len).max().unwrap_or(0);
        let n = ((turn / step).ceil() as usize).max(rail_points.saturating_sub(1)).clamp(MIN_STATIONS, MAX_STATIONS);
        let (pts, tangents) = path_samples(path, n)?;
        let st = crate::sweep_path::controlled_stations(plane, &pts, &tangents, &[], ctl)?;
        let s0 = st.first().ok_or_else(|| KernelError::Invalid("sweep path".into()))?.at;
        let size = pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() + region.outer.lp.signed_area().abs().sqrt();
        let closed = pts.last().is_some_and(|e| e.dist(start) <= 1e-7 * (1.0 + size));
        if closed {
            for l in region.loops() {
                crate::sweep_path::closing_offset(plane, l, &st)?;
            }
        }
        let maps: Vec<Affine> = st
            .iter()
            .map(|s| {
                let [a, b, c] = s.m;
                Affine { m: [p(a), p(b), p(c)], t: p(s.at - s.apply(s0)) }
            })
            .collect();
        let mut brep = Brep::sweep_stations(&c, &maps, if closed { Ends::Joined } else { Ends::Capped }).map_err(|e| failed("sweep", e))?;
        if let (false, Some((k, length))) = (bores.is_empty(), straight) {
            // Past each end by a share of the sweep's size, so no face of the cut lies on a cap.
            let past = 0.25 * size + 1e-3;
            for (center, radius) in bores {
                let from = *center - k * past;
                let circle =
                    Curve::Arc { center: p(from), axis: p(k), reference: p(plane.x), radius: *radius, start: 0.0, end: std::f64::consts::TAU };
                let bore =
                    Brep::extrude(&[circle], p(k), length + 2.0 * past, 0.0, DraftCorner::default(), true).map_err(|e| failed("sweep bore", e))?;
                brep = combined(&brep, &bore, solidcraft_smlib::BooleanOp::Subtract, "sweep bore")?;
            }
        }
        Ok((body_of(brep, "sweep")?, st, size, closed))
    }

    /// `x` moved by `a`.
    fn affine_apply(a: &Affine, x: Vec3) -> Vec3 {
        let [c0, c1, c2] = a.m;
        let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
        v(c0) * x.x + v(c1) * x.y + v(c2) * x.z + v(a.t)
    }

    /// `v` turned by `a`'s linear part.
    fn affine_turn(a: &Affine, v: Vec3) -> Vec3 {
        affine_apply(a, v) - affine_apply(a, Vec3::ZERO)
    }

    /// `a` after `b`: x goes to a(b(x)).
    fn affine_after(a: &Affine, b: &Affine) -> Affine {
        let col = |c: [f64; 3]| p(affine_turn(a, Vec3::new(c[0], c[1], c[2])));
        Affine { m: [col(b.m[0]), col(b.m[1]), col(b.m[2])], t: p(affine_apply(a, Vec3::new(b.t[0], b.t[1], b.t[2]))) }
    }

    /// The rigid turn by `angle` about the unit `k` through `center`, as an affine map.
    fn affine_turned(center: Vec3, k: Vec3, angle: f64) -> Affine {
        let col = |e: Vec3| p(turned(e, k, angle));
        Affine { m: [col(Vec3::X), col(Vec3::Y), col(Vec3::Z)], t: p(center - turned(center, k, angle)) }
    }

    fn affine_moved(a: &Affine, by: Vec3) -> Affine {
        Affine { m: a.m, t: p(Vec3::new(a.t[0], a.t[1], a.t[2]) + by) }
    }

    const IDENTITY: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    /// A sweep that twists, scales, keeps an orientation direction or follows guide rails along
    /// a path with corners, by the corner model of the plain sweep ([`cornered`]): each smooth run
    /// of the path is swept through stations by the laws a smooth path has (the twist and the
    /// scale running with the length along the whole path, each run going on from the section the
    /// last one ended with), and the runs meet at each corner in a mitre. Turning with the path,
    /// each run is carried on straight past its corners as a prism of its section there (the laws
    /// hold still across a mitre: the two runs' sections meet on the plane halving the corner only
    /// when both are the corner's section), cut back to that plane, and the section turned through
    /// the corner about its binormal; only moving along the path, the runs meet at the corner's
    /// section.
    ///
    /// At a corner an orientation direction turns the section as the corner does, and along the
    /// next run it turns back about the path to the direction's frame, evenly along the run's
    /// length, as a twist. Guide rails must turn with the path: each rail has a corner of its own
    /// at every corner of the path, in the plane halving it and by the same turn, where it is cut
    /// into the runs' rails. Round a closed path the last section has to land on the first: a
    /// twist where the profile maps onto itself; an end scale never does.
    fn cornered_controlled(
        plane: &Plane,
        region: &NumberedRegion,
        path: &[crate::PathSeg],
        runs: &[std::ops::Range<usize>],
        ctl: &crate::SweepControl,
    ) -> Result<Body> {
        let translate = ctl.orientation == crate::SweepOrientation::Parallel;
        let pieces = pieces(region.loops());
        let c = curves(plane, &pieces)?;
        let bad = || KernelError::Invalid("a path segment has no direction".into());
        let start = path.first().map(crate::PathSeg::start).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        let end = path.last().map(crate::PathSeg::end).ok_or_else(|| KernelError::Invalid("empty path".into()))?;
        let lens: Vec<f64> = path.iter().map(seg_len).collect();
        let total: f64 = lens.iter().sum();
        if !(total > 1e-9 && total.is_finite()) {
            return Err(KernelError::Invalid("the sweep path has no length".into()));
        }
        let tangent = |s: Option<&crate::PathSeg>| s.and_then(crate::PathSeg::tangents).ok_or_else(bad);
        let (first_t, _) = tangent(path.first())?;
        let (_, last_t) = tangent(path.last())?;
        let closed = end.dist(start) <= 1e-6;
        if closed && ctl.scale != 1.0 {
            return Err(KernelError::Invalid(
                "a sweep with an end scale can't go round a closed path: its last section would have to land on its first".into(),
            ));
        }
        let closing = (closed && last_t.dot(first_t) < (1e-3f64).cos()).then_some(first_t);
        let reach = pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start) * ctl.scale.max(1.0))
            .fold(0.0f64, f64::max);
        let size = 4.0 * (total + reach) + 1.0;
        let law = |u: f64| 1.0 + (ctl.scale - 1.0) * u;
        let n0 = plane.normal().normalized().ok_or_else(|| KernelError::Invalid("profile plane".into()))?;
        // Each run's rails, cut at the corners.
        let rails = run_rails(path, runs, &ctl.rails, closing)?;
        // The map carrying the profile as drawn onto the section where the next run starts, the
        // mitre that run starts from and how far along the path it starts.
        let mut at_start = IDENTITY;
        let mut entry: Option<CornerMitre> = match closing {
            Some(_) if !translate => Some(mitre_of(start, last_t, first_t)?),
            _ => None,
        };
        let mut before = 0.0f64;
        let mut parts: Vec<Brep> = Vec::with_capacity(runs.len());
        let mut inside = Vec::new();
        for (ri, run) in runs.iter().enumerate() {
            let segs = path.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?;
            let length: f64 = lens.get(run.clone()).ok_or_else(|| KernelError::Invalid("path run".into()))?.iter().sum();
            let (u0, u1) = (before / total, (before + length) / total);
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(KernelError::Invalid("path run".into())) };
            let ((t0, _), (_, t1)) = (first.tangents().ok_or_else(bad)?, last.tangents().ok_or_else(bad)?);
            let (a0, b1) = (first.start(), last.end());
            let normal = affine_turn(&at_start, n0).normalized().ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
            let pose = Plane::from_normal(a0, normal).ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
            // The run's own laws on its section at the start: the twist over its length (less the
            // turn back to the orientation direction after a corner), the scale from where the
            // last run left it to where this one ends, the rails from corner to corner.
            let back = match ctl.orientation {
                crate::SweepOrientation::Direction(_) if ri > 0 => direction_offset(plane, start, first_t, a0, t0, u0, ctl, &at_start)?,
                _ => 0.0,
            };
            let run_ctl = crate::SweepControl {
                orientation: ctl.orientation,
                twist: ctl.twist * (u1 - u0) - back,
                scale: law(u1) / law(u0),
                rails: rails.get(ri).cloned().unwrap_or_default(),
                rail_fit: ctl.rail_fit,
            };
            // A station per degree the section turns, with the path and the twist.
            let turn: f64 = segs.iter().map(path_turn).sum::<f64>() + run_ctl.twist.abs();
            let n = ((turn / (std::f64::consts::PI / 180.0)).ceil() as usize).clamp(MIN_STATIONS, MAX_STATIONS);
            let (pts, tangents) = path_samples(segs, n)?;
            let st = crate::sweep_path::controlled_stations(&pose, &pts, &tangents, &[], &run_ctl)?;
            let maps: Vec<Affine> = st
                .iter()
                .map(|s| {
                    let [a, b, cc] = s.m;
                    affine_after(&Affine { m: [p(a), p(b), p(cc)], t: p(s.at - s.apply(a0)) }, &at_start)
                })
                .collect();
            let (Some(&first_map), Some(&end_map)) = (maps.first(), maps.last()) else { return Err(KernelError::Invalid("sweep path".into())) };
            // Each wall is known by its piece's middle at the station half way along the stretch
            // of the run that point keeps between the mitres (the stations are evenly spaced by
            // length): a point standing `s` ahead of the path keeps the stretch `s` back from the
            // run's, as in the plain sweep ([`cornered`]).
            let last_station = maps.len().saturating_sub(1);
            inside.extend(pieces.iter().enumerate().map(|(piece, x)| {
                let q = plane.to_world(x.seg.point_at(0.5));
                let ahead = (affine_apply(&first_map, q) - a0).dot(t0);
                let share = if translate || !(length > 0.0) { 0.5 } else { (0.5 - ahead / length).clamp(0.0, 1.0) };
                let station = ((share * last_station as f64).round() as usize).min(last_station);
                Seed { at: affine_apply(maps.get(station).unwrap_or(&end_map), q), piece }
            }));
            let mut piece = Brep::sweep_stations(&c, &maps, Ends::Capped).map_err(|e| failed("sweep", e))?;
            let next = match runs.get(ri + 1).and_then(|r| path.get(r.start)) {
                Some(s) => Some(tangent(Some(s))?.0),
                None => closing,
            };
            let exit = next.map(|t2| mitre_of(b1, t1, t2)).transpose()?;
            if !translate {
                // On past each mitre, the run's section there held still, far enough to take
                // every point of it to the mitre's plane.
                let prism = |map: Affine, toward: Vec3, by: f64| -> Result<Brep> {
                    Brep::sweep_stations(&c, &[map, affine_moved(&map, toward * by)], Ends::Capped).map_err(|e| failed("sweep corner", e))
                };
                if let Some(m) = entry {
                    let (fo, fx, fy) = mapped_frame(plane, &first_map);
                    let pre = corner_mitre_lead(fo, fx, fy, &pieces, m, false);
                    let lead = prism(affine_moved(&first_map, t0 * -pre), t0, pre)?;
                    piece = combined(&piece, &lead, solidcraft_smlib::BooleanOp::Union, "sweep corner")?;
                    let cut = mitre_cutter(fo, fx, fy, &pieces, m.at, -t0, -m.normal, m.axis, pre, arc_spec(first))?;
                    piece = combined(&piece, &cut, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
                if let Some(m) = exit {
                    let (eo, ex, ey) = mapped_frame(plane, &end_map);
                    let post = corner_mitre_lead(eo, ex, ey, &pieces, m, true);
                    let tail = prism(end_map, t1, post)?;
                    piece = combined(&piece, &tail, solidcraft_smlib::BooleanOp::Union, "sweep corner")?;
                    let cut = mitre_cutter(eo, ex, ey, &pieces, m.at, t1, m.normal, m.axis, post, arc_spec(last))?;
                    piece = combined(&piece, &cut, solidcraft_smlib::BooleanOp::Subtract, "sweep mitre")?;
                }
            }
            parts.push(piece);
            // The section at the run's end, turned through the corner onto the next run.
            at_start = end_map;
            entry = None;
            if let (Some(_), Some(m), false) = (next, exit, translate) {
                at_start = affine_after(&affine_turned(m.at, m.axis, m.turn), &end_map);
                entry = Some(m);
            }
            before += length;
        }
        let brep = joined_runs(parts)?;
        if closed {
            // Back at the start, the section has to be the profile again (a whole number of its
            // symmetries of twist).
            let to_station = |a: &Affine| {
                let v = |q: [f64; 3]| Vec3::new(q[0], q[1], q[2]);
                crate::sweep_path::Station { at: affine_apply(a, start), m: [v(a.m[0]), v(a.m[1]), v(a.m[2])] }
            };
            for l in region.loops() {
                crate::sweep_path::closing_offset(plane, l, &[to_station(&IDENTITY), to_station(&at_start)])?;
            }
        }
        let body = body_of(brep, "sweep")?;
        let roles = if closed {
            roles(&body, &plane.offset(size * 10.0), None, &[], &inside, false)?
        } else {
            let end_plane = Plane::new(affine_apply(&at_start, plane.origin), affine_turn(&at_start, plane.x), affine_turn(&at_start, plane.y))
                .ok_or_else(|| KernelError::Invalid("the sweep's end section is degenerate".into()))?;
            roles(&body, plane, Some(&end_plane), &seeds(plane, &pieces, |q| q), &inside, false)?
        };
        with_history(body, &roles, &pieces)
    }

    /// After a corner, how far the section at `a0` (the profile carried there by `at_start`) is
    /// turned about its normal (toward the run's direction `t0`) from where the orientation direction's law puts it
    /// (the law of a smooth path from `start`, leaving along `first_t`, `u0` of the way along):
    /// the turn the next run takes back, as a twist. Radians, right-handed about `t0`.
    #[allow(clippy::too_many_arguments)]
    fn direction_offset(
        plane: &Plane,
        start: Vec3,
        first_t: Vec3,
        a0: Vec3,
        t0: Vec3,
        u0: f64,
        ctl: &crate::SweepControl,
        at_start: &Affine,
    ) -> Result<f64> {
        let law = crate::SweepControl { twist: ctl.twist * u0, scale: 1.0 + (ctl.scale - 1.0) * u0, rails: Vec::new(), ..ctl.clone() };
        // A two-station path from the start to here with the two directions: the law's section here.
        let st = crate::sweep_path::controlled_stations(plane, &[start, a0], &[first_t, t0], &[], &law)?;
        let want = st.last().map(|s| s.apply(plane.x)).ok_or_else(|| KernelError::Invalid("sweep path".into()))?;
        let have = affine_turn(at_start, plane.x);
        // About the run's direction (the twist's axis, as `controlled_stations` turns about the
        // path where it leaves the profile).
        let axis = t0.normalized().ok_or_else(|| KernelError::Invalid("the sweep's section is degenerate".into()))?;
        let flat = |v: Vec3| (v - axis * v.dot(axis)).normalized();
        let (Some(w), Some(h)) = (flat(want), flat(have)) else {
            return Err(KernelError::Invalid("the profile lies along the sweep path at a corner".into()));
        };
        Ok(w.cross(h).dot(axis).atan2(w.dot(h)))
    }

    /// The guide rails cut at the path's corners, one set per run: each rail must turn with the
    /// path there, with a corner of its own in the plane halving the path's corner and by the
    /// same turn about the same axis.
    fn run_rails(
        path: &[crate::PathSeg],
        runs: &[std::ops::Range<usize>],
        rails: &[Vec<Vec3>],
        closing: Option<Vec3>,
    ) -> Result<Vec<Vec<Vec<Vec3>>>> {
        if rails.is_empty() {
            return Ok(vec![Vec::new(); runs.len()]);
        }
        if closing.is_some() {
            return Err(KernelError::Invalid("guide rails can't follow a closed path with corners: a rail runs from the start to the end".into()));
        }
        let refused = |i: usize| {
            KernelError::Invalid(format!(
                "at corner {} of the path the guide rails have to turn with it: each rail needs a corner of its own there, in the plane halving the path's corner and by the same turn",
                i + 1
            ))
        };
        let mut rest: Vec<Vec<Vec3>> = rails.iter().map(|r| crate::sweep_path::clean(r)).collect::<Result<_>>()?;
        let mut out = Vec::with_capacity(runs.len());
        for (ci, w) in runs.windows(2).enumerate() {
            let (Some(a), Some(b)) = (w[0].end.checked_sub(1).and_then(|i| path.get(i)), path.get(w[1].start)) else { return Err(refused(ci)) };
            let ((_, t1), (t2, _)) = (a.tangents().ok_or_else(|| refused(ci))?, b.tangents().ok_or_else(|| refused(ci))?);
            let at = a.end();
            let CornerMitre { normal: m, axis: k, turn, .. } = mitre_of(at, t1, t2)?;
            let mut here = Vec::with_capacity(rest.len());
            for rail in &mut rest {
                let size = rail.windows(2).map(|q| q[0].dist(q[1])).sum::<f64>().max(1e-9);
                let tol = (size * 1e-6).max(1e-6);
                let corner = (1..rail.len().saturating_sub(1)).find(|&j| {
                    let (Some(p), Some(q), Some(r)) = (rail.get(j - 1), rail.get(j), rail.get(j + 1)) else { return false };
                    let (Some(d1), Some(d2)) = ((*q - *p).normalized(), (*r - *q).normalized()) else { return false };
                    (*q - at).dot(m).abs() <= tol
                        && (d1.dot(d2).clamp(-1.0, 1.0).acos() - turn).abs() <= 1e-3
                        && d1.cross(d2).normalized().is_some_and(|kr| kr.dot(k) > 1.0 - 1e-6)
                });
                let j = corner.ok_or_else(|| refused(ci))?;
                let tail = rail.split_off(j);
                let mut head = std::mem::replace(rail, tail);
                head.extend(rail.first().copied());
                here.push(head);
            }
            out.push(here);
        }
        out.push(rest);
        Ok(out)
    }

    /// Where the profile's points sit from the path, as a section carries them along it: for
    /// each sampled point of `pieces` on `plane`, its distance ahead of the path's `start` along
    /// the unit direction of travel `t` there, and its distance square to that direction, both
    /// times `scale`. A section that turns with the path (along a smooth run, and through a
    /// corner about the line square to both directions) keeps both, a twist keeps both, and an
    /// end scale grows them by at most its largest scale.
    fn section_offsets(plane: &Plane, pieces: &[Piece], start: Vec3, t: Vec3, scale: f64) -> Vec<(f64, f64)> {
        pieces
            .iter()
            .flat_map(|x| (0..=8).map(move |i| x.seg.point_at(f64::from(i) / 8.0)))
            .map(|q| {
                let d = plane.to_world(q) - start;
                let s = d.dot(t);
                (s * scale, (d - t * s).len() * scale)
            })
            .collect()
    }

    /// How far a run's section is carried straight on past a corner turning by `turn` (`ahead`),
    /// or back before one (else), so that each of its points reaches the plane halving the
    /// corner. A point `s` ahead of the path and `w` from it, the plane's unit normal `m` at half
    /// the turn from the direction of travel `t`: `m · (s t + w) >= s cos(turn / 2) - w sin(turn /
    /// 2)`, so carried on `e` along `t` it reaches the plane once `e >= w tan(turn / 2) - s` (and
    /// back before the corner once `e >= w tan(turn / 2) + s`). A profile square to the path has
    /// `s = 0`: its reach times the tangent of half the turn; one standing at a slant to the path
    /// reaches across the mitre by its slant as well. A little further than the farthest point,
    /// so the cut back runs through the walls, never along the run's own end.
    fn mitre_lead(offsets: &[(f64, f64)], reach: f64, turn: f64, ahead: bool) -> f64 {
        let k = (turn * 0.5).tan();
        let need = offsets.iter().map(|(s, w)| w * k + if ahead { -s } else { *s }).fold(0.0f64, f64::max);
        need * 1.1 + reach * 1e-3 + 1e-6
    }

    /// The plane halving the corner at `at` from direction `t1` to `t2` (its unit normal, corner
    /// axis, binormal, half-turn cosine, and turn); a path that turns back on itself has none.
    fn mitre_of(at: Vec3, t1: Vec3, t2: Vec3) -> Result<CornerMitre> {
        let turn = t1.dot(t2).clamp(-1.0, 1.0).acos();
        if turn > std::f64::consts::PI - 1e-3 {
            return Err(KernelError::Invalid("the path turns back on itself at a corner".into()));
        }
        let normal = (t1 + t2).normalized().ok_or_else(|| KernelError::Invalid("path corner".into()))?;
        let axis = t1.cross(t2).normalized().ok_or_else(|| KernelError::Invalid("path corner".into()))?;
        let binormal = normal.cross(axis);
        let cos_half = t1.dot(normal).max(1e-6);
        Ok(CornerMitre { at, normal, axis, binormal, cos_half, turn })
    }

    /// Whether the piece goes to SMLib as its halves ([`unfolded`]): one Bézier whose control
    /// polygon turns back on itself.
    fn goes_in_halves(plane: &Plane, x: &Piece) -> Result<bool> {
        let Curve::Nurbs { degree, points, knots } = curve(plane, &x.seg)? else { return Ok(false) };
        Ok(points.len() == degree + 1 && knots.len() == 2 * (degree + 1) && turns_back(&points))
    }

    /// Which pieces of a loft's `sections` go to the skin joined to the piece before them, as
    /// one B-spline ([`Piece::joins_previous`]); the same for every section.
    ///
    /// The skin gives each curve of a section one unit of a wall's parameter, whatever its
    /// length. Where two pieces of a section meet tangentially at different speeds (two spans of
    /// one spline of different lengths), the wall's first derivative along the section jumps
    /// there, though the wall is smooth, and a curve drawn across the wall (a cut's edge) has to
    /// turn a corner in the wall's parameters on that knot line (#22319). The kernel says which
    /// pieces one parameter can take that jump from (`solidcore_ops::sweep::loft_one_curve_runs`):
    /// those that in every section carry on the piece before them with one first derivative on
    /// the spans that keep the first section's speed, in a run with a joint where the speeds
    /// differ. Sections of one outline at different sizes agree so, and sections of different
    /// shapes where their pieces divide their curves alike. Where the sections do not agree, and
    /// where the pieces already meet at one speed, the pieces go in as they are.
    ///
    /// What is SMLib's stays here: a piece that goes in as its halves ([`unfolded`]) joins
    /// nothing, and a closed run that in any section turns back on itself where it meets its own
    /// start is broken half-way round, as an extrusion's is ([`broken_at_seams`]).
    fn loft_joins(sections: &[(Plane, Vec<Piece>)]) -> Result<Vec<bool>> {
        let segs: Vec<Vec<Seg2>> = sections.iter().map(|(_, ps)| ps.iter().map(|x| x.seg).collect()).collect();
        let mut joins = solidcore_ops::sweep::loft_one_curve_runs(&segs);
        if !joins.contains(&true) {
            return Ok(joins);
        }
        for (plane, pieces) in sections {
            for (i, x) in pieces.iter().enumerate() {
                if goes_in_halves(plane, x)? {
                    joins.iter_mut().skip(i).take(2).for_each(|joined| *joined = false);
                }
            }
        }
        for (plane, pieces) in sections {
            let flagged: Vec<Piece> = pieces.iter().zip(&joins).map(|(x, joined)| Piece { joins_previous: *joined, ..x.clone() }).collect();
            for (joined, x) in joins.iter_mut().zip(broken_at_seams(plane, &flagged)?) {
                *joined = x.joins_previous;
            }
        }
        Ok(joins)
    }

    /// The curves of a loft's `sections` for the skin: one for each piece, and one B-spline for
    /// each run of joined pieces ([`loft_joins`]), every section's on the first section's knots
    /// for the run. The sections' own knots for a run agree to the measure they were joined by;
    /// a skin through knots a breath apart would take both of each pair. A curve that is no
    /// such run is as [`curves`] makes it.
    fn loft_section_curves(sections: &[(Plane, Vec<Piece>)]) -> Result<Vec<Vec<Curve>>> {
        let joins = loft_joins(sections)?;
        if !joins.contains(&true) {
            return sections.iter().map(|(pl, ps)| curves(pl, ps)).collect();
        }
        // Whether each curve of a section is a run: `curves` makes one curve of each piece that
        // does not join the one before it, with the pieces that join it.
        let mut runs: Vec<bool> = Vec::new();
        for joined in &joins {
            match runs.last_mut() {
                Some(run) if *joined => *run = true,
                _ => runs.push(false),
            }
        }
        let mut all: Vec<Vec<Curve>> = sections
            .iter()
            .map(|(pl, ps)| {
                let flagged: Vec<Piece> = ps.iter().zip(&joins).map(|(x, joined)| Piece { joins_previous: *joined, ..x.clone() }).collect();
                curves(pl, &flagged)
            })
            .collect::<Result<_>>()?;
        let lead: Vec<Option<Vec<f64>>> = all
            .first()
            .map(|section| section.iter().map(|c| if let Curve::Nurbs { knots, .. } = c { Some(knots.clone()) } else { None }).collect())
            .unwrap_or_default();
        for section in all.iter_mut().skip(1) {
            for ((c, from), run) in section.iter_mut().zip(&lead).zip(&runs) {
                if let (true, Curve::Nurbs { knots, .. }, Some(lead_knots)) = (*run, c, from)
                    && knots.len() == lead_knots.len()
                {
                    knots.clone_from(lead_knots);
                }
            }
        }
        Ok(all)
    }

    /// Ruled between two sections, else smooth through them (cubic from four).
    fn loft_degree(sections: usize) -> u32 {
        match sections {
            0..=2 => 1,
            3 => 2,
            _ => 3,
        }
    }

    /// A loft through `sections` (each a closed loop of pieces, all re-split to the same count
    /// and lined up), `first` the index of the section given first (its cap is the start cap).
    /// A run of pieces that every section agrees is one curve goes to the skin as one curve
    /// ([`loft_joins`]): the same wall, each piece still paired with its partner on its own
    /// parameter, without a jump of its first derivative where pieces of different speeds meet.
    pub(crate) fn loft(sections: &[(Plane, Vec<Piece>)], first: usize) -> Result<Body> {
        loft_through(sections, first, &loft_section_curves(sections)?, loft_degree(sections.len()))
    }

    /// [`loft`] with every piece a curve of its own, whatever the sections agree on: what a loft
    /// through joined pieces is measured against.
    #[cfg(test)]
    pub(crate) fn loft_piece_by_piece(sections: &[(Plane, Vec<Piece>)], first: usize) -> Result<Body> {
        let curves: Vec<Vec<Curve>> = sections.iter().map(|(pl, ps)| curves(pl, ps)).collect::<Result<_>>()?;
        loft_through(sections, first, &curves, loft_degree(sections.len()))
    }

    /// The loft skinned through `curves` (the sections', or with stations between them), `degree`
    /// across them; `sections` and `first` as [`loft`] takes them name the faces.
    fn loft_through(sections: &[(Plane, Vec<Piece>)], first: usize, curves: &[Vec<Curve>], degree: u32) -> Result<Body> {
        let brep = Brep::loft(curves, degree, true).map_err(|e| failed("loft", e))?;
        loft_roles(body_of(brep, "loft")?, sections, first)
    }

    /// The section curves [`loft`] skins (runs of one curve joined where every section agrees),
    /// section by section: what another kernel lofts.
    #[cfg(feature = "backend-occt")]
    pub(crate) fn loft_curves_of(sections: &[(Plane, Vec<Piece>)]) -> Result<Vec<Vec<Curve>>> {
        loft_section_curves(sections)
    }

    /// `body`, another kernel's loft of [`loft_curves_of`], with the roles and history [`loft`]
    /// gives its own.
    #[cfg(feature = "backend-occt")]
    pub(crate) fn name_loft(body: Body, sections: &[(Plane, Vec<Piece>)], first: usize) -> Result<Body> {
        loft_roles(body, sections, first)
    }

    /// `body`, a loft through `sections` (`first` as [`loft`] takes it), with the roles and the
    /// history its faces and walls have.
    fn loft_roles(body: Body, sections: &[(Plane, Vec<Piece>)], first: usize) -> Result<Body> {
        let (Some((start, lead)), Some(last)) = (sections.get(first), sections.get(if first == 0 { sections.len().saturating_sub(1) } else { 0 }))
        else {
            return Err(KernelError::Invalid("loft sections".into()));
        };
        // Each wall comes from its column's segments in every section.
        let columns: Vec<Piece> = lead
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let mut all: BTreeSet<usize> = BTreeSet::new();
                for (_, ps) in sections {
                    if let Some(q) = ps.get(i) {
                        all.extend(q.numbers.iter().copied());
                    }
                }
                Piece { seg: x.seg, numbers: all.into_iter().collect(), drawn: x.drawn, joins_previous: false }
            })
            .collect();
        let roles = roles(&body, start, Some(&last.0), &seeds(start, &columns, |q| q), &[], false)?;
        with_history(body, &roles, &columns)
    }

    /// The curves of a loft through `sections` (as [`loft`] takes them) guided by one or two
    /// `rails` (polylines from the first section to the last, through every section's curve), each
    /// section's and each station's in loft order, for [`loft_curves`] to skin. Stations follow the
    /// law [`crate::loft_rails::railed`] gives a loft's rails: between each two sections they are
    /// the plain loft there moved so that the points the rails guide land on the rails. The
    /// sections' curves are brought to one form there, so a line pairs with an arc (#12512).
    pub(crate) fn rail_curves(sections: &[(Plane, Vec<Piece>)], rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Vec<Vec<Curve>>> {
        if rails.is_empty() || rails.len() > 2 {
            return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
        }
        let secs = matched(sections)?;
        let all: Vec<Vec<Curve>> =
            crate::loft_rails::railed(&secs, true, rails, fit)?.iter().map(|section| section.iter().map(rows_curve).collect()).collect();
        Ok(all)
    }

    /// A railed loft's row curve (homogeneous control points `[x w, y w, z w, w]`) as SMLib takes
    /// it: each point's coordinates with its weight, the degree and knots as they are.
    fn rows_curve(c: &solidcraft_geom::bspline::CurveN<4>) -> Curve {
        let points = c.ctrl.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3], h[3]]).collect();
        Curve::Nurbs { degree: c.degree, points, knots: c.knots.clone() }
    }

    /// The railed loft through `sections` (as [`loft`] takes them) skinned through `all`
    /// ([`rail_curves`]), cubic across them: the railed loft.
    pub(crate) fn loft_curves(sections: &[(Plane, Vec<Piece>)], first: usize, all: &[Vec<Curve>]) -> Result<Body> {
        loft_through(sections, first, all, 3)
    }

    /// The loft through `sections` (as [`loft`] takes them) guided by 3…16 `rails`: solidcore's
    /// ([`railed_skin`]), capped into a solid.
    pub(crate) fn loft_many_rails(sections: &[(Plane, Vec<Piece>)], first: usize, rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
        loft_roles(body_from_record(&railed_skin(&skin_sections(&matched(sections)?)?, rails, fit, true)?, "loft")?, sections, first)
    }

    /// Refused unless a loft has 1…[`crate::MAX_LOFT_RAILS`] guide rails.
    pub(crate) fn check_rail_count(n: usize) -> Result<()> {
        let most = crate::MAX_LOFT_RAILS;
        if (1..=most).contains(&n) { Ok(()) } else { Err(KernelError::Invalid(format!("a loft takes 1…{most} guide rails"))) }
    }

    /// The solidcore loft through `sections` (each a plane's origin and normal with its curves,
    /// paired as solidcore takes them) guided by 3…16 `rails` (each a polyline through every
    /// section), as its record: capped into a solid, or an open skin. Its guided edges are
    /// certified within 10⁻⁴ of the loft's size (at least 10⁻⁶, solidcore's relative tolerance),
    /// the rule the one or two rails' loft uses too; the rails pass through every section, so
    /// their points stand for the loft's.
    pub(crate) fn railed_skin(sections: &[SkinSection], rails: &[Vec<Vec3>], fit: crate::RailFit, cap: bool) -> Result<solidcore_record::brep::Brep> {
        check_rail_count(rails.len())?;
        let rail_curves: Vec<SkinCurve> = rails
            .iter()
            .map(|r| {
                let points: Vec<[f64; 3]> = r.iter().map(|p| [p.x, p.y, p.z]).collect();
                solidcore_ops::sweep::railed::polyline_rail(&points).map_err(|e| KernelError::Invalid(e.to_string()))
            })
            .collect::<Result<_>>()?;
        let how = RailLoft {
            fit: match fit {
                crate::RailFit::Scale => SkinFit::Scale,
                crate::RailFit::Stretch => SkinFit::Stretch,
            },
            cap,
            ..RailLoft::relative(1e-4)
        };
        let skin = solidcore_ops::sweep::railed::loft_rails_space(sections, &rail_curves, &how).map_err(|e| KernelError::Invalid(e.to_string()))?;
        let (record, _) = skin.to_record().map_err(|e| KernelError::Invalid(e.to_string()))?;
        Ok(record)
    }

    /// The sections as the one or two rails' loft makes them (their curves as NURBS, in order).
    fn matched(sections: &[(Plane, Vec<Piece>)]) -> Result<Vec<crate::loft_rails::Section>> {
        sections
            .iter()
            .map(|(pl, ps)| {
                let segs: Vec<Seg2> = ps.iter().map(|x| x.seg).collect();
                crate::loft_rails::Section::of(pl, &segs, ps.iter().map(|x| x.numbers.clone()).collect())
            })
            .collect()
    }

    /// The sections of a loft along three or more rails as solidcore's skin takes them: their curves
    /// brought to one form, as the one or two rails' loft does (`unified`). Solidcore pairs the
    /// curves by their weights, so a straight curve's weights are solidcore's to give.
    pub(crate) fn skin_sections(matched: &[crate::loft_rails::Section]) -> Result<Vec<SkinSection>> {
        let unified = crate::loft_rails::resolve_planes(&crate::loft_rails::unified(matched)?);
        Ok(unified
            .iter()
            .map(|s| SkinSection {
                plane: s.plane.map(|pl| {
                    let n = pl.normal();
                    ([pl.origin.x, pl.origin.y, pl.origin.z], [n.x, n.y, n.z])
                }),
                curves: s.curves.iter().map(skin_nurbs).collect(),
            })
            .collect())
    }

    /// A NURBS curve with homogeneous control points as solidcore's skin takes it (x, y, z, weight).
    fn skin_nurbs(c: &solidcraft_geom::bspline::CurveN<4>) -> SkinCurve {
        let points = c.ctrl.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3], h[3]]).collect();
        SkinCurve::Nurbs { degree: c.degree, points, knots: c.knots.clone() }
    }

    /// A loft from the section `pieces` (lines) on `plane` to `apex`: the base is the start cap,
    /// each triangle comes from the segment it stands on.
    pub(crate) fn loft_to_point(plane: &Plane, pieces: &[Piece], apex: Vec3) -> Result<Body> {
        let c = curves(plane, pieces)?;
        let brep = Brep::loft_to_point(&c, p(apex)).map_err(|e| failed("loft to a point", e))?;
        name_loft_to_point(body_of(brep, "loft to a point")?, plane, pieces)
    }

    /// `body`, a loft of `pieces` on `plane` to a point, with the roles and history
    /// [`loft_to_point`] gives its own: the base is the start cap, each wall comes from the
    /// segment it stands on.
    pub(crate) fn name_loft_to_point(body: Body, plane: &Plane, pieces: &[Piece]) -> Result<Body> {
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], false)?;
        with_history(body, &roles, pieces)
    }

    /// The section curves [`loft_to_point`] lofts: what another kernel lofts.
    #[cfg(feature = "backend-occt")]
    pub(crate) fn section_curves(plane: &Plane, pieces: &[Piece]) -> Result<Vec<Curve>> {
        curves(plane, pieces)
    }

    /// [`loft_to_point`] following one or two guide `rails` (polylines from the section to the
    /// apex), `fit` saying how they reshape the section on the way: the base is the start cap, each
    /// wall comes from the segment it stands on.
    pub(crate) fn loft_to_point_with_rails(plane: &Plane, pieces: &[Piece], apex: Vec3, rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
        if rails.is_empty() || rails.len() > 2 {
            return Err(KernelError::Invalid("a loft to a point takes one or two guide rails".into()));
        }
        let segs: Vec<Seg2> = pieces.iter().map(|x| x.seg).collect();
        let base = crate::loft_rails::Section::of(plane, &segs, pieces.iter().map(|x| x.numbers.clone()).collect())?;
        let (rows, along) = crate::loft_rails::railed_to_point(&base, apex, rails, fit)?;
        let sections: Vec<Vec<Curve>> = rows.iter().map(|section| section.iter().map(rows_curve).collect()).collect();
        let brep = Brep::loft_to_point_through(&sections, p(apex), &along).map_err(|e| failed("loft to a point", e))?;
        let body = body_of(brep, "loft to a point")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], false)?;
        with_history(body, &roles, pieces)
    }

    /// The profile `pieces` on `plane` carried by a screw motion about the axis through `origin`
    /// along `axis`: `turns` right-handed turns, rising `pitch` per turn.
    pub(crate) fn screw(plane: &Plane, pieces: &[Piece], origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
        let c = curves(plane, pieces)?;
        // Walls within a ten-thousandth of the pitch of the true screw surfaces.
        let tolerance = (pitch.abs() * 1e-4).clamp(1e-6, 1e-2);
        let brep = Brep::screw_sweep(&c, p(origin), p(axis), pitch, turns, tolerance).map_err(|e| failed("screw sweep", e))?;
        let body = body_of(brep, "screw sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }

    /// [`screw`] with the profile moved `offset(t)` mm off the axis at turn t (negative: towards
    /// it), piecewise linear through `law`'s (turn, offset) pairs: a groove fading over a runout.
    pub(crate) fn screw_radial(
        plane: &Plane,
        pieces: &[Piece],
        origin: Vec3,
        axis: Vec3,
        pitch: f64,
        turns: f64,
        law: &[(f64, f64)],
        continuous_flanks: bool,
    ) -> Result<Body> {
        let c = curves(plane, pieces)?;
        let tolerance = (pitch.abs() * 1e-4).clamp(1e-6, 1e-2);
        let options =
            solidcraft_smlib::sweeps::ScrewSweepOptions { law: (!law.is_empty()).then_some(law), join_spline_walls: continuous_flanks, tolerance };
        let brep =
            Brep::screw_sweep_with_options(&c, p(origin), p(axis), pitch, turns, options).map_err(|e| failed("screw sweep with a radial law", e))?;
        let body = body_of(brep, "screw sweep")?;
        let roles = roles(&body, plane, None, &seeds(plane, pieces, |q| q), &[], true)?;
        with_history(body, &roles, pieces)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    /// The corner pieces of a sweep along a path are joined in a balanced tree (#14451): every join
    /// is of two neighbouring stretches, left first, and for 180 runs (a 181-point polyline) the
    /// joins take in 180 · 8 = 1440 pieces' worth of operands at most, where joining each piece onto
    /// the body so far takes in 2 + 3 + … + 180 = 16,289.
    #[test]
    fn corner_pieces_are_joined_neighbours_first_in_a_balanced_tree() {
        let mut load = 0;
        let mut joins = 0;
        let whole = super::union_tree((0..180).map(|i| i..i + 1).collect(), |a: std::ops::Range<usize>, b: std::ops::Range<usize>| {
            assert_eq!(a.end, b.start, "joined {a:?} with {b:?}, not its neighbour");
            load += a.len() + b.len();
            joins += 1;
            Ok(a.start..b.end)
        })
        .unwrap();
        assert_eq!(whole, Some(0..180));
        assert_eq!(joins, 179);
        assert!(load <= 180 * 8, "{load}");
        assert_eq!(super::union_tree(Vec::<usize>::new(), |a, _| Ok(a)).unwrap(), None);
        assert_eq!(super::union_tree(vec![7], |a, _| Ok(a)).unwrap(), Some(7));
    }
}
