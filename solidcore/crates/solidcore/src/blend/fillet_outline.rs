//! A round bigger than the wall it rolls on ends on the body's outline (not upstream;
//! solidcore-native, tangency-labs/solidcore#2541, WR-730, opt-in
//! [`UpstreamFixes::fillet_round_ends_on_outline`](crate::kernel::UpstreamFixes::fillet_round_ends_on_outline)).
//!
//! A convex round rolls a ball between two faces. Where a face beside the round is narrower
//! than the ball reaches, its rail runs past the face's rim, a convex edge where the body's
//! outline turns away (a cliff). The cliff overflow (WR-631) builds the round on the face
//! continued past the rim and cuts it from the body; when that round passes inside the rim
//! it takes material beyond the face the round leaves (a 100 x 100 x 10 plate rounded r 90 on
//! a top edge loses the plate under the rim back to x = 48.77). With this construction the
//! ball touches the rim instead: it stays tangent to the face it keeps and its section passes
//! through the rim, so the round ends on the body's outline and only the material above that
//! arc is taken (86 577.030 mm³ on that plate, #2541).
//!
//! The ball touching a rim is the ball rolling on the surface tangent to it along the rim (a
//! pipe of no radius about the rim, `docs/generic-blend.md` §3.4's "keeping the edge"): a
//! plane through a straight rim, the face's own plane turned about the rim; a cone through a
//! circular rim, the wall's cylinder leaned about the rim ([`Cone::lean_cylinder`](crate::surface::cone::Cone::lean_cylinder)). Both keep
//! the face's parameterization along the rim, so the solver rolls the ball on that surface
//! in place of the face, with the same offset, and builds the round as it builds a round as
//! tall as its wall: its rail is the rim. Where the ball touching one rim would land on the
//! kept face past that face's own rim, it touches both rims and its section is the arc
//! through them (on the plate past r = (L² + t²) / 2t = 505, an arc from the bottom edge to
//! the far top edge).
//!
//! The ball is decided in the round's section ([`outline_supports`]), where it is exact: a
//! round along a straight edge between two planes, or along a circular edge between a plane
//! square to its axis and a coaxial cylinder (the wall). Another configuration is left to the
//! cliff overflow.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`outline_supports`] | none (solidcore-native) |
//! | [`rims_of_face`] | none (solidcore-native) |
//!
//! Deviations: none of upstream's; there is no recursion.

use crate::blend::fillet_exact::{BlendQuadric, QuadricKind};
use crate::blend::fillet_solver::p1::test_manifold_tangent_rollover;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::knots::Side as KnotSide;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_outline";

/// The tolerance the edges' curves are recognised as lines and circles within.
const CURVE_TOL: f64 = 1.0e-8;

/// The samples a circle is recognised by (`IsArc`'s, as the fillet solver's torus takes).
const ARC_SAMPLES: u64 = 11;

/// The sine of the angle two directions may make and still count as the same direction
/// (the section is the same along the round only where they are).
const DIR_TOL: f64 = 1.0e-9;

/// Whether the unit directions `a` and `b` are parallel, either way, within [`DIR_TOL`].
fn parallel(a: Vec3, b: Vec3) -> bool {
    near(a.cross(b).length(), 0.0, DIR_TOL)
}

/// Whether `a` lies within `tol` of `b` (`false` for a NaN).
fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// The component of `v` square to the unit `dir`.
fn across(v: Vec3, dir: Vec3) -> Vec3 {
    v - dir * v.dot(dir)
}

/// A point of the round's section plane, in the section's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct P2 {
    x: f64,
    y: f64,
}

impl P2 {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
    fn scale(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s)
    }
    fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn distance(self, o: Self) -> f64 {
        self.sub(o).length()
    }
    /// Turned a quarter turn anticlockwise.
    const fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }
}

/// A face's line in the section: the points `p` with `n . p = c` (`n` a unit vector).
#[derive(Debug, Clone, Copy)]
struct Line2 {
    n: P2,
    c: f64,
}

impl Line2 {
    fn signed(&self, p: P2) -> f64 {
        self.n.dot(p) - self.c
    }
    fn foot(&self, p: P2) -> P2 {
        p.sub(self.n.scale(self.signed(p)))
    }
}

/// How the section plane sits in space: along a straight spine (the section is the same in
/// every plane square to `dir`), or about an axis (the section turned about it).
#[derive(Debug, Clone, Copy)]
enum Section {
    /// Coordinates `(P - origin) . x` and `(P - origin) . y`, `x` and `y` square to the
    /// spine's direction.
    Straight { origin: Point3, x: Vec3, y: Vec3 },
    /// Coordinates `(distance from the axis, height along it)`; `radial` the unit
    /// direction from the axis of the half-plane the section is drawn in.
    Turned {
        origin: Point3,
        axis: Vec3,
        radial: Vec3,
    },
}

impl Section {
    /// The section's point `p` in space (a turned section's in its half-plane).
    fn to3(&self, p: P2) -> Point3 {
        match *self {
            Self::Straight { origin, x, y } => origin + x * p.x + y * p.y,
            Self::Turned {
                origin,
                axis,
                radial,
            } => origin + radial * p.x + axis * p.y,
        }
    }

    /// The section's direction `v` in space.
    fn direction(&self, v: P2) -> Vec3 {
        match *self {
            Self::Straight { x, y, .. } => x * v.x + y * v.y,
            Self::Turned { axis, radial, .. } => radial * v.x + axis * v.y,
        }
    }

    fn to2(&self, p: Point3) -> P2 {
        match *self {
            Self::Straight { origin, x, y } => P2::new((p - origin).dot(x), (p - origin).dot(y)),
            Self::Turned { origin, axis, .. } => {
                let w = p - origin;
                P2::new(across(w, axis).length(), w.dot(axis))
            }
        }
    }

    /// A face's line in the section, `None` for a face this construction does not take: a
    /// plane along the spine (straight), a plane square to the axis or a coaxial cylinder
    /// (turned).
    fn face_line(&self, q: &BlendQuadric, tol: f64) -> Option<Line2> {
        match (*self, q.kind) {
            (Self::Straight { x, y, .. }, QuadricKind::Plane) => {
                let n = P2::new(q.dir.dot(x), q.dir.dot(y));
                let len = n.length();
                // the plane runs along the spine: its normal is square to it
                if !near(q.dir.dot(x.cross(y)), 0.0, DIR_TOL) {
                    return None;
                }
                let n = n.scale(1.0 / len);
                Some(Line2 {
                    n,
                    c: n.dot(self.to2(q.point)),
                })
            }
            (Self::Turned { axis, .. }, QuadricKind::Plane) => {
                if !parallel(q.dir, axis) {
                    return None;
                }
                Some(Line2 {
                    n: P2::new(0.0, 1.0),
                    c: self.to2(q.point).y,
                })
            }
            (Self::Turned { origin, axis, .. }, QuadricKind::Cylinder) => {
                let coaxial =
                    parallel(q.dir, axis) && across(q.point - origin, axis).length() <= tol;
                coaxial.then_some(Line2 {
                    n: P2::new(1.0, 0.0),
                    c: q.radius,
                })
            }
            _ => None,
        }
    }

    /// A rim's point in the section, `None` for a rim that does not run along the round: a
    /// line along the spine (straight), a circle about the axis (turned).
    fn rim_point(&self, q: &BlendQuadric, tol: f64) -> Option<P2> {
        match (*self, q.kind) {
            (Self::Straight { x, y, .. }, QuadricKind::Line) => {
                parallel(q.dir, x.cross(y)).then(|| self.to2(q.point))
            }
            (Self::Turned { origin, axis, .. }, QuadricKind::Circle) => {
                let coaxial =
                    parallel(q.dir, axis) && across(q.point - origin, axis).length() <= tol;
                coaxial.then(|| P2::new(q.radius, (q.point - origin).dot(axis)))
            }
            _ => None,
        }
    }
}

/// A face of the round in the section: its line, the filleted edge's point on it, the unit
/// direction along it away from the edge (toward the round's rail), and the nearest rim
/// past the edge on it, with its distance from the edge.
#[derive(Debug, Clone, Copy)]
struct Side {
    line: Line2,
    along: P2,
    rim: Option<(P2, f64)>,
}

/// Whether the ball of `radius` about `centre` meets side `side`'s line inside the face,
/// between the edge `e` and the rim, other than at `touch` (where it touches the line or
/// passes through the rim). The face is kept whole only where the ball does not cut it.
fn cuts_face(side: &Side, e: P2, centre: P2, radius: f64, touch: P2, tol: f64) -> bool {
    let h = side.line.signed(centre);
    let w2 = radius * radius - h * h;
    if w2.is_nan() || w2 <= 0.0 {
        return false;
    }
    let w = w2.sqrt();
    let foot = side.line.foot(centre);
    let t = side.line.n.perp();
    let limit = side.rim.map_or(f64::INFINITY, |(_, s)| s);
    [foot.add(t.scale(w)), foot.sub(t.scale(w))]
        .into_iter()
        .any(|p| {
            let s = p.sub(e).dot(side.along);
            p.distance(touch) > tol && s > tol && s < limit - tol
        })
}

/// The centre of the ball of `radius` tangent to `keep`'s line (on the side of `toward`)
/// and passing through `rim`, whose foot on `keep`'s line lies on the face (past the edge
/// `e`, not past its own rim) and which cuts neither face: of two, the one nearer `toward`.
fn tangent_through_rim(
    keep: &Side,
    leave: &Side,
    rim: P2,
    e: P2,
    toward: P2,
    radius: f64,
    tol: f64,
) -> Option<P2> {
    let side = if keep.line.signed(toward) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let offset = Line2 {
        n: keep.line.n,
        c: keep.line.c + side * radius,
    };
    let foot = offset.foot(rim);
    let h = foot.distance(rim);
    let w2 = radius * radius - h * h;
    if w2.is_nan() || w2 < 0.0 {
        return None;
    }
    let w = w2.sqrt();
    let t = offset.n.perp();
    let mut best: Option<P2> = None;
    for centre in [foot.add(t.scale(w)), foot.sub(t.scale(w))] {
        let touch = keep.line.foot(centre);
        let s = touch.sub(e).dot(keep.along);
        let on_face = s > tol && keep.rim.is_none_or(|(_, limit)| s <= limit + tol);
        // (tangent to the kept face's line, it meets that line only at its foot)
        if !on_face || cuts_face(leave, e, centre, radius, rim, tol) {
            continue;
        }
        if best.is_none_or(|b| centre.distance(toward) < b.distance(toward)) {
            best = Some(centre);
        }
    }
    best
}

/// The centre of the ball of `radius` through both rims that cuts neither face: of two, the
/// one nearer `toward`.
fn through_both_rims(
    sides: &[Side; 2],
    rims: [P2; 2],
    e: P2,
    toward: P2,
    radius: f64,
    tol: f64,
) -> Option<P2> {
    let [a, b] = rims;
    let half = a.distance(b) / 2.0;
    let w2 = radius * radius - half * half;
    if half.is_nan() || half <= tol || w2.is_nan() || w2 < 0.0 {
        return None;
    }
    let mid = a.add(b).scale(0.5);
    let t = b.sub(a).scale(1.0 / (2.0 * half)).perp();
    let w = w2.sqrt();
    let [s0, s1] = sides;
    let mut best: Option<P2> = None;
    for centre in [mid.add(t.scale(w)), mid.sub(t.scale(w))] {
        if cuts_face(s0, e, centre, radius, a, tol) || cuts_face(s1, e, centre, radius, b, tol) {
            continue;
        }
        if best.is_none_or(|c| centre.distance(toward) < c.distance(toward)) {
            best = Some(centre);
        }
    }
    best
}

/// The ball the round ends on the outline with ([`decide`]): its centre in the section, and
/// for each side the rim it touches there (`None`: it stays tangent to that face).
#[derive(Debug, Clone, Copy)]
struct Outline {
    centre: P2,
    touches: [Option<P2>; 2],
}

/// The ball of `radius` the round ends on the outline with, if it does: `lines` the faces'
/// lines in the section, `c2` the centre of the ball on the faces' continuations, `rims[i]`
/// the rims of face `i` in the section.
///
/// A rim counts when the ball's rail on that face runs past it and the ball on the face's
/// continuation passes inside it by more than `tol`: the round then takes material past
/// the rim, beyond the face it leaves. The ball then touches that rim and stays tangent to
/// the other face, if its foot there lies on that face (up to that face's own nearest rim)
/// and it cuts neither face; where no such ball is, but the ball through both rims cuts
/// neither face, it touches both. Otherwise (no rim passed, two ways, or none) `None`.
fn decide(lines: [Line2; 2], c2: P2, radius: f64, rims: [&[P2]; 2], tol: f64) -> Option<Outline> {
    let [l0, l1] = lines;
    // the filleted edge: where the faces' lines meet
    let det = l0.n.x * l1.n.y - l0.n.y * l1.n.x;
    if det.is_nan() || det.abs() <= DIR_TOL {
        return None;
    }
    let e = P2::new(
        (l0.c * l1.n.y - l0.n.y * l1.c) / det,
        (l0.n.x * l1.c - l0.c * l1.n.x) / det,
    );
    let mut sides = Vec::with_capacity(2);
    let mut passed = Vec::with_capacity(2);
    for (line, face_rims) in lines.into_iter().zip(rims) {
        let rail = line.foot(c2);
        let reach = rail.distance(e);
        if reach.is_nan() || reach <= tol {
            return None;
        }
        let along = rail.sub(e).scale(1.0 / reach);
        let mut rim: Option<(P2, f64)> = None;
        for &p in face_rims {
            let s = p.sub(e).dot(along);
            if !near(line.signed(p), 0.0, tol) || s.is_nan() || s <= tol {
                continue;
            }
            if rim.is_none_or(|(_, best)| s < best) {
                rim = Some((p, s));
            }
        }
        // the rail runs past the rim, and the ball on the continued face passes inside it
        passed.push(rim.is_some_and(|(p, s)| s < reach - tol && p.distance(c2) - radius > tol));
        sides.push(Side { line, along, rim });
    }
    let [s0, s1] = <[Side; 2]>::try_from(sides).ok()?;
    let [p0, p1] = <[bool; 2]>::try_from(passed).ok()?;
    if !p0 && !p1 {
        return None;
    }
    let mut found = Vec::with_capacity(2);
    for (keep, leave, leaving, first) in [(&s1, &s0, p0, true), (&s0, &s1, p1, false)] {
        let Some((rim, _)) = leave.rim.filter(|_| leaving) else {
            continue;
        };
        if let Some(centre) = tangent_through_rim(keep, leave, rim, e, c2, radius, tol) {
            found.push(Outline {
                centre,
                touches: if first {
                    [Some(rim), None]
                } else {
                    [None, Some(rim)]
                },
            });
        }
    }
    match found.as_slice() {
        [one] => Some(*one),
        [] => {
            let ((r0, _), (r1, _)) = (s0.rim?, s1.rim?);
            let centre = through_both_rims(&[s0, s1], [r0, r1], e, c2, radius, tol)?;
            Some(Outline {
                centre,
                touches: [Some(r0), Some(r1)],
            })
        }
        _ => None,
    }
}

/// The rims of `face` for a round of `filleted` ([`outline_supports`] reads them): each other
/// edge of the face where the body's outline turns away from the material at an angle (a
/// convex edge, not concave and not tangent within `tangency_tol_rad`) and that runs the
/// filleted edge's whole length: for a straight filleted edge, a straight edge parallel to
/// it whose span along it covers the filleted edge's; for a circular one, a whole circle
/// about the same axis. Each is given as a [`QuadricKind::Line`] (a point of it and its unit
/// direction) or a [`QuadricKind::Circle`] (its centre, unit axis and radius). Empty for a
/// filleted edge that is neither straight nor circular.
///
/// # Errors
///
/// Those of the topology queries, the curves' recognisers and the convexity and tangency
/// tests.
pub fn rims_of_face(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    face: FaceId,
    tangency_tol_rad: f64,
) -> Result<Vec<BlendQuadric>> {
    let mut rims = Vec::new();
    let Some(curve) = edge::curve(target, filleted)? else {
        return Ok(rims);
    };
    let ivl = edge::interval(target, filleted)?;
    let ends = [
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ];
    let straight = curve.is_line(k, 3, CURVE_TOL)?;
    let circular = if straight.is_none() {
        curve.is_arc(k, ARC_SAMPLES, CURVE_TOL)?
    } else {
        None
    };
    if straight.is_none() && circular.is_none() {
        return Ok(rims);
    }
    let tol = target.tolerance();
    for e in face_p4::edges(target, face, None, false, None)? {
        if e == filleted {
            continue;
        }
        let Some(rim_curve) = edge::curve(target, e)? else {
            continue;
        };
        let rim_ivl = edge::interval(target, e)?;
        let quadric = if let Some((_, vector)) = straight {
            let Ok(d) = vector.unitize() else {
                continue;
            };
            let Some((point, rim_vector)) = rim_curve.is_line(k, 3, CURVE_TOL)? else {
                continue;
            };
            let Ok(rd) = rim_vector.unitize() else {
                continue;
            };
            if !parallel(d, rd) {
                continue;
            }
            // its span along the filleted edge covers the filleted edge's
            let along = |p: Point3| (p - point).dot(d);
            let rim_ends = [
                along(rim_curve.evaluate_point(k, rim_ivl.min())?),
                along(rim_curve.evaluate_point(k, rim_ivl.max())?),
            ];
            let [a0, a1] = ends.map(along);
            let lo = a0.min(a1);
            let hi = a0.max(a1);
            let [r0, r1] = rim_ends;
            let covers = r0.min(r1) <= lo + tol && r0.max(r1) >= hi - tol;
            if !covers {
                continue;
            }
            BlendQuadric {
                kind: QuadricKind::Line,
                point,
                dir: d,
                ..BlendQuadric::default()
            }
        } else if let Some((frame, _, _, _)) = circular {
            let axis = frame.z_axis();
            let Ok(axis) = axis.unitize() else {
                continue;
            };
            let Some((rim_frame, rim_radius, _, _)) =
                rim_curve.is_arc(k, ARC_SAMPLES, CURVE_TOL)?
            else {
                continue;
            };
            let Ok(rim_axis) = rim_frame.z_axis().unitize() else {
                continue;
            };
            let coaxial = parallel(axis, rim_axis)
                && across(rim_frame.origin() - frame.origin(), axis).length() <= tol;
            // a whole circle
            if !coaxial || !rim_curve.is_closed(k, &rim_ivl, 0.0)? {
                continue;
            }
            BlendQuadric {
                kind: QuadricKind::Circle,
                point: rim_frame.origin(),
                dir: axis,
                radius: rim_radius,
                ..BlendQuadric::default()
            }
        } else {
            continue;
        };
        // where the outline turns away at an angle: convex and not tangent
        if is_concave_edge(k, target, e)? {
            continue;
        }
        let (tangent, _) = test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?;
        if tangent {
            continue;
        }
        rims.push(quadric);
    }
    Ok(rims)
}

/// The surfaces a lone constant round's ball rolls on so that it ends on the body's outline
/// (the rolling ball solver's supports in place of the faces), or `None` where the round
/// does not end there ([`decide`]).
///
/// `filleted` is the round's edge, `faces[i]` the face of its rail `i` and `offsets[i]` the
/// solver's signed offset of that face's surface (the ball's centre lies at the face's
/// point plus its normal times the offset). The section is taken across the middle of a
/// straight edge, between two planes, or through a circular edge's start, between a plane
/// square to its axis and a coaxial cylinder. The round's ends are cut by planes: every
/// other face at the edge's ends is planar, as the cliff overflow needs its end faces
/// (WR-631's `CliffEndFaceNotPlanar`).
///
/// A ball touching a rim rolls on the surface tangent to it along that rim: the face's plane
/// turned about a straight rim, or the wall's cylinder leaned into the cone through a
/// circular rim. Each such side's entry is that surface, whose normal at the rim the same
/// offset takes to the ball's centre; a side where the ball stays tangent to its face is
/// `None`. A support the configuration does not give (a rim on a plane about the axis, a
/// normal turned by a right angle or more) leaves the round to the cliff overflow (`None`).
///
/// # Errors
///
/// Those of the topology queries, the evaluations, [`rims_of_face`] and the surfaces'
/// constructions; `SM_ERR` for a face without a surface or a point that does not drop on
/// it.
pub fn outline_supports(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
    tangency_tol_rad: f64,
) -> Result<Option<[Option<Surface>; 2]>> {
    // the outline is a solid's: its rims bound material on one side
    if !crate::topology::brep::p2::is_manifold_solid(target)? {
        return Ok(None);
    }
    let tol = target.tolerance();
    // the round's ends are cut by planes, as the cliff overflow's are (WR-631): every other
    // face at the filleted edge's ends is planar
    for v in [
        edge::start_vertex(target, filleted)?,
        edge::end_vertex(target, filleted)?,
    ]
    .into_iter()
    .flatten()
    {
        for f in crate::topology::vertex::faces(target, v)? {
            if faces.contains(&f) {
                continue;
            }
            let surface = crate::topology::face::p2::surface(target, f)?
                .ok_or_else(|| SolidError::sm_err(OP))?;
            if !surface.is_planar(k, tol)?.is_planar {
                return Ok(None);
            }
        }
    }
    let Some(curve) = edge::curve(target, filleted)? else {
        return Ok(None);
    };
    let ivl = edge::interval(target, filleted)?;
    let straight = curve.is_line(k, 3, CURVE_TOL)?;
    let circular = if straight.is_none() {
        curve.is_arc(k, ARC_SAMPLES, CURVE_TOL)?
    } else {
        None
    };
    // the section: across the middle of a straight edge; through a circular edge's start,
    // where a closed edge's seam is
    let m = curve.evaluate_point(
        k,
        if straight.is_some() {
            ivl.evaluate(0.5)
        } else {
            ivl.min()
        },
    )?;
    // each face's surface, its normal at the section and its natural quadric
    let mut surfaces = Vec::with_capacity(2);
    for face in faces {
        let surface = crate::topology::face::p2::surface(target, face)?
            .ok_or_else(|| SolidError::sm_err(OP))?
            .clone_for_evaluation()?;
        // a plane or a cylinder, the only faces the section takes
        if !matches!(surface.kind(), SurfaceKind::Cone(_)) && !surface.is_planar(k, tol)?.is_planar
        {
            return Ok(None);
        }
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let dropped =
            surface.drop_point::<NoObject>(k, m, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(None);
        }
        let uv = dropped.result_uv;
        let normal = surface.evaluate_normal(k, uv, KnotSide::Left, KnotSide::Left)?;
        let quadric = BlendQuadric::from_surface(k, &surface, uv, tol)?;
        surfaces.push((surface, normal, quadric));
    }
    let [(s0, n0, q0), (s1, n1, q1)] = <[(Surface, Vec3, BlendQuadric); 2]>::try_from(surfaces)
        .map_err(|_| SolidError::sm_err(OP))?;
    let section = if let Some((_, vector)) = straight {
        let d = vector.unitize()?;
        let Ok(x) = across(n0, d).unitize() else {
            return Ok(None);
        };
        Section::Straight {
            origin: m,
            x,
            y: d.cross(x),
        }
    } else if let Some((frame, _, _, _)) = circular {
        let origin = frame.origin();
        let axis = frame.z_axis().unitize()?;
        Section::Turned {
            origin,
            axis,
            radial: across(m - origin, axis).unitize()?,
        }
    } else {
        return Ok(None);
    };
    let (Some(l0), Some(l1)) = (section.face_line(&q0, tol), section.face_line(&q1, tol)) else {
        return Ok(None);
    };
    // the ball's centre on the faces' continuations: where their offsets' lines meet
    let [d0, d1] = offsets;
    let shifted = |l: Line2, n: Vec3, d: f64| Line2 {
        n: l.n,
        c: l.n.dot(section.to2(m + n * d)),
    };
    let (o0, o1) = (shifted(l0, n0, d0), shifted(l1, n1, d1));
    // (the sine of the faces' angle)
    let det = o0.n.x * o1.n.y - o0.n.y * o1.n.x;
    if det.is_nan() || det.abs() <= DIR_TOL {
        return Ok(None);
    }
    let c2 = P2::new(
        (o0.c * o1.n.y - o0.n.y * o1.c) / det,
        (o0.n.x * o1.c - o0.c * o1.n.x) / det,
    );
    let radius = d0.abs();
    if !near(d1.abs(), radius, tol) {
        return Ok(None);
    }
    let mut rims = [Vec::new(), Vec::new()];
    for (slot, face) in rims.iter_mut().zip(faces) {
        for q in rims_of_face(k, target, filleted, face, tangency_tol_rad)? {
            if let Some(p) = section.rim_point(&q, tol) {
                slot.push(p);
            }
        }
    }
    let [r0, r1] = &rims;
    let Some(outline) = decide([l0, l1], c2, radius, [r0, r1], tol) else {
        return Ok(None);
    };
    // a turned ball clear of the axis (its round a ring torus)
    let clear = outline.centre.x - radius;
    if matches!(section, Section::Turned { .. }) && (clear.is_nan() || clear <= tol) {
        return Ok(None);
    }
    let mut supports = [None, None];
    for ((((support, touch), surface), normal), d) in supports
        .iter_mut()
        .zip(outline.touches)
        .zip([s0, s1])
        .zip([n0, n1])
        .zip(offsets)
    {
        let Some(rim) = touch else {
            continue;
        };
        // the support's normal at the rim: the same offset reaches the centre
        let to = outline.centre.sub(rim).scale(1.0 / d);
        let Some(tangent) = tangent_along_rim(k, &section, surface, normal, rim, to)? else {
            return Ok(None);
        };
        // the support turns the face's normal by less than a right angle, and its own normal
        // at the rim is `to`
        let want = section.direction(to);
        let turn = normal.dot(want);
        if turn.is_nan() || turn <= 0.0 || !normal_at(k, &tangent, section.to3(rim), want)? {
            return Ok(None);
        }
        *support = Some(tangent);
    }
    Ok(Some(supports))
}

/// The surface tangent to the ball along a rim at `rim` (in the section), whose normal there
/// is `to` (a unit vector in the section): `surface` (the face's, normal `normal` at the
/// section) turned about the rim line, for a straight section; the wall's cylinder leaned
/// into the cone through the rim along the line square to `to`, for a turned one (`None`
/// for a face that is not a cylinder, or a line square to the axis).
fn tangent_along_rim(
    k: &Kernel,
    section: &Section,
    mut surface: Surface,
    normal: Vec3,
    rim: P2,
    to: P2,
) -> Result<Option<Surface>> {
    match *section {
        Section::Straight { x, y, .. } => {
            let from = P2::new(normal.dot(x), normal.dot(y));
            let angle = (from.x * to.y - from.y * to.x).atan2(from.dot(to));
            let mut turn = Axis2Placement::default();
            turn.init();
            turn.rotate_about_axis_at_point(angle, section.to3(rim), x.cross(y));
            surface.transform(k, &turn, None)?;
            Ok(Some(surface))
        }
        Section::Turned { origin, axis, .. } => {
            // the wall's cylinder leaned into the cone through the rim along the line square
            // to `to` (its parameterization kept, so the face's trims stay valid on it)
            let SurfaceKind::Cone(cylinder) = surface.kind() else {
                return Ok(None);
            };
            let frame = &cylinder.canonical().origin;
            let cone_axis = frame.x_axis().cross(frame.y_axis()).unitize()?;
            let along = to.perp();
            if !cylinder.is_cylinder() || along.y.is_nan() || along.y.abs() <= EFF_ZERO {
                return Ok(None);
            }
            // the radius's change per unit of height along the cone's own axis
            let slope = along.x / along.y * cone_axis.dot(axis);
            let rim_point = origin + axis * rim.y;
            let height = |p: Point3| (p - frame.origin()).dot(cone_axis);
            let neutral = height(rim_point);
            // the cone keeps a radius over the whole wall (its rows run from one end of the
            // wall's height to the other): one that reaches the axis is not this support
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            for v in [domain.min.y, domain.max.y] {
                let p = surface.evaluate_point(k, Point2::new(domain.min.x, v))?;
                let leaned = cylinder.canonical().radius + slope * (height(p) - neutral);
                if leaned.is_nan() || leaned <= EFF_ZERO {
                    return Ok(None);
                }
            }
            Ok(Some(Surface::from(
                cylinder.lean_cylinder(k, slope, neutral)?,
            )))
        }
    }
}

/// Whether `surface`'s unit normal at its point nearest `at` is `want`, within [`DIR_TOL`].
fn normal_at(k: &Kernel, surface: &Surface, at: Point3, want: Vec3) -> Result<bool> {
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<NoObject>(k, at, &domain, &DropPointOptions::default())?;
    if !dropped.success {
        return Ok(false);
    }
    let n = surface.evaluate_normal(k, dropped.result_uv, KnotSide::Left, KnotSide::Left)?;
    let Ok(n) = n.unitize() else {
        return Ok(false);
    };
    Ok(n.dot(want) > 0.0 && parallel(n, want))
}
