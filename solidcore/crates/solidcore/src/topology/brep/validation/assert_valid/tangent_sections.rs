//! Where test 9 reads the faces round a line edge whose faces are tangent along it: past
//! the band in which tangent faces are within the contact tolerance of each other.
//!
//! Upstream: none (not upstream; #18639, on top of WR-540's step into the faces).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `tangent_directions` | none: the directions test 9 reads round such an edge |
//! | `Section`, `choose_step` (private) | none: the faces' sections square to the edge and the certified step |
//! | `clearance`, `boundary_clearance` (private) | none: the ball round the edge point no other boundary enters |
//!
//! Deviations: not upstream. WR-540 reads every face of an edge with tangent sectors at
//! its point 20 edge tolerances into the face. Two faces tangent along the edge that dip
//! into each other by no more than the contact tolerance touch there (WR-510,
//! docs/booleans.md §6), but they may cross each other farther out than that step: a box
//! face 5e-6 inside a cylinder of radius 1 crosses it 3.2e-3 from the tangent line, and at
//! 20 edge tolerances it reads as lying inside the cylinder although past the band it lies
//! outside. Where every face round the edge is a plane or a cylinder whose axis is the
//! edge's direction, the faces' sections in the plane square to the edge are lines and
//! circles, and this module decides in closed form a step at which every tangent pair is
//! more than the contact tolerance apart along the normal, with no two neighbouring faces
//! round the edge (each sector's pair), tangent or not, ever more than it apart on both
//! sides of each other anywhere
//! from the edge out to that step (the gap's closed-form extremes over the whole stretch,
//! the edge point included); the steps stay
//! inside a ball round
//! the edge point that no other boundary of the faces enters. Every other edge, and every
//! edge where one of those conditions fails or reading the faces' other boundaries fails,
//! keeps WR-540's reading. No recursion, and a
//! fixed amount of work per edge use.

use crate::math::det;
use std::cmp::Ordering;

use crate::curve::curve::CurveKind;
use crate::error::{ignored, Result};
use crate::geom::extent3d::Extent3d;
use crate::geom::tol::{contact_tol_3d, within_contact_tol};
use crate::geom::vector2d::Vec2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, PI};
use crate::nurbs::math::gamma;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge as tedge, face as tface, Brep, EdgeId, EdgeuseId, FaceId};

/// A cylinder's axis may lean off the edge's direction by at most this sine: its section
/// then differs from the circle of its radius by `r (1 / cos a - 1) < 1e-18 r`, far under
/// one rounding of `r`, so inside the rounding allowance of the contact rule.
const AXIS_LEAN: f64 = 1e-9;

/// The largest turn round a circle a step may take (60 degrees: the chord is at most the
/// radius), so a step and its angle determine each other.
const MAX_TURN: f64 = PI / 3.0;

/// A face's section by the plane square to the edge at the edge point `P` (the origin), in
/// the frame `e1`, `e2` with `e1 x e2` the edge's direction.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Section {
    /// A plane: the signed distance of a point `X` of the section plane from the plane is
    /// `n . X - c` (its 3D distance, `n` the plane's unit normal projected). Its points
    /// a step `s` into the face are `foot + s dir`.
    Line {
        /// The plane's normal projected into the section plane.
        n: Vec2,
        /// The plane's offset: `n3 . (origin - P)`.
        c: f64,
        /// The point of the line nearest `P`.
        foot: Vec2,
        /// The unit direction of the line into the face.
        dir: Vec2,
    },
    /// A cylinder along the edge: the signed distance of `X` from it is
    /// `|X - centre| - r`. Its point a step `s` into the face is on the circle at chord
    /// `s` from the foot, turning by `sense` (+1 counterclockwise).
    Circle {
        /// The axis's point in the section plane.
        centre: Vec2,
        /// The radius.
        r: f64,
        /// The angle of the point of the circle nearest `P`.
        foot_angle: f64,
        /// `+1.0` when the face leaves the foot counterclockwise, else `-1.0`.
        sense: f64,
    },
}

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn unit_angle(a: f64) -> Vec2 {
    Vec2::new(det::cos(a), det::sin(a))
}

impl Section {
    /// The point a step `s` into the face.
    fn point(&self, s: f64) -> Vec2 {
        match *self {
            Self::Line { foot, dir, .. } => foot + s * dir,
            Self::Circle {
                centre,
                r,
                foot_angle,
                sense,
            } => {
                let theta = foot_angle + sense * 2.0 * det::asin(s / (2.0 * r));
                centre + r * unit_angle(theta)
            }
        }
    }

    /// The signed distance of `x` from this face's surface.
    fn gap(&self, x: Vec2) -> f64 {
        match *self {
            Self::Line { n, c, .. } => n.dot(x) - c,
            Self::Circle { centre, r, .. } => (x - centre).length() - r,
        }
    }

    /// The unit direction in which [`Section::gap`] grows at `x`.
    fn gradient(&self, x: Vec2) -> Option<Vec2> {
        let g = match *self {
            Self::Line { n, .. } => n,
            Self::Circle { centre, .. } => x - centre,
        };
        let l = g.length();
        (l > 0.0).then(|| g * (1.0 / l))
    }

    /// The point of the section nearest `P`.
    fn foot(&self) -> Vec2 {
        match *self {
            Self::Line { foot, .. } => foot,
            Self::Circle {
                centre,
                r,
                foot_angle,
                ..
            } => centre + r * unit_angle(foot_angle),
        }
    }

    /// The unit direction in which the face leaves its foot.
    fn tangent(&self) -> Vec2 {
        match *self {
            Self::Line { dir, .. } => dir,
            Self::Circle {
                foot_angle, sense, ..
            } => sense * Vec2::new(-det::sin(foot_angle), det::cos(foot_angle)),
        }
    }

    /// The longest step this section is read at.
    fn max_step(&self) -> f64 {
        match *self {
            Self::Line { .. } => f64::INFINITY,
            Self::Circle { r, .. } => r,
        }
    }

    /// A bound on the angle between the direction from `P` to the point a step `s` in and
    /// [`Section::tangent`]: the foot's offset from `P` turns it by at most
    /// `asin(|foot| / s)`, and a circle's chord turns from its tangent by `asin(s / 2r)`.
    fn drift(&self, s: f64) -> Option<f64> {
        let f = self.foot().length();
        if f.partial_cmp(&s) != Some(Ordering::Less) {
            return None;
        }
        let turn = match *self {
            Self::Line { .. } => 0.0,
            Self::Circle { r, .. } => det::asin(s / (2.0 * r)),
        };
        Some(det::asin(f / s) + turn)
    }

    /// The step at which a circle reaches angle `theta`, if it does within [`MAX_TURN`].
    fn step_at_angle(&self, theta: f64) -> Option<f64> {
        let Self::Circle {
            r,
            foot_angle,
            sense,
            ..
        } = *self
        else {
            return None;
        };
        let mut d = sense * (theta - foot_angle);
        d -= 2.0 * PI * (d / (2.0 * PI)).round();
        (0.0..=MAX_TURN)
            .contains(&d)
            .then(|| 2.0 * r * det::sin(d / 2.0))
    }
}

/// The phase of the cosine in which the gap of `b` at the points of the circle `a` is
/// increasing, `gap = H(cos(theta - phase))` with `H` increasing; `None` when the gap is
/// constant round `a` or `a` is not a circle.
fn circle_phase(a: &Section, b: &Section) -> Option<f64> {
    let Section::Circle { centre: ca, .. } = *a else {
        return None;
    };
    let v = match *b {
        Section::Line { n, .. } => n,
        Section::Circle { centre: cb, .. } => ca - cb,
    };
    (v.length() > 0.0).then(|| det::atan2(v.y, v.x))
}

/// The steps at which the gap of `b` at the points of `a` equals `v` (closed form; only
/// candidates, each is checked where it is used).
fn roots(a: &Section, b: &Section, v: f64) -> Vec<f64> {
    let mut out = Vec::new();
    match (*a, *b) {
        (Section::Line { foot, dir, .. }, Section::Line { n, c, .. }) => {
            let slope = n.dot(dir);
            if slope != 0.0 {
                out.push((v - (n.dot(foot) - c)) / slope);
            }
        }
        (Section::Line { foot, dir, .. }, Section::Circle { centre, r, .. }) => {
            let big = r + v;
            if big > 0.0 {
                let w = foot - centre;
                let half = w.dot(dir);
                let disc = half * half - (w.dot(w) - big * big);
                if disc >= 0.0 {
                    let root = disc.sqrt();
                    out.push(-half - root);
                    out.push(-half + root);
                }
            }
        }
        (
            Section::Circle {
                centre: ca, r: ra, ..
            },
            _,
        ) => {
            let Some(phase) = circle_phase(a, b) else {
                return out;
            };
            let cosine = match *b {
                Section::Line { n, c, .. } => (v - (n.dot(ca) - c)) / (ra * n.length()),
                Section::Circle {
                    centre: cb, r: rb, ..
                } => {
                    let d = (ca - cb).length();
                    let big = rb + v;
                    if big <= 0.0 {
                        return out;
                    }
                    (big * big - d * d - ra * ra) / (2.0 * ra * d)
                }
            };
            if (-1.0..=1.0).contains(&cosine) {
                let turn = det::acos(cosine);
                out.extend(
                    [phase + turn, phase - turn]
                        .into_iter()
                        .filter_map(|t| a.step_at_angle(t)),
                );
            }
        }
    }
    out
}

/// The least and greatest gap of `b` at the points of `a` over the steps `lo..=hi`: the
/// gap at the ends and at the gap's stationary points inside, which are where its
/// extremes lie (a line's gap from a plane is linear, its distance from a circle's centre
/// convex with its least value where the line is square to the centre, a circle's gap a
/// function increasing in `cos(theta - phase)`). `None` when a value is not finite.
fn extremes(a: &Section, b: &Section, lo: f64, hi: f64) -> Option<(f64, f64)> {
    let mut steps = vec![lo, hi];
    match (*a, *b) {
        (Section::Line { foot, dir, .. }, Section::Circle { centre, .. }) => {
            steps.push(dir.dot(centre - foot));
        }
        (Section::Circle { .. }, _) => {
            if let Some(phase) = circle_phase(a, b) {
                steps.extend(
                    [phase, phase + PI]
                        .into_iter()
                        .filter_map(|t| a.step_at_angle(t)),
                );
            }
        }
        (Section::Line { .. }, Section::Line { .. }) => {}
    }
    let mut out: Option<(f64, f64)> = None;
    for s in steps.into_iter().filter(|s| (lo..=hi).contains(s)) {
        let g = b.gap(a.point(s));
        if !g.is_finite() {
            return None;
        }
        out = Some(out.map_or((g, g), |(l, h)| (l.min(g), h.max(g))));
    }
    out
}

/// One sector round the edge: the indices of its two faces' sections and whether
/// the faces leave the edge tangent to each other.
#[derive(Clone, Copy, Debug)]
struct Pair {
    a: usize,
    b: usize,
    tangent: bool,
}

/// The step in `(s0, s_max]` at which test 9 reads the faces, or `None` to keep the
/// reading at `s0`. A step `sigma` is taken when, for every pair, tangent or not, and
/// each of its faces against the other, over `0..=sigma`, from the face's foot at the edge
/// out, the gap is never past the contact tolerance on both sides (else the faces cross
/// each other and the order depends on the step: `None`). The stretch inside `s0` counts
/// too: a dip past the tolerance at the edge that comes back within it before `s0` is a
/// crossing. A pair that is not tangent counts because its drift bound below holds only
/// at `sigma`: feet offset from the edge point at a small angle can put the faces in one
/// order at `s0` and the other at `sigma`. And for every tangent pair and each of its
/// faces against the other:
/// - at `sigma` the gap is past the contact tolerance (`within_contact_tol` fails);
/// - at `sigma` the turn from one face's direction to the other's has the sign the gap
///   gives (the other face lies on that side of the first one's surface);
///
/// and for every other pair the angle between their directions at `sigma` is within the
/// drift bound of their angle at the edge, which is that far from 0 and 360 degrees. The
/// candidates are `s0` and the closed-form steps at which a gap reaches the tolerance plus
/// twice the rounding allowance; `sigma` is the least that passes.
///
/// Rounding: every gap here is a handful of products, sums, one square root and, for a
/// circle, a sine, cosine and arcsine (one rounding each, correctly rounded to within an
/// ulp), on coordinates at most `size`; their error is far below `slack = gamma(32) size`.
/// The foot (`s = 0`) takes `asin(0) = 0` exactly, so its gap has no more roundings than
/// any other step's and the same allowance covers it. Past the contact tolerance means
/// past it as `within_contact_tol` reads it, both for the crossing and for the gaps at
/// `sigma`: that is the reading the Boolean that made the faces tangent along the edge
/// used, so a dip of the tolerance itself, or past it by no more than the rounding of
/// copies, is a touch here too and not a crossing (#21104: a cylinder sunk exactly 1e-5
/// into a box face was built tangent and then read as crossing it).
fn choose_step(
    sections: &[Section],
    pairs: &[Pair],
    s0: f64,
    s_max: f64,
    contact: f64,
    size: f64,
) -> Option<f64> {
    if !(s0 > 0.0 && s0 < s_max && contact > 0.0) {
        return None;
    }
    let slack = gamma(32) * size;
    let mut faces = Vec::new();
    let mut crossing = Vec::new();
    for p in pairs {
        let (a, b) = (sections.get(p.a)?, sections.get(p.b)?);
        crossing.push((a, b));
        crossing.push((b, a));
        if p.tangent {
            faces.push((a, b));
            faces.push((b, a));
        }
    }
    let mut candidates = vec![s0];
    for &(x, y) in &faces {
        for v in [contact + 2.0 * slack, -(contact + 2.0 * slack)] {
            candidates.extend(roots(x, y, v).into_iter().filter(|&s| s > s0 && s <= s_max));
        }
    }
    candidates.sort_by(f64::total_cmp);
    candidates.dedup();
    let mut chosen = None;
    for &sigma in &candidates {
        for &(x, y) in &crossing {
            let (lo, hi) = extremes(x, y, 0.0, sigma)?;
            if !within_contact_tol(hi, contact, size) && !within_contact_tol(-lo, contact, size) {
                return None;
            }
        }
        let mut apart = true;
        for &(x, y) in &faces {
            let g = y.gap(x.point(sigma));
            if !g.is_finite() {
                return None;
            }
            if within_contact_tol(g.abs(), contact, size) {
                apart = false;
            }
        }
        if apart {
            chosen = Some(sigma);
            break;
        }
    }
    let sigma = chosen?;
    if sigma <= s0 {
        return None;
    }
    for p in pairs {
        let (a, b) = (sections.get(p.a)?, sections.get(p.b)?);
        let (pa, pb) = (a.point(sigma), b.point(sigma));
        let turn = cross(pa, pb);
        if p.tangent {
            if turn
                .abs()
                .partial_cmp(&(slack * (pa.length() + pb.length())))
                != Some(Ordering::Greater)
            {
                return None;
            }
            for (x, px, py, sign) in [(a, pa, pb, 1.0), (b, pb, pa, -1.0)] {
                let grad = x.gradient(px)?;
                let lean = cross(px, grad) / px.length();
                if !matches!(
                    lean.abs().partial_cmp(&0.5),
                    Some(Ordering::Greater | Ordering::Equal)
                ) {
                    return None;
                }
                let want = x.gap(py).signum() * lean.signum();
                if want != (sign * turn).signum() {
                    return None;
                }
            }
        } else {
            let (ta, tb) = (a.tangent(), b.tangent());
            let mut at_edge = det::atan2(cross(ta, tb), ta.dot(tb));
            if at_edge < 0.0 {
                at_edge += 2.0 * PI;
            }
            let drift = a.drift(sigma)? + b.drift(sigma)? + 64.0 * f64::EPSILON;
            if !(at_edge > drift && 2.0 * PI - at_edge > drift) {
                return None;
            }
            let mut read = det::atan2(turn, pa.dot(pb));
            if read < 0.0 {
                read += 2.0 * PI;
            }
            if !matches!(
                (read - at_edge).abs().partial_cmp(&drift),
                Some(Ordering::Less | Ordering::Equal)
            ) {
                return None;
            }
        }
    }
    Some(sigma)
}

/// The section of `surface` by the plane through `p` square to `t`, in the frame
/// `e1`, `e2`, with its side chosen by the edge use's binormal; with the largest
/// coordinate of its definition. `None` for a surface that is not a plane or a cylinder
/// along `t`, or a binormal that does not lie along the section.
fn section(
    surface: &Surface,
    p: Point3,
    frame: [Vec3; 3],
    binormal: Vec3,
) -> Option<(Section, f64)> {
    let [t, e1, e2] = frame;
    let b2 = Vec2::new(binormal.dot(e1), binormal.dot(e2));
    let bl = b2.length();
    if bl.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return None;
    }
    let side = |tangent: Vec2| {
        let along = tangent.dot(b2) / bl;
        (along.abs() >= 0.5).then_some(along.signum())
    };
    match surface.kind() {
        SurfaceKind::Plane(plane) => {
            let n3 = plane.position().z_axis();
            let o = plane.position().origin();
            let n = Vec2::new(n3.dot(e1), n3.dot(e2));
            let nl = n.length();
            if !matches!(
                nl.partial_cmp(&0.5),
                Some(Ordering::Greater | Ordering::Equal)
            ) {
                return None;
            }
            let c = n3.dot(o - p);
            let tangent = Vec2::new(-n.y / nl, n.x / nl);
            let s = side(tangent)?;
            Some((
                Section::Line {
                    n,
                    c,
                    foot: n * (c / (nl * nl)),
                    dir: s * tangent,
                },
                o.max_dimension(),
            ))
        }
        SurfaceKind::Cone(cone) if cone.semi_angle_deg() == 0.0 => {
            let position = cone.rev().position();
            let a = position.z_axis();
            let o = position.origin();
            let r = cone.base_radius().abs();
            if !(r > 0.0 && a.cross(t).length() <= AXIS_LEAN) {
                return None;
            }
            let w = o + (t.dot(p - o) / t.dot(a)) * a - p;
            let centre = Vec2::new(w.dot(e1), w.dot(e2));
            if centre.length().partial_cmp(&0.0) != Some(Ordering::Greater) {
                return None;
            }
            let foot_angle = det::atan2(-centre.y, -centre.x);
            let tangent = Vec2::new(-det::sin(foot_angle), det::cos(foot_angle));
            let sense = side(tangent)?;
            Some((
                Section::Circle {
                    centre,
                    r,
                    foot_angle,
                    sense,
                },
                o.max_dimension() + r,
            ))
        }
        _ => None,
    }
}

/// How far from `p` the faces' boundaries other than `e` stay: the least over the
/// other edges of `faces` of the distance from `p` to the edge's certified box less
/// the edge's tolerance, and over the vertices of those edges and of `e` of the
/// distance less the vertex's tolerance. `None` where an edge has no curve or no
/// certified box, or where reading those boundaries or `e`'s vertices fails
/// ([`boundary_clearance`]).
///
/// # Errors
///
/// The unported, cancelled and preparation errors [`ignored`] passes on.
fn clearance(k: &Kernel, b: &Brep, e: EdgeId, p: Point3, faces: &[FaceId]) -> Result<Option<f64>> {
    // The edge check of `e` never reads the faces' other edges or their vertices on
    // WR-540's reading, nor `e`'s vertex records beyond the ones it looks up itself, so
    // a failure here is inconclusive and keeps that reading. Their own checks (the face,
    // loop, edge and vertex checks of the same run) read them as before, so no fault is
    // hidden from the run.
    Ok(ignored(boundary_clearance(k, b, e, p, faces))?.flatten())
}

/// Lowers `free` to the distance from `p` of each vertex of `edge` less its tolerance.
fn vertex_clearance(b: &Brep, edge: EdgeId, p: Point3, free: &mut f64) -> Result<()> {
    for v in tedge::vertices(b, edge, None)?.into_iter().flatten() {
        let v = b.vertex(v)?;
        *free = free.min(v.point.distance(p) - v.tolerance);
    }
    Ok(())
}

/// [`clearance`] over `e`'s vertices and the edges of `faces` other than `e` and their
/// vertices: `None` where an edge has no curve or no certified box.
///
/// # Errors
///
/// Those of the loop walks, the lookups and the boxes, which [`clearance`] reads as
/// inconclusive.
fn boundary_clearance(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    p: Point3,
    faces: &[FaceId],
) -> Result<Option<f64>> {
    let at = Extent3d::from_points(p, p);
    let mut free = f64::INFINITY;
    vertex_clearance(b, e, p, &mut free)?;
    for &f in faces {
        for edge in tface::p4::edges(b, f, None, false, None)? {
            if edge == e {
                continue;
            }
            let rec = b.edge(edge)?;
            let Some(curve) = rec.curve.as_ref() else {
                return Ok(None);
            };
            let Some(bx) = curve.certified_bounding_box(k, &rec.interval)? else {
                return Ok(None);
            };
            free = free.min(at.min_distance(&bx) - tedge::tolerance(b, edge)?);
            vertex_clearance(b, edge, p, &mut free)?;
        }
    }
    Ok(Some(free))
}

/// The directions test 9 reads round edge `e` at parameter `t` (not upstream; #18639):
/// for every edge use, the unit direction from the edge point to its face's point at the
/// step [`choose_step`] certifies, square to the edge. `None` keeps WR-540's reading: the
/// edge is not a line, a face is not a plane or a cylinder along it, `sectors` (each
/// sector's two edge uses and whether they leave the edge tangent) does not cover every
/// edge use, a face's boundary comes within the step, reading the faces' other
/// boundaries or `e`'s vertices fails, a binormal fails, or no step is certified.
///
/// # Errors
///
/// Those of the lookups of `e`, its edge uses and their faces, and of the evaluation of
/// `e`'s curve at `t`: the edge check's own reading makes the same lookups and
/// evaluation and passes their errors on. And the unported, cancelled and preparation
/// errors [`ignored`] passes on.
pub(super) fn tangent_directions(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    t: f64,
    edge_tol: f64,
    sectors: &[(EdgeuseId, EdgeuseId, bool)],
) -> Result<Option<Vec<(EdgeuseId, Vec3)>>> {
    if !sectors.iter().any(|s| s.2) {
        return Ok(None);
    }
    let (p, dir) = {
        let rec = b.edge(e)?;
        let Some(curve) = rec.curve.as_ref() else {
            return Ok(None);
        };
        if !matches!(curve.kind(), CurveKind::Line(_)) {
            return Ok(None);
        }
        let pv = curve.evaluate(k, t, 1, true, true)?;
        let (Some(&p), Some(&d)) = (pv.first(), pv.get(1)) else {
            return Ok(None);
        };
        (p, d)
    };
    let Ok(tan) = dir.unitize() else {
        return Ok(None);
    };
    let helper = if tan.x.abs() <= tan.y.abs() && tan.x.abs() <= tan.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if tan.y.abs() <= tan.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let Ok(e1) = tan.cross(helper).unitize() else {
        return Ok(None);
    };
    let e2 = tan.cross(e1);
    let frame = [tan, e1, e2];

    let mut uses: Vec<EdgeuseId> = Vec::new();
    let mut faces: Vec<FaceId> = Vec::new();
    let mut sections = Vec::new();
    let mut size = p.max_dimension();
    for &(x, y, _) in sectors {
        for eu in [x, y] {
            if uses.contains(&eu) {
                continue;
            }
            let Some(f) = eu1::face(b, eu)? else {
                return Ok(None);
            };
            let Some(bn) = ignored(evaluate_binormal(
                k,
                b,
                eu,
                t,
                true,
                &BinormalOptions::default(),
            ))?
            else {
                return Ok(None);
            };
            let Some(surface) = b.face(f)?.surface.as_ref() else {
                return Ok(None);
            };
            let Some((s, scale)) = section(surface, p, frame, bn.binormal) else {
                return Ok(None);
            };
            size = size.max(scale);
            uses.push(eu);
            sections.push(s);
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    if uses.len() != b.edge(e)?.edgeuses.len() {
        return Ok(None);
    }
    let index = |eu: EdgeuseId| uses.iter().position(|&u| u == eu);
    let mut pairs = Vec::new();
    for &(x, y, tangent) in sectors {
        let (Some(a), Some(b)) = (index(x), index(y)) else {
            return Ok(None);
        };
        pairs.push(Pair { a, b, tangent });
    }

    let Some(free) = clearance(k, b, e, p, &faces)? else {
        return Ok(None);
    };
    let reach = sections
        .iter()
        .fold(0.0_f64, |m, s| m.max(s.foot().length()));
    let cap = sections
        .iter()
        .fold(free - reach, |m, s| m.min(s.max_step()));
    if !cap.is_finite() {
        return Ok(None);
    }
    let size = size + cap.abs();
    let s_max = cap - gamma(32) * size;
    let Some(sigma) = choose_step(
        &sections,
        &pairs,
        20.0 * edge_tol,
        s_max,
        contact_tol_3d(k),
        size,
    ) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for (&eu, s) in uses.iter().zip(&sections) {
        let q = s.point(sigma);
        let Ok(d) = (q.x * e1 + q.y * e2).unitize() else {
            return Ok(None);
        };
        out.push((eu, d));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: f64 = 1e-5;

    /// The #18639 sections at the edge point `(1, 0)`: the plane `x = 1 - d` leaving
    /// towards `+y` (`Line`) and the cylinder of radius 1 about the origin leaving
    /// towards `+y` (`Circle`), in the frame `e1 = x`, `e2 = y`.
    fn issue(d: f64) -> [Section; 2] {
        [
            Section::Line {
                n: Vec2::new(1.0, 0.0),
                c: -d,
                foot: Vec2::new(-d, 0.0),
                dir: Vec2::new(0.0, 1.0),
            },
            Section::Circle {
                centre: Vec2::new(-1.0, 0.0),
                r: 1.0,
                foot_angle: 0.0,
                sense: 1.0,
            },
        ]
    }

    fn step(d: f64) -> Option<f64> {
        let pairs = [Pair {
            a: 0,
            b: 1,
            tangent: true,
        }];
        choose_step(&issue(d), &pairs, 20.0 * L, 0.4, L, 1.4)
    }

    /// Within the contact tolerance the step is the closed-form one where both gaps
    /// first pass the tolerance plus twice the rounding allowance: the plane's points
    /// reach `|X| - 1 = v` at `s = sqrt((1 + v)^2 - (1 - d)^2)` and the cylinder's points
    /// reach `cos(theta) - (1 - d) = -v` at the chord `sqrt(2 (d + v))`.
    #[test]
    fn a_plane_within_the_contact_tolerance_of_a_cylinder_is_read_past_its_band() {
        let v = L + 2.0 * gamma(32) * 1.4;
        // `L` itself is a touch by the contact rule (#21104).
        for d in [0.0, 1e-6, 5e-6, 9e-6, L] {
            let want = ((1.0 + v) * (1.0 + v) - (1.0 - d) * (1.0 - d))
                .sqrt()
                .max((2.0 * (d + v)).sqrt());
            let got = step(d).unwrap();
            assert!(
                (got - want).abs() <= 1e-9 * want,
                "d {d:e}: {got} against {want}"
            );
        }
    }

    /// Past the contact tolerance the faces lie apart on one side at the first step and
    /// on the other side farther out: they cross, so the reading stays at the first step,
    /// where the plane reads inside the cylinder. `L + 1e-12` is past the tolerance by
    /// more than the contact rule's rounding allowance (`gamma(32) 1.4`, about 5e-15).
    #[test]
    fn a_plane_dipping_past_the_contact_tolerance_keeps_the_first_step() {
        for d in [L + 1e-12, 1.1e-5, 2e-5, 3.5e-5, 4e-5] {
            assert_eq!(step(d), None, "d {d:e}");
        }
    }

    /// Faces that never part within the reach keep the first step: one plane twice, and
    /// a reach too short for the band.
    #[test]
    fn faces_that_never_part_keep_the_first_step() {
        let [plane, cylinder] = issue(0.0);
        let pairs = [Pair {
            a: 0,
            b: 1,
            tangent: true,
        }];
        assert_eq!(
            choose_step(&[plane, plane], &pairs, 20.0 * L, 0.4, L, 1.4),
            None
        );
        assert_eq!(
            choose_step(&[plane, cylinder], &pairs, 20.0 * L, 1e-3, L, 1.4),
            None
        );
    }

    /// The plane `x = -dp` leaving towards `+y` and the cylinder of radius `r` whose wall
    /// lies `h` out from the edge point, `(h, 0)`, leaving towards `+y`: the faces overlap
    /// by `dp + h` at the edge, the plane `dp` and the cylinder `h` from the edge point.
    fn split(r: f64, dp: f64, h: f64) -> [Section; 2] {
        [
            Section::Line {
                n: Vec2::new(1.0, 0.0),
                c: -dp,
                foot: Vec2::new(-dp, 0.0),
                dir: Vec2::new(0.0, 1.0),
            },
            Section::Circle {
                centre: Vec2::new(h - r, 0.0),
                r,
                foot_angle: 0.0,
                sense: 1.0,
            },
        ]
    }

    fn split_step(r: f64, dp: f64, h: f64) -> Option<f64> {
        let pairs = [Pair {
            a: 0,
            b: 1,
            tangent: true,
        }];
        choose_step(&split(r, dp, h), &pairs, 20.0 * L, 0.4 * r, L, 1.4)
    }

    /// Small cylinders, and overlaps split between the plane and the cylinder, within the
    /// contact tolerance are still read past their band, at the closed-form step: with
    /// `o = dp + h`, the plane's points reach the cylinder's gap `v` at
    /// `s = sqrt((r + v)^2 - (r - o)^2)` and the cylinder's points (`1 - cos(theta) =
    /// (o + v) / r`) reach the plane's gap `-v` at the chord `sqrt(2 r (o + v))`.
    #[test]
    fn small_cylinders_and_split_overlaps_within_the_contact_tolerance_are_read_past_their_band() {
        let v = L + 2.0 * gamma(32) * 1.4;
        for r in [1.0, 0.01, 0.002] {
            for o in [0.0, 1e-6, 5e-6, 9e-6] {
                for (dp, h) in [(o, 0.0), (0.5 * o, 0.5 * o), (0.0, o)] {
                    let want = ((r + v) * (r + v) - (r - o) * (r - o))
                        .sqrt()
                        .max((2.0 * r * (o + v)).sqrt());
                    let got = split_step(r, dp, h).unwrap();
                    assert!(
                        (got - want).abs() <= 1e-9 * want,
                        "r {r}, plane {dp:e}, cylinder {h:e}: {got} against {want}"
                    );
                }
            }
        }
    }

    /// Real defects at small radii and split between the faces: the faces overlap past
    /// the contact tolerance at the edge (each face may still lie within the tolerance of
    /// the edge point) and lie more than the tolerance apart the other way round within
    /// the reach. They cross, so no step is certified and the reading stays at the first
    /// step, whatever the radius. At `r = 0.002` such a dip comes back within the
    /// tolerance before the first step, `20 L` out (the dip less `s0^2 / 2r` is under `L`
    /// for dips up to `L + 1e-5`), so only the stretch from the edge shows the crossing.
    #[test]
    fn small_cylinders_and_split_overlaps_past_the_contact_tolerance_keep_the_first_step() {
        for r in [1.0, 0.01, 0.002] {
            for o in [1.001e-5, 1.1e-5, 1.5e-5, 1.9e-5, 2e-5, 3.5e-5] {
                for (dp, h) in [(o, 0.0), (0.5 * o, 0.5 * o), (0.0, o)] {
                    let case = format!("r {r}, plane {dp:e}, cylinder {h:e}");
                    let [plane, cylinder] = split(r, dp, h);
                    // The premises: past the tolerance one way at the edge, the other way
                    // within the reach.
                    assert!(cylinder.gap(plane.point(0.0)) < -L, "{case}");
                    assert!(cylinder.gap(plane.point(0.4 * r)) > L, "{case}");
                    assert_eq!(split_step(r, dp, h), None, "{case}");
                }
            }
        }
    }

    /// The #18639 plane and cylinder (`d = 5e-6`, within the contact tolerance) with a
    /// third face, a plane leaving the edge `1.15` degrees counterclockwise of them whose
    /// foot lies `f` out from the edge point along its normal; the sectors are plane to
    /// cylinder (tangent), cylinder to third plane and third plane to plane.
    fn with_neighbour(f: f64) -> ([Section; 3], [Pair; 3]) {
        let [plane, cylinder] = issue(5e-6);
        let a = 1.15_f64.to_radians();
        let n = Vec2::new(det::cos(a), det::sin(a));
        let third = Section::Line {
            n,
            c: f,
            foot: n * f,
            dir: Vec2::new(-det::sin(a), det::cos(a)),
        };
        let pairs = [
            Pair {
                a: 0,
                b: 1,
                tangent: true,
            },
            Pair {
                a: 1,
                b: 2,
                tangent: false,
            },
            Pair {
                a: 2,
                b: 0,
                tangent: false,
            },
        ];
        ([plane, cylinder, third], pairs)
    }

    /// A face that is not tangent to its neighbour but crosses it past the contact
    /// tolerance on both sides between the edge and the step (its foot `1.9e-5` out, the
    /// plane's points `2.4e-5` behind it at the edge and more than the tolerance in front
    /// of it at the step) reads one way round at the first step and the other way round
    /// at the step, while its drift bound holds at the step. The order would depend on
    /// the step, so the reading stays at the first step. The same face through the edge
    /// point (the gap at the edge `5e-6 cos(1.15 deg)`, within the tolerance) does not
    /// cross, and the step is the one of the tangent pair alone.
    #[test]
    fn a_face_crossing_a_non_tangent_neighbour_keeps_the_first_step() {
        let s0 = 20.0 * L;
        let ([plane, cylinder, third], pairs) = with_neighbour(1.9e-5);
        let sigma = step(5e-6).unwrap();
        // The premises: the third plane lies past the tolerance behind the plane at the
        // edge and in front of it at the step, and the order read flips in between.
        assert!(third.gap(plane.point(0.0)) < -L);
        assert!(third.gap(plane.point(sigma)) > L);
        assert!(cross(plane.point(s0), third.point(s0)) < 0.0);
        assert!(cross(plane.point(sigma), third.point(sigma)) > 0.0);
        assert_eq!(
            choose_step(&[plane, cylinder, third], &pairs, s0, 0.4, L, 1.4),
            None
        );

        let (sections, pairs) = with_neighbour(0.0);
        assert_eq!(choose_step(&sections, &pairs, s0, 0.4, L, 1.4), Some(sigma));
    }

    /// The tangent pair's order at the chosen step is the side the gaps give: the plane
    /// lies outside the cylinder, so the cylinder's direction is counterclockwise of the
    /// plane's, towards the inside (a positive turn from the plane to the cylinder about
    /// `e1 x e2`).
    #[test]
    fn the_order_read_is_the_side_past_the_band() {
        let [plane, cylinder] = issue(5e-6);
        let sigma = step(5e-6).unwrap();
        assert!(cylinder.gap(plane.point(sigma)) > L);
        assert!(plane.gap(cylinder.point(sigma)) < -L);
        assert!(cross(plane.point(sigma), cylinder.point(sigma)) > 0.0);
    }

    /// The sectors `radial_order` walks round `e` at `t`, with its tie test.
    fn walk(k: &Kernel, b: &mut Brep, e: EdgeId, t: f64) -> Vec<(EdgeuseId, EdgeuseId, bool)> {
        let tie_dot = det::cos(crate::geom::tol::ANG_TOL_DEG * PI / 180.0);
        let options = BinormalOptions::default();
        let target = eu1::primary_edgeuse(b, e).unwrap().unwrap();
        let uses = b.edge(e).unwrap().edgeuses.len();
        let mut out = Vec::new();
        let mut eu = target;
        while let Some(rad) = eu1::radial(b, eu).unwrap() {
            let b0 = evaluate_binormal(k, b, eu, t, true, &options).unwrap();
            let b1 = evaluate_binormal(k, b, rad, t, true, &options).unwrap();
            let (u0, u1) = (
                b0.binormal.unitize().unwrap(),
                b1.binormal.unitize().unwrap(),
            );
            out.push((eu, rad, u0.dot(u1) > tie_dot));
            match eu1::mate(b, rad).unwrap() {
                Some(next) if next != target && out.len() < uses => eu = next,
                _ => break,
            }
        }
        out
    }

    /// #20695 follow-up: the step reads the boxes of the faces' other edges, which the
    /// edge check never read before it. A box that cannot be computed (a neighbouring
    /// edge's B-spline curve without NURB data, whose bounding box fails) is
    /// inconclusive: the directions are `None`, so test 9 keeps WR-540's reading at 20
    /// edge tolerances, instead of the edge check failing with the box's error. The body
    /// is the #18639 union (a box face 5e-6 inside a cylinder of radius 1), whose step
    /// is certified while the neighbour is intact.
    #[test]
    fn a_neighbouring_edge_whose_box_fails_keeps_the_first_step() {
        use crate::boolean::api_brep::boolean_union;
        use crate::curve::curve::Curve;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::primitive::api_primitives::{create_box, create_cylinder};

        let mut k = Kernel::default();
        let cylinder = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0).unwrap();
        let block = create_box(&mut k, Point3::new(1.0 - 5e-6, -0.5, 0.0), 1.0, 1.0, 1.0).unwrap();
        let mut u = boolean_union(&mut k, cylinder, block).unwrap();
        let line = u
            .edges()
            .ids()
            .find(|&e| u.edge(e).unwrap().edgeuses.len() == 8)
            .unwrap();
        let t = u.edge(line).unwrap().interval.evaluate(0.456);
        let tol = tedge::tolerance(&u, line).unwrap();
        let sectors = walk(&k, &mut u, line, t);
        assert!(sectors.iter().any(|s| s.2), "{sectors:?}");
        let intact = tangent_directions(&k, &mut u, line, t, tol, &sectors).unwrap();
        assert_eq!(intact.map(|d| d.len()), Some(8));

        let face = eu1::face(&u, sectors[0].0).unwrap().unwrap();
        let other = tface::p4::edges(&u, face, None, false, None)
            .unwrap()
            .into_iter()
            .find(|&x| x != line)
            .unwrap();
        u.edge_mut(other).unwrap().curve = Some(Curve::from(BSplineCurve::default()));
        let rec = u.edge(other).unwrap();
        let curve = rec.curve.as_ref().unwrap();
        assert!(curve.certified_bounding_box(&k, &rec.interval).is_err());
        assert!(tangent_directions(&k, &mut u, line, t, tol, &sectors)
            .unwrap()
            .is_none());
    }
}
