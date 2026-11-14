//! The Roll-over's torus in its own frame, the cliff's profile in the plane square to its
//! axis, and the creases as curves: cubic Bézier pieces, polynomial on a plane, rational
//! with the profile circle's weights on a cylinder, so that each lies exactly on its
//! cliff face's surface (to the rounding of its control points) and within its certified
//! bound of the torus.
//!
//! Upstream: none.
//!
//! | Rust | What |
//! |------|------|
//! | [`TorusFrame`] | the torus: centre, axis, radii, and two directions square to the axis |
//! | [`Profile`], [`CliffShape`] | the cliff faces' profiles: the face past E's rim (a plane) and the round's other face (a plane or a cylinder parallel to the axis) |
//! | [`Profile::pieces`] | the crease's pieces: from where the rail leaves E (the torus's rim, `rho = R`) to the seam between the cliff faces, and from there to the tangent point on the round |
//! | [`crease_curve`] | a piece's certified fit as a 3D curve |
//! | [`torus_creases`] | the creases of an exact torus with one cliff face (a plane or a cylinder parallel to its axis, touching the round), each piece certified |
//!
//! Deviations: none (not a port). No recursion; bounded loops.

use super::crease::{fit_certified, Fit, Piece, Radii};
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::blend_corner::rollover::geometry";

/// A refusal: the geometry is not one the Roll-over's creases are built for.
const fn failed(why: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: why,
    }
}

/// The Roll-over's torus: centre on the convex round's axis A1, in the plane of the
/// concave balls' centres; `axis` A1's direction towards the end face E; `up` and `side`
/// unit and square to it and to each other. A torus point at distance `rho` from A1 in the
/// direction `(cos t) up + (sin t) side` has height `h` along `axis` with `h^2 = (rho -
/// r1)(r2 + R - rho)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TorusFrame {
    /// The centre.
    pub centre: Point3,
    /// The unit axis, towards E.
    pub axis: Vec3,
    /// A unit direction square to the axis.
    pub up: Vec3,
    /// The unit direction `axis x up`.
    pub side: Vec3,
    /// The radii.
    pub radii: Radii,
}

impl TorusFrame {
    /// The frame with `up` taken from `towards` (its part square to `axis`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unsupported`] for an axis or a `towards` of no direction square to
    /// it, or radii not finite and positive.
    pub fn new(centre: Point3, axis: Vec3, towards: Vec3, radii: Radii) -> Result<Self> {
        let ok = radii.r1 > 0.0 && radii.r2 > 0.0 && radii.r1.is_finite() && radii.r2.is_finite();
        if !ok {
            return Err(failed("the torus's radii are not finite and positive"));
        }
        let axis = axis.unitize()?;
        let up = (towards - axis * towards.dot(axis)).unitize()?;
        let side = axis.cross(up);
        Ok(Self {
            centre,
            axis,
            up,
            side,
            radii,
        })
    }

    /// `p`'s coordinates along `up` and `side` about the centre (its projection on the
    /// plane square to the axis).
    #[must_use]
    pub fn local(&self, p: Point3) -> [f64; 2] {
        let d = p - self.centre;
        [d.dot(self.up), d.dot(self.side)]
    }

    /// `p`'s height along the axis over the centre's plane.
    #[must_use]
    pub fn height(&self, p: Point3) -> f64 {
        (p - self.centre).dot(self.axis)
    }

    /// The point at local `q` and height `h`.
    #[must_use]
    pub fn at(&self, q: [f64; 2], h: f64) -> Point3 {
        self.centre + self.up * q[0] + self.side * q[1] + self.axis * h
    }

    /// The direction of local `v`.
    #[must_use]
    pub fn dir(&self, v: [f64; 2]) -> Vec3 {
        self.up * v[0] + self.side * v[1]
    }
}

/// The round's other face's profile, square to the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CliffShape {
    /// A plane parallel to the axis: its profile the line through `at` along `dir`
    /// (local coordinates, `dir` unit).
    Line {
        /// A point.
        at: [f64; 2],
        /// The unit direction.
        dir: [f64; 2],
    },
    /// A cylinder parallel to the axis: its profile the circle of `radius` about `centre`.
    Circle {
        /// The centre.
        centre: [f64; 2],
        /// The radius.
        radius: f64,
    },
}

/// The cliff faces' profiles: `wall`, the face past E's rim, a plane parallel to the
/// axis, its line through `seam` along `wall_dir` (unit, pointing away from the round's
/// other face); `round_face`, the round's other face, tangent to `wall` at `seam` and to
/// the round's circle at its tangent point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// The seam between the two cliff faces (local).
    pub seam: [f64; 2],
    /// The wall's direction from the seam, away from the round's other face (unit).
    pub wall_dir: [f64; 2],
    /// The round's other face.
    pub round_face: CliffShape,
}

/// One crease piece: its profile piece, and whether it lies on the wall (else on the
/// round's other face).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreasePiece {
    /// The profile piece, from the rail's exit or the seam to the seam or the tangent point.
    pub piece: Piece,
    /// On the wall (the face past E's rim), else on the round's other face.
    pub on_wall: bool,
}

fn dot2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn norm2(a: [f64; 2]) -> f64 {
    dot2(a, a).sqrt()
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add2(a: [f64; 2], b: [f64; 2], s: f64) -> [f64; 2] {
    [a[0] + s * b[0], a[1] + s * b[1]]
}

impl Profile {
    /// The tangent point qA of the round's other face with the round's circle (radius
    /// `r1` about the origin): the foot of the origin on a line, the point of a circle
    /// towards the origin.
    #[must_use]
    pub fn tangent_point(&self, r1: f64) -> [f64; 2] {
        match self.round_face {
            CliffShape::Line { at, dir } => add2(at, dir, -dot2(at, dir)),
            CliffShape::Circle { centre, radius } => {
                let d = norm2(centre);
                let _ = radius;
                [centre[0] * r1 / d, centre[1] * r1 / d]
            }
        }
    }

    /// The crease's pieces for `radii`: where the torus's rim (`rho = R`, the rail on E)
    /// crosses the profile, the piece on the wall to the seam (if the rail leaves E across
    /// the wall's rim), then the piece on the round's other face from the seam (or from
    /// the rail's exit, if it leaves E across that face) to the tangent point.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unsupported`] where the profile is not the Roll-over's: the round's
    /// other face not tangent to the round's circle (to `tol`), the seam outside the
    /// torus's rim, a circle not touching the round from outside, or an arc of half a turn
    /// or more.
    pub fn pieces(&self, radii: Radii, tol: f64) -> Result<Vec<CreasePiece>> {
        let (r1, big_r) = (radii.r1, radii.major());
        let finite = |p: [f64; 2]| p.iter().all(|c| c.is_finite());
        if !(finite(self.seam) && finite(self.wall_dir) && tol > 0.0 && tol.is_finite()) {
            return Err(failed("the cliff's profile is not finite"));
        }
        let qa = self.tangent_point(r1);
        if !(finite(qa) && (norm2(qa) - r1).abs() <= tol) {
            return Err(failed(
                "the round's other face does not run on tangentially from the round",
            ));
        }
        let rho_seam = norm2(self.seam);
        let mut out = Vec::new();
        // the rail leaves E across the wall's rim where the seam lies inside the rim
        let mut from = self.seam;
        if rho_seam < big_r {
            // |seam + t dir| = R, t > 0
            let b = dot2(self.seam, self.wall_dir);
            let c = rho_seam * rho_seam - big_r * big_r;
            let disc = b * b - c;
            let crosses = disc >= 0.0;
            if !crosses {
                return Err(failed("the torus's rim does not cross the wall"));
            }
            let t = -b + disc.sqrt();
            let ahead = t > 0.0;
            if !ahead {
                return Err(failed("the torus's rim crosses the wall behind the seam"));
            }
            let exit = add2(self.seam, self.wall_dir, t);
            out.push(CreasePiece {
                piece: Piece::Segment {
                    a: exit,
                    b: self.seam,
                },
                on_wall: true,
            });
        } else {
            // the rail leaves E across the round's other face: from its crossing with
            // the rim, between the seam and the tangent point
            from = match self.round_face {
                CliffShape::Line { at, dir } => {
                    let b = dot2(at, dir);
                    let c = dot2(at, at) - big_r * big_r;
                    let disc = b * b - c;
                    let crosses = disc >= 0.0;
                    if !crosses {
                        return Err(failed("the torus's rim does not cross the round's face"));
                    }
                    // the root on the seam's side of the tangent point
                    let s_seam = dot2(sub2(self.seam, at), dir);
                    let root = disc.sqrt();
                    let t = if s_seam >= -b { -b + root } else { -b - root };
                    add2(at, dir, t)
                }
                CliffShape::Circle { centre, radius } => {
                    // |c + radius u| = R with u towards the seam's side
                    let d = norm2(centre);
                    let cos_a = (big_r * big_r - d * d - radius * radius) / (2.0 * d * radius);
                    let crosses = cos_a.abs() <= 1.0;
                    if !crosses {
                        return Err(failed("the torus's rim does not cross the round's face"));
                    }
                    let to_a = [-centre[0] / d, -centre[1] / d];
                    let perp = [-to_a[1], to_a[0]];
                    let toward = sub2(self.seam, centre);
                    let sign = if dot2(toward, perp) >= 0.0 { 1.0 } else { -1.0 };
                    let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt() * sign;
                    // the cosine is from the direction away from the axis, `-to_a`
                    let u = [
                        -to_a[0] * cos_a + perp[0] * sin_a,
                        -to_a[1] * cos_a + perp[1] * sin_a,
                    ];
                    add2(centre, u, radius)
                }
            };
        }
        // the piece on the round's other face, from `from` to the tangent point, kept as
        // a piece from the tangent point (its parameter 0 there)
        let piece = match self.round_face {
            CliffShape::Line { .. } => Piece::Tangent { a: qa, b: from },
            CliffShape::Circle { centre, radius } => {
                let d = norm2(centre);
                let touches = (d - radius - r1).abs() <= tol;
                if !touches {
                    return Err(failed("the cylinder does not touch the round from outside"));
                }
                let ua = [(qa[0] - centre[0]) / radius, (qa[1] - centre[1]) / radius];
                let ub = [
                    (from[0] - centre[0]) / radius,
                    (from[1] - centre[1]) / radius,
                ];
                let cos_full = dot2(ua, ub).clamp(-1.0, 1.0);
                // the half angle's cosine: the middle weight
                let w1 = ((1.0 + cos_full) * 0.5).sqrt();
                let turns = w1 > 0.0;
                if !turns {
                    return Err(failed("the crease's arc on the cylinder is half a turn"));
                }
                let mid = [(ua[0] + ub[0]) * 0.5, (ua[1] + ub[1]) * 0.5];
                let m = norm2(mid);
                let bisects = m > 0.0;
                if !bisects {
                    return Err(failed("the crease's arc on the cylinder is half a turn"));
                }
                // the middle control point: on the bisector, radius / w1 from the centre
                let p1 = add2(centre, [mid[0] / m, mid[1] / m], radius / w1);
                Piece::Arc {
                    a: qa,
                    p1,
                    b: from,
                    w1,
                    kappa: d / radius,
                }
            }
        };
        out.push(CreasePiece {
            piece,
            on_wall: false,
        });
        Ok(out)
    }
}

/// The 2D control points (homogeneous `W q` with `W`) of `piece` restricted to `[sa, sb]`
/// and raised to degree 3: four of each.
fn piece_span(piece: &Piece, sa: f64, sb: f64) -> [([f64; 2], f64); 4] {
    match *piece {
        Piece::Segment { a, b } | Piece::Tangent { a, b } => {
            let at = |s: f64| (add2(a, sub2(b, a), s), 1.0);
            let w = sb - sa;
            [at(sa), at(sa + w / 3.0), at(sa + 2.0 * w / 3.0), at(sb)]
        }
        Piece::Arc { a, p1, b, w1, .. } => {
            // homogeneous control points of the quadratic
            let h = [
                ([a[0], a[1]], 1.0),
                ([w1 * p1[0], w1 * p1[1]], w1),
                ([b[0], b[1]], 1.0),
            ];
            let blossom = |u: f64, v: f64| {
                let c0 = (1.0 - u) * (1.0 - v);
                let c1 = (1.0 - u) * v + u * (1.0 - v);
                let c2 = u * v;
                let [(p0, w0), (pm, wm), (p2, w2)] = h;
                (
                    [
                        c0 * p0[0] + c1 * pm[0] + c2 * p2[0],
                        c0 * p0[1] + c1 * pm[1] + c2 * p2[1],
                    ],
                    c0 * w0 + c1 * wm + c2 * w2,
                )
            };
            let q0 = blossom(sa, sa);
            let q1 = blossom(sa, sb);
            let q2 = blossom(sb, sb);
            let mix = |x: ([f64; 2], f64), y: ([f64; 2], f64), s: f64| {
                (
                    [
                        (1.0 - s) * x.0[0] + s * y.0[0],
                        (1.0 - s) * x.0[1] + s * y.0[1],
                    ],
                    (1.0 - s) * x.1 + s * y.1,
                )
            };
            [q0, mix(q0, q1, 2.0 / 3.0), mix(q1, q2, 1.0 / 3.0), q2]
        }
    }
}

/// The crease piece as a 3D cubic B-spline with Bézier knots over `[0, 1]`: in the frame,
/// at local `q(s)` (the profile's piece, exact: linear, or the circle's rational
/// quadratic raised to degree 3) and height `fit(s) / W(s)` along the axis. On a plane
/// the curve is polynomial; on a cylinder it carries the circle's weights.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a fit of no spans or a weight not positive; those of
/// the B-spline's construction.
pub fn crease_curve(frame: &TorusFrame, piece: &Piece, fit: &Fit) -> Result<Curve> {
    let n = fit.spans();
    if n == 0 {
        return Err(failed("a crease fit of no spans"));
    }
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for i in 0..n {
        let (sa, sb) = fit.span(i);
        let c = fit.control.get(i).copied().unwrap_or([0.0; 4]);
        let cps = piece_span(piece, sa, sb);
        let first = usize::from(i != 0);
        for (j, (wq, w)) in cps.iter().enumerate().skip(first) {
            let positive = *w > 0.0;
            if !positive {
                return Err(failed("a crease control point's weight is not positive"));
            }
            let cj = c.get(j).copied().unwrap_or(0.0);
            let q = [wq[0] / w, wq[1] / w];
            points.push(frame.at(q, cj / w));
            weights.push(*w);
        }
    }
    let mut knots = Vec::new();
    let mut mult = Vec::new();
    for i in 0..=n {
        #[allow(clippy::cast_precision_loss)]
        knots.push(i as f64 / n as f64);
        mult.push(if i == 0 || i == n { 4 } else { 3 });
    }
    let rational = weights.iter().any(|&w| w != 1.0);
    Ok(Curve::from(BSplineCurve::create_canonical(
        &CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mult,
            knots: &knots,
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: if rational { Some(&weights) } else { None },
            trim: None,
        },
    )?))
}

/// The crease piece's curve, its fit certified within `target`: on the cylinder the fit is
/// of `W h`, whose bound over the least weight (the middle weight, a convex combination's
/// least) bounds the height's error.
///
/// # Errors
///
/// Those of [`fit_certified`] and [`crease_curve`].
pub fn certified_crease(
    k: &Kernel,
    frame: &TorusFrame,
    piece: &Piece,
    target: f64,
) -> Result<(Curve, f64)> {
    let least_weight = match piece {
        Piece::Arc { w1, .. } => w1.min(1.0),
        _ => 1.0,
    };
    let (fit, bound) = fit_certified(k, piece, frame.radii, target * least_weight)?;
    let curve = crease_curve(frame, piece, &fit)?;
    Ok((curve, bound / least_weight))
}

/// The creases of the Roll-over's torus `torus` (an exact torus, or a round of the circle
/// class kept as its definition, whose minor radius is less than its major one: the convex
/// round's radius `r1` is their difference) with one cliff face `face`, each piece certified
/// within `target` of the torus: the face's profile square to the axis, from where it touches
/// the round's circle (`r1` about the axis) or from its nearest point to the axis, out to the
/// torus's rim (`rho = R`), on each side and on each side of the torus's centre plane (the
/// pieces outside the torus's face are the caller's to drop). The face must be a plane
/// parallel to the axis that touches or clears the round's circle (to `tol`) and crosses the
/// rim, or a cylinder parallel to it that touches the round's circle from outside (to `tol`).
///
/// Each curve lies on `face` (to the rounding of its control points) and within `target` of
/// the torus, the sum of three bounds:
/// - the fit's certified bound ([`certified_crease`]) against the exact crease of the face
///   moved to touch the round's circle exactly (a plane or a cylinder within `tol` of
///   touching is moved square to the axis by its distance `delta` from touching, so that the
///   piece from the tangent point is the closed form of a face that touches);
/// - `|delta|`: the curve is that crease moved back onto `face`, and the distance from the
///   torus grows by at most the move (it is 1-Lipschitz);
/// - the rounding of the frame (its directions' departure from an orthonormal frame, measured)
///   and of each control point's evaluation (`gamma_16` of the coordinates' magnitudes; the
///   curve is a positive combination of its control points, so it moves no further than they
///   do).
///
/// `None` for every other face or surface, and where the three bounds do not certify the
/// crease within `target` (a fit that does not converge, a face too far from touching): the
/// general intersection then stands.
///
/// # Errors
///
/// The kernel's cancellation.
pub fn torus_creases(
    k: &Kernel,
    torus: &Surface,
    face: &Surface,
    tol: f64,
    target: f64,
) -> Result<Option<Vec<Curve>>> {
    let (centre, axis, big_r, r2) = match torus.kind() {
        SurfaceKind::Torus(t) => {
            let place = t.base().position();
            (
                place.origin(),
                place.z_axis(),
                t.major_radius(),
                t.minor_radius(),
            )
        }
        _ => match crate::surface::procedural_blend::exact::circle_round_torus(torus)? {
            Some(found) => found,
            None => return Ok(None),
        },
    };
    let r1 = big_r - r2;
    let sized = r1 > tol
        && r2 > 0.0
        && big_r.is_finite()
        && tol > 0.0
        && tol.is_finite()
        && target > 0.0
        && target.is_finite();
    if !sized {
        return Ok(None);
    }
    let radii = Radii { r1, r2 };
    // a direction square to the axis: the axis turned onto the coordinate axis it is
    // furthest from
    let towards = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let mut out = Vec::new();
    for sign in [1.0, -1.0] {
        let Ok(frame) = TorusFrame::new(centre, axis * sign, towards, radii) else {
            return Ok(None);
        };
        let Some((pieces, shift, tilt)) = face_pieces(&frame, face, tol) else {
            return Ok(None);
        };
        let moved = norm2(shift);
        let rounding = frame_rounding(&frame, moved);
        // what is left of the target for the fit, after the move, the face's tilt and the
        // rounding
        let left = target - moved - tilt - rounding;
        let room = left > 0.0;
        if !room {
            return Ok(None);
        }
        // the crease of the face moved to touch, carried back onto the face
        let back = TorusFrame {
            centre: frame.centre + frame.dir(shift),
            ..frame
        };
        // the fit is of the piece's height alone (the frame places it): its bound holds in
        // `back` as in `frame`
        for piece in pieces {
            match certified_crease(k, &back, &piece, left) {
                Ok((curve, bound)) if bound <= left => out.push(curve),
                Err(e) if e.is_cancelled() => return Err(e),
                // a bound over what is left, or no certified fit
                Ok(_) | Err(_) => return Ok(None),
            }
        }
    }
    Ok(Some(out))
}

/// The rounding bound of a crease curve's control points in `frame` (the face moved by
/// `moved` from touching): the frame's departure from an orthonormal one (the largest of
/// its directions' squared lengths' distance from 1 and their dot products, plus their own
/// rounding), times the coordinates' magnitude three times over (one for each direction),
/// plus `gamma_16` of the point's magnitude (the centre's plus the coordinates': the
/// profile point and the height, each at most `R + r2 + moved`), doubled. `NaN` never
/// certifies (the caller's comparison fails).
fn frame_rounding(frame: &TorusFrame, moved: f64) -> f64 {
    use crate::nurbs::math::gamma;
    let (u, s, a) = (frame.up, frame.side, frame.axis);
    let defect = [
        (u.dot(u) - 1.0).abs(),
        (s.dot(s) - 1.0).abs(),
        (a.dot(a) - 1.0).abs(),
        u.dot(s).abs(),
        u.dot(a).abs(),
        s.dot(a).abs(),
    ]
    .iter()
    .fold(0.0_f64, |m, &d| if d > m || d.is_nan() { d } else { m });
    let reach = frame.radii.major() + frame.radii.r2 + moved;
    let c = frame.centre;
    let centre = c.x.abs() + c.y.abs() + c.z.abs();
    2.0 * ((defect + gamma(8)) * 3.0 * reach + gamma(16) * (centre + 3.0 * reach))
}

/// The profile pieces of `face` in `frame` ([`torus_creases`]), with the move (local) that
/// carries the face moved to touch the round's circle back onto `face` (zero where the face
/// is not moved), and the bound of the face's tilt; `None` where the face is not one of its
/// kinds.
///
/// The face is parallel to the axis only to `tol`: its profile is taken in the torus's centre
/// plane (where the face crosses it), and the crease, the profile carried along the axis, is
/// then off the tilted face by at most `|h| tan(tilt)` at height `h`, `|h| <= r2`, and on a
/// cylinder also by its section's ellipse, `radius (1 / cos(tilt) - 1)`: the tilt bound, 0 for
/// a face exactly parallel. It is charged to the target as the move is.
fn face_pieces(
    frame: &TorusFrame,
    face: &Surface,
    tol: f64,
) -> Option<(Vec<Piece>, [f64; 2], f64)> {
    let (r1, big_r) = (frame.radii.r1, frame.radii.major());
    // a plane's normal square to the axis, to `tol` (its cosine)
    let parallel = |d: Vec3| d.dot(frame.axis).abs() <= tol * d.length();
    match face.kind() {
        SurfaceKind::Plane(p) => {
            let (at, normal, _) = p.is_planar();
            if !parallel(normal) {
                return None;
            }
            let n2 = frame.local(frame.centre + normal);
            let n_len = norm2(n2);
            let flat = n_len > 0.0;
            if !flat {
                return None;
            }
            let n2 = [n2[0] / n_len, n2[1] / n_len];
            let dir = [-n2[1], n2[0]];
            // the plane where it crosses the centre plane: `at` moved along the plane's
            // steepest direction towards the axis, `a_p = axis - n c`, to height 0
            let n_hat = normal.unitize().ok()?;
            let c = n_hat.dot(frame.axis);
            let steep = 1.0 - c * c;
            let upright = steep > 0.0;
            if !upright {
                return None;
            }
            let a_p = frame.axis - n_hat * c;
            let at = at - a_p * (frame.height(at) / steep);
            let tilt = frame.radii.r2 * c.abs() / steep.sqrt();
            let q = frame.local(at);
            // the foot of the axis on the line, and its distance
            let foot = add2(q, dir, -dot2(q, dir));
            let d = norm2(foot);
            let near = d >= r1 - tol && d < big_r;
            if !near {
                return None;
            }
            if d <= r1 + tol {
                // within `tol` of tangent to the round's circle: the line moved by
                // `d - r1` along the foot's direction to touch it at `a`, from the tangent
                // point out, each side
                let positive = d > 0.0;
                if !positive {
                    return None;
                }
                let toward = [foot[0] / d, foot[1] / d];
                let a = [toward[0] * r1, toward[1] * r1];
                let reach = (big_r * big_r - r1 * r1).sqrt();
                let pieces = [add2(a, dir, reach), add2(a, dir, -reach)]
                    .iter()
                    .map(|&b| Piece::Tangent { a, b })
                    .collect();
                Some((pieces, [toward[0] * (d - r1), toward[1] * (d - r1)], tilt))
            } else {
                let reach = (big_r * big_r - d * d).sqrt();
                let ends = [add2(foot, dir, reach), add2(foot, dir, -reach)];
                Some((
                    vec![Piece::Segment {
                        a: ends[0],
                        b: ends[1],
                    }],
                    [0.0, 0.0],
                    tilt,
                ))
            }
        }
        SurfaceKind::Cone(c) if c.is_cylinder() => {
            let rev = c.rev().position();
            // the cylinder's axis along the torus's, to `tol` (its sine)
            let z = rev.z_axis();
            let along = z.cross(frame.axis).length() <= tol * z.length();
            if !along {
                return None;
            }
            let radius = c.radius(0.0);
            // the axis where it crosses the centre plane, and the tilt
            let axis_c = rev.z_axis().unitize().ok()?;
            let cos_t = axis_c.dot(frame.axis);
            let leans = cos_t.abs() > 0.0;
            if !leans {
                return None;
            }
            let origin = rev.origin() - axis_c * (frame.height(rev.origin()) / cos_t);
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let tilt = frame.radii.r2 * sin_t / cos_t.abs() + radius * (1.0 / cos_t.abs() - 1.0);
            let actual = frame.local(origin);
            let d_actual = norm2(actual);
            let touches = (d_actual - radius - r1).abs() <= tol && d_actual > 0.0 && radius > 0.0;
            if !touches {
                return None;
            }
            // the cylinder moved by `d - radius - r1` towards the axis to touch the round's
            // circle exactly
            let toward = [actual[0] / d_actual, actual[1] / d_actual];
            let d = radius + r1;
            let centre = [toward[0] * d, toward[1] * d];
            let shift = [toward[0] * (d_actual - d), toward[1] * (d_actual - d)];
            let qa = [toward[0] * r1, toward[1] * r1];
            // |c + radius u| = R: the arc from qa each way to the rim, under half a turn
            let cos_a = (big_r * big_r - d * d - radius * radius) / (2.0 * d * radius);
            if cos_a.abs() > 1.0 {
                return None;
            }
            let to_a = [-toward[0], -toward[1]];
            let perp = [-to_a[1], to_a[0]];
            let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt();
            let mut pieces = Vec::new();
            // `cos_a` is the angle's cosine from the direction away from the axis (`toward`):
            // `|c + radius u|^2 = d^2 + radius^2 + 2 d radius (toward . u) = R^2`
            for side in [1.0, -1.0] {
                let u = [
                    toward[0] * cos_a + perp[0] * sin_a * side,
                    toward[1] * cos_a + perp[1] * sin_a * side,
                ];
                let from = add2(centre, u, radius);
                let ua = to_a;
                let cos_full = dot2(ua, u).clamp(-1.0, 1.0);
                let w1 = ((1.0 + cos_full) * 0.5).sqrt();
                let mid = [(ua[0] + u[0]) * 0.5, (ua[1] + u[1]) * 0.5];
                let m = norm2(mid);
                let turns = w1 > 0.0 && m > 0.0;
                if !turns {
                    return None;
                }
                let p1 = add2(centre, [mid[0] / m, mid[1] / m], radius / w1);
                pieces.push(Piece::Arc {
                    a: qa,
                    p1,
                    b: from,
                    w1,
                    kappa: d / radius,
                });
            }
            Some((pieces, shift, tilt))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The planar model at r = 1 (generic-blend's Roll-over, the ledge's planar twin): A1
    /// at u = 1, w = -1 (local up = w + 1, side = 1 - u), the wall and the round's other
    /// face both the plane u = 0, their seam at w = 0.
    fn planar() -> (TorusFrame, Profile) {
        let frame = TorusFrame::new(
            Point3::new(1.0, -1.0, -1.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Radii { r1: 1.0, r2: 1.0 },
        )
        .unwrap();
        let profile = Profile {
            seam: frame.local(Point3::new(0.0, 0.0, 0.0)),
            wall_dir: [1.0, 0.0],
            round_face: CliffShape::Line {
                at: frame.local(Point3::new(0.0, 0.0, -1.0)),
                dir: [1.0, 0.0],
            },
        };
        (frame, profile)
    }

    #[test]
    fn the_planar_frame_puts_the_cliff_at_side_one() {
        let (frame, profile) = planar();
        // side = axis x up = y x z = x: local side of (0, *, w) is -1 ... the wall is
        // side = -1 here, the tangent point at (0, -1)
        assert!((frame.local(Point3::new(0.0, 0.0, 0.0))[1].abs() - 1.0).abs() < 1e-15);
        let qa = profile.tangent_point(1.0);
        assert!((norm2(qa) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn the_planar_crease_meets_the_wall_at_the_rail_and_fades_at_the_round() {
        let (frame, profile) = planar();
        let pieces = profile.pieces(frame.radii, 1e-12).unwrap();
        assert_eq!(pieces.len(), 2);
        let k = Kernel::default();
        for p in &pieces {
            let (curve, bound) = certified_crease(&k, &frame, &p.piece, 1e-10).unwrap();
            assert!(bound <= 1e-10);
            let ivl = curve.natural_interval();
            for i in 0..=8 {
                let x = curve
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 8.0))
                    .unwrap();
                // on the plane u = 0 to rounding
                assert!(x.x.abs() < 1e-14, "{x:?}");
                // on the torus: sin t (2 - cos p) = 1 with rho = sqrt(up^2 + side^2)
                let q = frame.local(x);
                let rho = norm2(q);
                let h = frame.height(x);
                let want = ((rho - 1.0) * (3.0 - rho)).max(0.0).sqrt();
                assert!((h - want).abs() <= bound + 1e-14, "{h} {want}");
            }
        }
        // the wall piece starts on the rail: u = 0, rho = 2, so up = sqrt 3 (w = 0.732)
        let Piece::Segment { a, .. } = pieces[0].piece else {
            panic!("the wall piece is a segment");
        };
        assert!((norm2(a) - 2.0).abs() < 1e-15);
        assert!((frame.at(a, 0.0).z - (3f64.sqrt() - 1.0)).abs() < 1e-15);
    }

    /// The planar twin's torus (centre (1, -1, -1), axis along y, radii 2 and 1) against the
    /// wall x = 0, tangent to the round's circle at (0, -1, -1): four certified pieces (each
    /// side of the tangent point, each side of the centre plane), every point on the wall and,
    /// within the bound asked plus rounding, on the torus, and on the closed form
    /// `sin t (2 - cos p) = 1`.
    /// The planar twin's torus: centre (1, -1, -1), axis along y, radii 2 and 1.
    fn planar_torus(k: &Kernel) -> Surface {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::torus::{Torus, TorusOptions};
        let frame = Axis2Placement::new(
            Point3::new(1.0, -1.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        Surface::from(
            Torus::create_canonical(k, &frame, 2.0, 1.0, TorusOptions::default(), None).unwrap(),
        )
    }

    /// The wall x = `x`, parallel to the planar twin's torus's axis, `1 - x` from it (the
    /// round's circle, radius 1, is touched at x = 0).
    fn wall_at(x: f64) -> Surface {
        use crate::geom::extent2d::Extent2d;
        use crate::geom::vector2d::Vec2;
        use crate::surface::plane::Plane;
        let domain = Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0);
        Surface::from(Plane::new(
            Point3::new(x, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec2::new(1.0, 1.0),
            &domain,
        ))
    }

    /// The distance of `p` from the planar twin's torus (exact but for rounding).
    fn off_torus(p: Point3) -> f64 {
        let (dx, dz, h) = (p.x - 1.0, p.z + 1.0, p.y + 1.0);
        let rho = (dx * dx + dz * dz).sqrt();
        (((rho - 2.0).powi(2) + h * h).sqrt() - 1.0).abs()
    }

    /// A wall within `tol` of touching the round's circle (moved `delta = 5e-10` clear of it,
    /// `tol = 1e-9`) is taken as touching, moved back by `delta`: its four pieces lie on the
    /// wall x = -delta and within the target of the real torus (the move counted in the
    /// bound). A wall moved further than the target allows (`delta = 5e-7`, within
    /// `tol = 1e-6` but past `target = 1e-8`) is not certified: `None`, the general
    /// intersection stands. Without the move counted, its pieces would lie about
    /// `sqrt(2 r2 delta) = 1e-3` off the torus at the tangent point.
    #[test]
    fn a_wall_near_touching_is_moved_and_the_move_is_in_the_bound() {
        let k = Kernel::default();
        let torus = planar_torus(&k);
        let (delta, target) = (5e-10, 1e-8);
        let curves = torus_creases(&k, &torus, &wall_at(-delta), 1e-9, target)
            .unwrap()
            .unwrap();
        assert_eq!(curves.len(), 4);
        for c in &curves {
            let ivl = c.natural_interval();
            for i in 0..=64 {
                let p = c
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap();
                assert!((p.x + delta).abs() < 1e-14, "{p:?}");
                assert!(off_torus(p) <= target, "{p:?} {}", off_torus(p));
            }
        }
        // the moved wall's height error at the tangent point, which the bound must carry
        assert!((2.0 * 5e-7_f64).sqrt() > 1e-8);
        let far = torus_creases(&k, &torus, &wall_at(-5e-7), 1e-6, target).unwrap();
        assert!(far.is_none());
    }

    /// The wall `x = -0.5 + eps y` (through `(-0.5 + eps y0, y0, 0)`), tilted `eps` from
    /// parallel to the torus's axis (y), clear of the round: a plain segment's crease.
    fn tilted_wall(eps: f64, y0: f64) -> Surface {
        use crate::geom::extent2d::Extent2d;
        use crate::geom::vector2d::Vec2;
        use crate::surface::plane::Plane;
        let domain = Extent2d::from_bounds(-1e5, -1e5, 1e5, 1e5);
        let along = Vec3::new(eps, 1.0, 0.0).unitize().unwrap();
        Surface::from(Plane::new(
            Point3::new(-0.5 + eps * y0, y0, 0.0),
            along,
            Vec3::new(0.0, 0.0, 1.0),
            Vec2::new(1.0, 1.0),
            &domain,
        ))
    }

    /// A wall tilted within `tol` of parallel to the axis, whose own point lies far along
    /// it, is profiled where it crosses the torus's centre plane, and its tilt is charged:
    /// tilted 1e-11 with its point at y = 1e4 (where the wall is 1e-7 off its crossing),
    /// its crease lies within the target of the torus and on the real wall; tilted 1e-7 with
    /// its point at y = 30 (the tilt alone 1e-7 off over the crease's heights, past the
    /// target 1e-8) it is not certified, `None`. Profiled at its own point instead, its
    /// crease would lie 1e-7 and 3e-6 off the wall.
    #[test]
    fn a_tilted_wall_is_profiled_in_the_centre_plane_and_its_tilt_charged() {
        let k = Kernel::default();
        let torus = planar_torus(&k);
        let (eps, target) = (1e-11, 1e-8);
        let curves = torus_creases(&k, &torus, &tilted_wall(eps, 1e4), 1e-6, target)
            .unwrap()
            .unwrap();
        assert_eq!(curves.len(), 2);
        for c in &curves {
            let ivl = c.natural_interval();
            for i in 0..=64 {
                let p = c
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap();
                assert!((p.x - (-0.5 + eps * p.y)).abs() < 1e-10, "{p:?}");
                assert!(off_torus(p) <= target, "{p:?} {}", off_torus(p));
            }
        }
        let steep = torus_creases(&k, &torus, &tilted_wall(1e-7, 30.0), 1e-6, target).unwrap();
        assert!(steep.is_none());
    }

    /// A cylinder of radius 2 along the axis (y), its axis at x = -2, z = -1 (3 from the
    /// round's axis: touching the round's circle from outside at (0, -1)), placed at y = 50:
    /// its two arcs from the tangent point to the rim, on each side of the centre plane,
    /// lie on the cylinder (to rounding) and within the target of the torus.
    #[test]
    fn a_cylinder_touching_the_round_has_certified_arc_creases() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::cone::Cone;
        let k = Kernel::default();
        let torus = planar_torus(&k);
        let frame = Axis2Placement::new(
            Point3::new(-2.0, 50.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        let wall = Surface::from(Cone::create_canonical_cylinder(&k, &frame, 2.0).unwrap());
        let target = 1e-8;
        let curves = torus_creases(&k, &torus, &wall, 1e-9, target)
            .unwrap()
            .unwrap();
        assert_eq!(curves.len(), 4);
        for c in &curves {
            let ivl = c.natural_interval();
            for i in 0..=64 {
                let p = c
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap();
                let (dx, dz) = (p.x + 2.0, p.z + 1.0);
                assert!(((dx * dx + dz * dz).sqrt() - 2.0).abs() < 1e-12, "{p:?}");
                assert!(off_torus(p) <= target, "{p:?} {}", off_torus(p));
            }
        }
    }

    /// A wall just clear of the round's circle, past the touching tolerance (x = -2 tol, so
    /// `rho = r1 + 2 tol` at its nearest point), is a plain segment whose crease's height
    /// `sqrt(rho^2 - r1^2) ...` is steep (its second derivative about `tol^-1/2`) at its
    /// middle: the fit is not certified within the halvings, and that is `None` (the general
    /// intersection stands), not an error.
    #[test]
    fn a_wall_just_clear_of_the_round_is_left_to_the_general_intersection() {
        let k = Kernel::default();
        let torus = planar_torus(&k);
        let tol = 1e-9;
        let got = torus_creases(&k, &torus, &wall_at(-2.0 * tol), tol, 1e-10).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn torus_creases_on_the_planar_wall_are_certified_and_closed_form() {
        let k = Kernel::default();
        let torus = planar_torus(&k);
        let wall = wall_at(0.0);
        let target = 1e-10;
        let curves = torus_creases(&k, &torus, &wall, 1e-9, target)
            .unwrap()
            .unwrap();
        assert_eq!(curves.len(), 4);
        for c in &curves {
            let ivl = c.natural_interval();
            for i in 0..=16 {
                let p = c
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 16.0))
                    .unwrap();
                assert!(p.x.abs() < 1e-12, "{p:?}");
                // about the axis: rho, and the height off the centre plane y = -1
                let (dx, dz, h) = (p.x - 1.0, p.z + 1.0, p.y + 1.0);
                let rho = (dx * dx + dz * dz).sqrt();
                let off = ((rho - 2.0).powi(2) + h * h).sqrt() - 1.0;
                assert!(off.abs() <= target + 1e-12, "{p:?} {off}");
                // the crease's closed form: sin t = (1 - x) / rho = 1 / rho at x = 0, and
                // cos p = 2 - rho
                let sin_t = (1.0 - p.x) / rho;
                assert!((sin_t * (2.0 - (2.0 - rho)) - 1.0).abs() < 1e-12);
            }
        }
    }
}
