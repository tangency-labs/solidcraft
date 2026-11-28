//! The NURBS forms of the record's analytic curves and surfaces: the exact B-spline each one is
//! (docs/native-brep.md, "Surfaces"), built the way SMLib holds its own, so that a circle and every
//! face it bounds share one parameterisation and their pcurves are exact by construction.
//!
//! Angles are on rational quadratic quarter arcs laid on their frame's quarter grid: quarter `q`
//! runs from `q·π/2` to `(q+1)·π/2` from the frame's x axis, and the form parameter of an angle
//! `θ` in it is `q + u`, `u` the arc's own parameter (closed form, [`quarter_u`]). Every form on one
//! frame therefore has the same parameter at the same angle, whatever quarters it spans: a circle
//! edge's form and its cylinder's form agree along it, so its pcurve is a straight line in (u, v).
//! A form spans the quarters its interval needs (at most five for one turn), so an arc across the
//! frame's x axis has a form like any other. A curve's form of five is cut to its interval's ends
//! ([`cut_to`]): whole, it would run a quarter past one turn. A face of five would cover part of
//! its surface twice: its frame is turned about its axis first ([`turned`], [`turn_quarters`]),
//! so that it starts on a quarter line and its form spans four; a torus face's tube likewise
//! starts from its tube start ([`tube_turned`], [`tube_quarters`]).
//!
//! A circle or ellipse is the arcs (an ellipse scaled by its radii, which keeps it exact: its
//! parameter is the eccentric angle). A cylinder or cone is the arcs times a degree-1 line along the
//! axis; a sphere the arcs times arcs of latitude (from `−π/2` to `π/2`); a torus the arcs times
//! the arcs of its tube. A line is the degree-1 B-spline on its own parameter.
//!
//! A face that winds round its axis more than a turn (a thread's flank) has a form of as many
//! quarters as it winds, up to 64 turns ([`wound_quarters`]): it does not close on itself, and a
//! turn more is four more form parameters.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests
#![allow(clippy::type_complexity)] // knots with weighted arc points

use super::geometry::{Curve, Frame, Surface};
use super::vec::VecExt;
use super::{CurveEval, EvalError, SurfaceEval};
use crate::math::det;
use crate::math::Vec3;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_8, TAU};

/// Rounding room on an angle at a quarter's end.
const SLACK: f64 = 1e-12;

/// Rounding room on a turn, as a fraction of it: the one [`extent`] and [`face_arcs`] already
/// use to tell a whole turn from less.
const TURN_ROUNDING: f64 = 1e-9;

/// Whether a face's box ([`extent`]) goes round its axis more than a turn. [`extent`] clamps a
/// box to one turn unless its angle runs past a turn by more than rounding and the angle the
/// edges' distance off the surface makes, so a box wider than a turn and rounding is a face that
/// winds ([`wound_surface`]).
pub fn winds([u, _]: [[f64; 2]; 2]) -> bool {
    u[1] - u[0] > TAU * (1.0 + TURN_ROUNDING)
}

/// The quarter arc's parameter `u` in `[0, 1]` at angle `ψ` in `[0, π/2]` from its start: the
/// arc with ends `±π/4` from its bisector and middle weight `cos π/4` has
/// `tan((ψ − π/4)/2) = tan(π/8)(2u − 1)`.
fn quarter_u(psi: f64) -> f64 {
    0.5 * (1.0 + det::tan(0.5 * psi - FRAC_PI_8) / det::tan(FRAC_PI_8))
}

fn quarter_psi(u: f64) -> f64 {
    2.0 * (det::atan((2.0 * u - 1.0) * det::tan(FRAC_PI_8)) + FRAC_PI_8)
}

/// The quarter an angle lies in (an angle on a quarter's end lies in the one after it).
fn quarter_of(theta: f64) -> f64 {
    (theta / FRAC_PI_2 + SLACK).floor()
}

/// The form parameter of angle `theta`: its quarter plus the arc's parameter there.
pub fn param(theta: f64) -> f64 {
    let q = quarter_of(theta);
    q + quarter_u((theta - q * FRAC_PI_2).clamp(0.0, FRAC_PI_2))
}

/// The angle at form parameter `s`.
pub fn angle(s: f64) -> f64 {
    let q = s.floor();
    q * FRAC_PI_2 + quarter_psi(s - q)
}

/// The forms' parameter intervals are finite and ordered.
fn valid_interval([a0, a1]: [f64; 2]) -> bool {
    a0.is_finite() && a1.is_finite() && a1 > a0
}

/// The analytic forms cover a finite, positive interval of at most one turn.
fn valid_angles([a0, a1]: [f64; 2]) -> bool {
    valid_interval([a0, a1]) && a1 - a0 <= TAU * (1.0 + 1e-9)
}

/// The quarters an angle interval needs: the first one and how many. An end within `reach` (an
/// angle) of a quarter's end is on it: a face whose boundary lies within its tolerance of a
/// quarter's end needs no part of the quarter past it.
fn quarters([a0, a1]: [f64; 2], reach: f64) -> Option<(f64, usize)> {
    if !valid_angles([a0, a1]) {
        return None;
    }
    let (a0, a1) = (on_quarter_line(a0, reach), on_quarter_line(a1, reach));
    let q0 = quarter_of(a0);
    let q1 = (a1 / FRAC_PI_2 - SLACK).ceil();
    let n = (q1 - q0).max(1.0) as usize;
    (n <= 5).then_some((q0, n))
}

/// An angle within `reach` of a quarter's end, on it; another as it is.
fn on_quarter_line(a: f64, reach: f64) -> f64 {
    let end = (a / FRAC_PI_2).round() * FRAC_PI_2;
    if (a - end).abs() <= reach {
        end
    } else {
        a
    }
}

/// A face's quarters along an angle direction and its domain there: a whole turn is exactly
/// four quarters from the one it starts in (the form closes on itself, so it is periodic and the
/// face's seam uses lie a period apart), a start within `reach` of a quarter's end starting on
/// it (a seam a rounding short of the quarter line its frame puts it on would otherwise start
/// the form a quarter early, and the seam's second use would lie a quarter past its end);
/// anything less the quarters it needs ([`quarters`], ends within `reach` of a quarter's end on
/// it).
fn face_quarters(a: [f64; 2], reach: f64) -> Option<((f64, usize), [f64; 2])> {
    if !valid_angles(a) {
        return None;
    }
    if a[1] - a[0] >= TAU * (1.0 - 1e-9) {
        let q0 = quarter_of(on_quarter_line(a[0], reach));
        return Some(((q0, 4), [q0, q0 + 4.0]));
    }
    let grid = quarters(a, reach)?;
    Some((grid, params(a, grid)))
}

/// The most quarters a face that winds round its axis may take (64 turns).
const MAX_WOUND_QUARTERS: usize = 256;

/// The quarters of an angle interval of more than a turn (a face that winds round its axis, a
/// thread's flank), and its domain on them. The form is not closed on itself: its quarters run
/// on from the one the face starts in, so a whole turn more is exactly four more form
/// parameters, and a pcurve moved a turn is moved four.
fn wound_quarters([a0, a1]: [f64; 2], reach: f64) -> Option<((f64, usize), [f64; 2])> {
    if !valid_interval([a0, a1]) {
        return None;
    }
    let (a0, a1) = (on_quarter_line(a0, reach), on_quarter_line(a1, reach));
    let q0 = quarter_of(a0);
    let n = ((a1 / FRAC_PI_2 - SLACK).ceil() - q0).max(1.0);
    if !(n <= MAX_WOUND_QUARTERS as f64) {
        return None;
    }
    let grid = (q0, n as usize);
    Some((grid, params([a0, a1], grid)))
}

/// The form parameters of an angle interval on the quarters `quarters` chose for it (its ends kept
/// on them: an end within rounding past a quarter's end stays in that quarter).
fn params([a0, a1]: [f64; 2], (q0, n): (f64, usize)) -> [f64; 2] {
    let end = q0 + n as f64;
    [param(a0).clamp(q0, end), param(a1).clamp(q0, end)]
}

/// Equal arcs from angle `a0` to `a1` (less than a turn), as few as keep each within a quarter:
/// their knot vector (0 at `a0`, one more at each arc's end) and control points (as
/// [`arcs`]). A form on them is not on the quarter grid: for a face whose angle range needs the
/// four quarters of a turn (or five) without being a turn, which on the grid would be a form that
/// closes on itself (or overlaps itself) around a face that does not.
fn arcs_between(a0: f64, a1: f64) -> Option<(Vec<f64>, Vec<([f64; 2], f64)>)> {
    let span = a1 - a0;
    if !(a0.is_finite() && span > 0.0 && span < TAU) {
        return None;
    }
    let n = ((span / FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let phi = span / n as f64;
    let w = det::cos(0.5 * phi);
    let at = |a: f64| [det::cos(a), det::sin(a)];
    let mut pts = Vec::new();
    let mut knots = vec![0.0; 3];
    for k in 0..n {
        let start = a0 + phi * k as f64;
        if k == 0 {
            pts.push((at(start), 1.0));
        }
        let mid = at(start + 0.5 * phi);
        pts.push(([mid[0] / w, mid[1] / w], w));
        pts.push((at(start + phi), 1.0));
        let end = (k + 1) as f64;
        knots.extend(if k + 1 < n {
            vec![end; 2]
        } else {
            vec![end; 3]
        });
    }
    Some((knots, pts))
}

/// The arcs of a face's angle range `a` and its domain on them: the quarter grid's
/// ([`face_quarters`]), or with `own` and a range short of a turn, arcs of its own from its start
/// ([`arcs_between`], the domain all of them).
fn face_arcs(
    a: [f64; 2],
    reach: f64,
    own: bool,
    winds: bool,
) -> Option<((Vec<f64>, Vec<([f64; 2], f64)>), [f64; 2])> {
    // Only [`wound_surface`] lets an angle wind; every other form refuses more than a turn.
    if winds && a[1] - a[0] > TAU * (1.0 + TURN_ROUNDING) {
        let (grid, d) = wound_quarters(a, reach)?;
        return Some((arcs(grid.0, grid.1), d));
    }
    if !valid_angles(a) {
        return None;
    }
    if own && a[1] - a[0] < TAU * (1.0 - 1e-9) {
        // Past each end by `reach` (while that stays short of a turn): the face's boundary, which
        // lies off the surface by as much, then stays on the form.
        let pad = reach
            .max(0.0)
            .min(0.25 * (TAU * (1.0 - 1e-9) - (a[1] - a[0])));
        let (a0, a1) = (a[0] - pad, a[1] + pad);
        let made = arcs_between(a0, a1)?;
        let n = made.0.last().copied()?;
        let phi = (a1 - a0) / n;
        // The form parameter at angle `t`: its arc, plus the arc's own parameter there (as
        // `quarter_u`, for an arc of `phi`).
        let at = |t: f64| {
            let k = ((t - a0) / phi).floor().clamp(0.0, n - 1.0);
            let psi = (t - a0 - k * phi).clamp(0.0, phi);
            k + 0.5 * (1.0 + det::tan(0.5 * psi - 0.25 * phi) / det::tan(0.25 * phi))
        };
        return Some((made, [at(a[0]), at(a[1])]));
    }
    let (grid, d) = face_quarters(a, reach)?;
    Some((arcs(grid.0, grid.1), d))
}

/// Quarter arcs from quarter `q0`, `n` of them: their knot vector (in form parameter) and their
/// control points (as `(cos, sin)` factors on the unit circle's plane) with weights.
fn arcs(q0: f64, n: usize) -> (Vec<f64>, Vec<([f64; 2], f64)>) {
    let at = |a: f64| [det::cos(a), det::sin(a)];
    let mut pts = Vec::new();
    let mut knots = vec![q0; 3];
    for k in 0..n {
        let q = q0 + k as f64;
        let (p, r) = (at(q * FRAC_PI_2), at((q + 1.0) * FRAC_PI_2));
        if k == 0 {
            pts.push((p, 1.0));
        }
        pts.push(([p[0] + r[0], p[1] + r[1]], FRAC_1_SQRT_2));
        pts.push((r, 1.0));
        knots.extend(if k + 1 < n {
            vec![q + 1.0; 2]
        } else {
            vec![q + 1.0; 3]
        });
    }
    (knots, pts)
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn hom(p: Vec3, w: f64) -> [f64; 4] {
    [p.x * w, p.y * w, p.z * w, w]
}

/// The frame's origin and unit axes (z = x × y).
fn axes(f: &Frame) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let (x, y) = (v3(f.x_axis).normalized()?, v3(f.y_axis).normalized()?);
    Some((v3(f.origin), x, y, x.cross(y).normalized()?))
}

/// The NURBS form of a line, circle or ellipse edge and its interval in the form's parameter;
/// `None` for another kind, or an arc of more than one turn. An arc's form is the quarters of its
/// frame's grid that it needs; where those are five (it starts inside a quarter and ends past
/// the fourth quarter line on from it), the first and the last are cut to the arc's ends, so
/// the form does not run past one turn.
pub fn curve(c: &Curve, [t0, t1]: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    match c {
        Curve::Line { start, end } => {
            let (a, d) = (v3(*start), v3(*end) - v3(*start));
            let (p0, p1) = (a + d * t0, a + d * t1);
            Some((
                Curve::Spline {
                    knots: vec![t0, t0, t1, t1],
                    points: vec![[p0.x, p0.y, p0.z], [p1.x, p1.y, p1.z]],
                },
                [t0, t1],
            ))
        }
        Curve::Circle { frame, radius } => conic(frame, [*radius; 2], [t0, t1]),
        Curve::Ellipse { frame, radii } => conic(frame, *radii, [t0, t1]),
        _ => None,
    }
}

fn conic(frame: &Frame, [rx, ry]: [f64; 2], t: [f64; 2]) -> Option<(Curve, [f64; 2])> {
    let (o, x, y, _) = axes(frame)?;
    let grid = quarters(t, 0.0)?;
    let on = params(t, grid);
    let whole = arcs(grid.0, grid.1);
    let (knots, pts) = match grid.1 {
        5 => cut_to(&whole, on).unwrap_or(whole),
        _ => whole,
    };
    let homogeneous = pts
        .iter()
        .map(|([c, s], w)| hom(o + x * (rx * c) + y * (ry * s), *w))
        .collect();
    Some((Curve::RationalSpline { knots, homogeneous }, on))
}

/// Quarter arcs ([`arcs`]) with the first and the last cut to `[s0, s1]`, form parameters inside
/// those two arcs: the same curve at the same parameters, from `s0` to `s1` and no further. Five
/// whole quarters run a quarter past one turn, and the kernel takes a conic's form for at most
/// one (`curve::from_parameters::conic_on_form`); cut to an arc of at most one turn they are
/// that arc. Each cut is the arc's blossom at its new ends, in homogeneous coordinates. `None`
/// where an end is not inside its arc.
fn cut_to(
    (knots, pts): &(Vec<f64>, Vec<([f64; 2], f64)>),
    [s0, s1]: [f64; 2],
) -> Option<(Vec<f64>, Vec<([f64; 2], f64)>)> {
    let (mut knots, mut pts) = (knots.clone(), pts.clone());
    let (k, n) = (knots.len(), pts.len());
    // Each new end as its own arc's parameter in [0, 1].
    let (a, b) = (s0 - knots.first()?, s1 - (knots.last()? - 1.0));
    if !(n >= 5 && (0.0..1.0).contains(&a) && b > 0.0 && b <= 1.0) {
        return None;
    }
    let blossom = |arc: &[([f64; 2], f64)], u: f64, v: f64| -> Option<([f64; 2], f64)> {
        let [([x0, y0], w0), ([x1, y1], w1), ([x2, y2], w2)] = arc else {
            return None;
        };
        let (c0, c1, c2) = (
            (1.0 - u) * (1.0 - v) * w0,
            ((1.0 - u) * v + u * (1.0 - v)) * w1,
            u * v * w2,
        );
        let w = c0 + c1 + c2;
        (w > 0.0).then(|| {
            (
                [
                    (c0 * x0 + c1 * x1 + c2 * x2) / w,
                    (c0 * y0 + c1 * y1 + c2 * y2) / w,
                ],
                w,
            )
        })
    };
    let (start, end) = (pts.get(..3)?, pts.get(n - 3..)?);
    let head = [blossom(start, a, a)?, blossom(start, a, 1.0)?];
    let tail = [blossom(end, 0.0, b)?, blossom(end, b, b)?];
    pts.splice(..2, head);
    pts.splice(n - 2.., tail);
    knots.get_mut(..3)?.fill(s0);
    knots.get_mut(k - 3..)?.fill(s1);
    Some((knots, pts))
}

/// The NURBS form of a cylinder, cone, sphere or torus face over `domain` (in the label's
/// parameters: angles, and the height along the axis of a cylinder or cone) and the domain in the
/// form's parameters; `None` for another kind, or a domain of more than one turn.
pub fn surface(s: &Surface, domain: [[f64; 2]; 2]) -> Option<(Surface, [[f64; 2]; 2])> {
    surface_within(s, domain, 0.0)
}

/// The angles `tolerance` (mm) makes along a face's two directions ([`surface_within`]): on the
/// widest circle of the face about the axis (over `v`, the height of a cylinder or cone), and on
/// a sphere's meridian or a torus's tube.
fn reaches(s: &Surface, v: [f64; 2], tolerance: f64) -> (f64, f64) {
    // The angle `tolerance` makes on a circle of `radius`.
    let reach = |radius: f64| {
        if tolerance > 0.0 && radius > 0.0 && radius.is_finite() {
            tolerance / radius
        } else {
            0.0
        }
    };
    match s {
        Surface::Cylinder { radius, .. } => (reach(radius.abs()), 0.0),
        Surface::Cone {
            radius_at_origin,
            half_angle,
            ..
        } => {
            let slope = det::tan(*half_angle);
            (
                reach(
                    v.iter()
                        .map(|h| (radius_at_origin + h * slope).abs())
                        .fold(0.0, f64::max),
                ),
                0.0,
            )
        }
        Surface::Sphere { radius, .. } => (reach(radius.abs()), reach(radius.abs())),
        Surface::Torus {
            major_radius,
            minor_radius,
            ..
        } => (
            reach(major_radius.abs() + minor_radius.abs()),
            reach(minor_radius.abs()),
        ),
        _ => (0.0, 0.0),
    }
}

/// How many quarters a cylinder, cone, sphere or torus face over `[u, v]` (its box on the label,
/// as for [`surface_within`]) spans about the axis on its frame's quarter grid: five when the face
/// does not start on a quarter line and more than three quarters are left to it. A face of less
/// than one turn then has a form past one turn (a NURBS that covers part of its surface twice); a
/// whole turn has a form of four quarters from the one it starts in, which ends short of the
/// face's seam, so the seam's second use lies off the form. [`turned`] puts its start on a quarter
/// line. A whole turn's start is on a quarter line only as its seam circle's is ([`conic`], no
/// tolerance reach): its seam is a whole-turn circle on the same grid, whose form needs a fifth
/// quarter whenever it starts off the line, however close. `None` for another surface.
pub fn turn_quarters(s: &Surface, [u, v]: [[f64; 2]; 2], tolerance: f64) -> Option<usize> {
    if !matches!(
        s,
        Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
    ) {
        return None;
    }
    let reach = if u[1] - u[0] >= TAU * (1.0 - TURN_ROUNDING) {
        0.0
    } else {
        reaches(s, v, tolerance).0
    };
    Some(quarters(u, reach)?.1)
}

/// A cylinder, cone, sphere or torus with its frame turned by `angle` about its own axis (its x
/// and y axes turned right-handed about z): the same surface, its angle parameter `angle` less at
/// every point. `None` for another surface.
pub fn turned(s: &Surface, angle: f64) -> Option<Surface> {
    let turn = |f: &Frame| -> Option<Frame> {
        let (o, x, _, z) = axes(f)?;
        let (sin, cos) = det::sin_cos(angle);
        let nx = x * cos + z.cross(x) * sin;
        let ny = z.cross(nx);
        Some(Frame {
            origin: [o.x, o.y, o.z],
            x_axis: [nx.x, nx.y, nx.z],
            y_axis: [ny.x, ny.y, ny.z],
        })
    };
    Some(match s {
        Surface::Cylinder { frame, radius } => Surface::Cylinder {
            frame: turn(frame)?,
            radius: *radius,
        },
        Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => Surface::Cone {
            frame: turn(frame)?,
            radius_at_origin: *radius_at_origin,
            half_angle: *half_angle,
        },
        Surface::Sphere { frame, radius } => Surface::Sphere {
            frame: turn(frame)?,
            radius: *radius,
        },
        Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        } => Surface::Torus {
            frame: turn(frame)?,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
            tube_start: *tube_start,
        },
        _ => return None,
    })
}

/// How many quarters a torus face over `[u, v]` (its box on the label, as for
/// [`surface_within`]) spans along its tube on the label's quarter grid ([`turn_quarters`] for
/// the tube angle): five when the face does not start on a quarter line of the tube and more
/// than three quarters are left to it, a whole turn round the tube included (its form of four
/// quarters from the one it starts in ends short of its seam's second use). A whole turn is
/// counted from its start, within the rounding [`face_quarters`] takes for one. [`tube_turned`]
/// puts its start on one. `None` for another surface.
pub fn tube_quarters(s: &Surface, [_, v]: [[f64; 2]; 2], tolerance: f64) -> Option<usize> {
    if !matches!(s, Surface::Torus { .. }) {
        return None;
    }
    let reach = reaches(s, v, tolerance).1;
    let whole = valid_angles(v) && v[1] - v[0] >= TAU * (1.0 - TURN_ROUNDING);
    let v = if whole { [v[0], v[0] + TAU] } else { v };
    Some(quarters(v, reach)?.1)
}

/// A torus with its tube start moved on by `angle`: the same surface, its tube angle parameter
/// `angle` less at every point. `None` for another surface.
pub fn tube_turned(s: &Surface, angle: f64) -> Option<Surface> {
    let Surface::Torus {
        frame,
        major_radius,
        minor_radius,
        tube_start,
    } = s
    else {
        return None;
    };
    // Kept within one turn either way, as a record's tube start is.
    let start = tube_start + angle;
    let start = if start.abs() > TAU {
        start % TAU
    } else {
        start
    };
    Some(Surface::Torus {
        frame: frame.clone(),
        major_radius: *major_radius,
        minor_radius: *minor_radius,
        tube_start: start,
    })
}

/// [`surface`] for a face whose boundary lies within `tolerance` (mm) of its surface: where the
/// domain ends within `tolerance` of a quarter's end (the angle that far along the widest circle
/// of the face in that direction), the form ends there too. A face's box is the extent of its
/// boundary's points ([`extent`]), which may run past a quarter's end by as much as its edges lie
/// off the surface; the form then covers no quarter for that alone (a torus face of half a tube
/// would otherwise go round the whole tube).
pub fn surface_within(
    s: &Surface,
    domain: [[f64; 2]; 2],
    tolerance: f64,
) -> Option<(Surface, [[f64; 2]; 2])> {
    surface_on(s, domain, tolerance, false, false)
}

/// The NURBS form of a cylinder or cone face over `domain` (as [`surface_within`]) that winds
/// round its axis more than a turn (a thread's flank, up to 64 turns): a form that does not
/// close on itself, as many quarters as the face winds. `None` for any other surface, and for a
/// domain of a turn or less ([`winds`]: use [`surface_within`]). Only the STEP reader's winding
/// path takes this: [`surface`], [`surface_within`] and [`surface_on_own_arcs`] refuse more than
/// a turn for every form.
pub fn wound_surface(
    s: &Surface,
    domain: [[f64; 2]; 2],
    tolerance: f64,
) -> Option<(Surface, [[f64; 2]; 2])> {
    match s {
        Surface::Cylinder { .. } | Surface::Cone { .. } if winds(domain) => {
            surface_on(s, domain, tolerance, false, true)
        }
        _ => None,
    }
}

/// The NURBS form of a cylinder, cone, sphere or torus face over `domain` (as [`surface`]) whose
/// angles run on arcs of the face's own from where it starts, not on its frame's quarter grid,
/// wherever its range is short of a turn: the same surface, exactly, but a form that never
/// closes on itself around a face that does not (a torus face round 250° of its tube needs all
/// four quarters of the grid, a form SMLib does not take for a torus); each angle runs past the
/// face's ends by the angle `tolerance` (mm) makes, so a boundary that far off stays on the
/// form. Its circles' forms then no
/// longer share its parameter, so its pcurves are not lines: Repair Part rebuilds a refused face
/// on it and has SMLib make them.
pub fn surface_on_own_arcs(
    s: &Surface,
    domain: [[f64; 2]; 2],
    tolerance: f64,
) -> Option<(Surface, [[f64; 2]; 2])> {
    match s {
        Surface::Cylinder { .. }
        | Surface::Cone { .. }
        | Surface::Sphere { .. }
        | Surface::Torus { .. } => surface_on(s, domain, tolerance, true, false),
        _ => None,
    }
}

fn surface_on(
    s: &Surface,
    [u, v]: [[f64; 2]; 2],
    tolerance: f64,
    own: bool,
    wound: bool,
) -> Option<(Surface, [[f64; 2]; 2])> {
    if let Surface::PlacedRevolution {
        profile,
        origin,
        axis,
        matrix,
        forward,
    } = s
    {
        // The profile's control points turned about the axis on the quarter grid from angle
        // zero (the profile): (a + ib)(c + is) for each point's radial (a, b) in the frame and
        // each arc control point (c, s), weights multiplied; then the matrix.
        let (angles, along) = if *forward { (v, u) } else { (u, v) };
        if !valid_interval(along) {
            return None;
        }
        let (p_knots, h) = profile_nurbs(profile, Some(along))?;
        let fp = match profile.as_ref() {
            Curve::Circle { .. } | Curve::Ellipse { .. } => curve(profile, along)?.1,
            _ => along,
        };
        let (o, x, y, k) = revolution_frame(&h, *origin, *axis)?;
        // The angle `tolerance` makes on the widest circle the face's profile reaches about the
        // axis, as a cylinder's ([`reaches`]): no wider than its control points' (the profile
        // lies in their hull). A matrix that is not the identity may scale the placed surface,
        // so its angle ends are taken as they are.
        let identity = *matrix
            == [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
        let widest = h
            .iter()
            .map(|hp| {
                let d = Vec3::new(hp[0] / hp[3], hp[1] / hp[3], hp[2] / hp[3]) - o;
                (d - k * d.dot(k)).len()
            })
            .fold(0.0, f64::max);
        let reach = if identity && tolerance > 0.0 && widest > 0.0 && widest.is_finite() {
            tolerance / widest
        } else {
            0.0
        };
        let (ring_grid, fa) = face_quarters(angles, reach)?;
        let (a_knots, arcs_r) = arcs(ring_grid.0, ring_grid.1);
        let net: Vec<Vec<[f64; 4]>> = arcs_r
            .iter()
            .map(|([c, s], wu)| {
                h.iter()
                    .map(|hp| {
                        let d = Vec3::new(hp[0] / hp[3], hp[1] / hp[3], hp[2] / hp[3]) - o;
                        let (a, b, z) = (d.dot(x), d.dot(y), d.dot(k));
                        let q = o + k * z + x * (a * c - b * s) + y * (a * s + b * c);
                        hom(affine(matrix, q), wu * hp[3])
                    })
                    .collect()
            })
            .collect();
        return if *forward {
            // u the profile, v the angle: the net turned over.
            let n = net.first()?.len();
            let t: Vec<Vec<[f64; 4]>> = (0..n)
                .map(|j| net.iter().filter_map(|r| r.get(j).copied()).collect())
                .collect();
            Some((
                Surface::RationalSpline {
                    u_knots: p_knots,
                    v_knots: a_knots,
                    homogeneous: t,
                },
                [fp, fa],
            ))
        } else {
            Some((
                Surface::RationalSpline {
                    u_knots: a_knots,
                    v_knots: p_knots,
                    homogeneous: net,
                },
                [fa, fp],
            ))
        };
    }
    let (reach_u, reach_v) = reaches(s, v, tolerance);
    let ((u_knots, ring), fu) = face_arcs(u, reach_u, own, wound)?;
    // The ring of arcs times a profile `(control, weight)` mapped to (radial, axial) about the
    // frame: a rational tensor product, its weights the products.
    let net = |f: &Frame,
               profile: &[([f64; 2], f64)],
               at: &dyn Fn([f64; 2]) -> (f64, f64)|
     -> Option<Vec<Vec<[f64; 4]>>> {
        let (o, x, y, z) = axes(f)?;
        Some(
            ring.iter()
                .map(|([c, s], wu)| {
                    let r = x * *c + y * *s;
                    profile
                        .iter()
                        .map(|(q, wv)| {
                            let (radial, axial) = at(*q);
                            hom(o + r * radial + z * axial, wu * wv)
                        })
                        .collect()
                })
                .collect(),
        )
    };
    let line = [([v[0], 0.0], 1.0), ([v[1], 0.0], 1.0)];
    match s {
        Surface::Cylinder { frame, radius } => {
            let homogeneous = net(frame, &line, &|[h, _]| (*radius, h))?;
            Some((
                Surface::RationalSpline {
                    u_knots,
                    v_knots: vec![v[0], v[0], v[1], v[1]],
                    homogeneous,
                },
                [fu, v],
            ))
        }
        Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => {
            let slope = det::tan(*half_angle);
            let homogeneous = net(frame, &line, &|[h, _]| (radius_at_origin + h * slope, h))?;
            Some((
                Surface::RationalSpline {
                    u_knots,
                    v_knots: vec![v[0], v[0], v[1], v[1]],
                    homogeneous,
                },
                [fu, v],
            ))
        }
        Surface::Sphere { frame, radius } => {
            let vgrid = quarters(v, reach_v)?;
            let (v_knots, meridian) = arcs(vgrid.0, vgrid.1);
            let homogeneous = net(frame, &meridian, &|[c, s]| (radius * c, radius * s))?;
            Some((
                Surface::RationalSpline {
                    u_knots,
                    v_knots,
                    homogeneous,
                },
                [fu, params(v, vgrid)],
            ))
        }
        Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        } => {
            let ((v_knots, tube), fv) = face_arcs(v, reach_v, own, false)?;
            // The tube's arcs turned on by its start (a rotation of the control points, exact).
            let (ts, tc) = det::sin_cos(*tube_start);
            let homogeneous = net(frame, &tube, &|[c, s]| {
                (
                    major_radius + minor_radius * (c * tc - s * ts),
                    minor_radius * (s * tc + c * ts),
                )
            })?;
            Some((
                Surface::RationalSpline {
                    u_knots,
                    v_knots,
                    homogeneous,
                },
                [fu, fv],
            ))
        }
        _ => None,
    }
}

/// A circle edge labelled on another frame about the same axis: the same circle, its angle
/// measured from `frame`'s x axis (its y axis taken so the circle keeps its own direction), and
/// its interval shifted to match, so its form lies on `frame`'s quarter grid. `None` when the
/// circle is not about `frame`'s axis (centre on it, normal along it, within `tol`).
pub fn on_frame(
    c: &Curve,
    [t0, t1]: [f64; 2],
    frame: &Frame,
    tol: f64,
) -> Option<(Curve, [f64; 2])> {
    let Curve::Circle { frame: own, radius } = c else {
        return None;
    };
    let (o, x, _, z) = axes(own)?;
    let (fo, fx, _, fz) = axes(frame)?;
    let d = o - fo;
    if !((d - fz * d.dot(fz)).len() <= tol && (z.dot(fz).abs() - 1.0).abs() <= 1e-9) {
        return None;
    }
    let nx = (fx - z * fx.dot(z)).normalized()?;
    let ny = z.cross(nx);
    let shift = det::atan2(x.dot(ny), x.dot(nx));
    let relabel = Frame {
        origin: [o.x, o.y, o.z],
        x_axis: [nx.x, nx.y, nx.z],
        y_axis: [ny.x, ny.y, ny.z],
    };
    Some((
        Curve::Circle {
            frame: relabel,
            radius: *radius,
        },
        [t0 + shift, t1 + shift],
    ))
}

/// A circle edge round a torus's tube (in a plane through the axis, centred on the ring, of the
/// tube's radius, all within `tol`): the same circle, its angle measured from the tube direction
/// at the torus's tube start (its y axis taken so the circle keeps its own direction), and its
/// interval shifted to match, so its form lies on the torus's quarter grid along the tube. `None`
/// for another circle or surface.
pub fn on_tube(
    c: &Curve,
    [t0, t1]: [f64; 2],
    torus: &Surface,
    tol: f64,
) -> Option<(Curve, [f64; 2])> {
    let (
        Curve::Circle { frame: own, radius },
        Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        },
    ) = (c, torus)
    else {
        return None;
    };
    let (o, x, _, z) = axes(own)?;
    let (fo, _, _, fz) = axes(frame)?;
    // The ring's direction at the circle's centre, and the centre on the ring.
    let d = o - fo;
    let radial = (d - fz * d.dot(fz)).normalized()?;
    let on_ring = (o - (fo + radial * *major_radius)).len() <= tol;
    if !(on_ring
        && (radius - minor_radius).abs() <= tol
        && z.dot(fz).abs() <= 1e-9
        && z.dot(radial).abs() <= 1e-9)
    {
        return None;
    }
    let (s, cs) = det::sin_cos(*tube_start);
    let nx = (radial * cs + fz * s).normalized()?;
    let ny = z.cross(nx);
    let shift = det::atan2(x.dot(ny), x.dot(nx));
    let relabel = Frame {
        origin: [o.x, o.y, o.z],
        x_axis: [nx.x, nx.y, nx.z],
        y_axis: [ny.x, ny.y, ny.z],
    };
    Some((
        Curve::Circle {
            frame: relabel,
            radius: *radius,
        },
        [t0 + shift, t1 + shift],
    ))
}

/// Whether a circle round a torus's tube that is about `frame`'s axis (within `tol`) has one
/// quarter grid on both: its angle from `frame` ([`on_frame`]) and from the tube start
/// ([`on_tube`]) differ by whole quarters to rounding, so its form and both faces' lie on one
/// grid. `None` when it is not such a circle.
pub fn tube_on_grid(
    c: &Curve,
    interval: [f64; 2],
    frame: &Frame,
    torus: &Surface,
    tol: f64,
) -> Option<bool> {
    let (Curve::Circle { frame: a, .. }, _) = on_frame(c, interval, frame, tol)? else {
        return None;
    };
    let (Curve::Circle { frame: b, .. }, _) = on_tube(c, interval, torus, tol)? else {
        return None;
    };
    let (_, ax, _, _) = axes(&a)?;
    let (_, bx, by, _) = axes(&b)?;
    let turn = det::atan2(ax.dot(by), ax.dot(bx));
    let off = turn - (turn / FRAC_PI_2).round() * FRAC_PI_2;
    Some(off.abs() <= 1e-9)
}

/// The frame of a cylinder, cone, sphere or torus, or of a placed revolution (its axis, angle
/// zero at its profile: [`revolution_frame`]); `None` for another surface.
pub fn frame_of(s: &Surface) -> Option<Frame> {
    match s {
        Surface::Cylinder { frame, .. }
        | Surface::Cone { frame, .. }
        | Surface::Sphere { frame, .. }
        | Surface::Torus { frame, .. } => Some(frame.clone()),
        Surface::PlacedRevolution {
            profile,
            origin,
            axis,
            matrix,
            ..
        } => {
            let (_, h) = profile_nurbs(profile, None)?;
            let (o, x, y, _) = revolution_frame(&h, *origin, *axis)?;
            let m = |p: Vec3| affine(matrix, p);
            let (mo, mx, my) = (m(o), m(o + x) - m(o), m(o + y) - m(o));
            Some(Frame {
                origin: [mo.x, mo.y, mo.z],
                x_axis: [mx.x, mx.y, mx.z],
                y_axis: [my.x, my.y, my.z],
            })
        }
        _ => None,
    }
}

/// `matrix` (column-major, `matrix[column][row]`) applied to `p`.
fn affine(matrix: &[[f64; 4]; 4], p: Vec3) -> Vec3 {
    let [c0, c1, c2, c3] = matrix;
    let row = |[a, b, c, d]: [f64; 4]| a * p.x + b * p.y + c * p.z + d;
    Vec3::new(
        row([c0[0], c1[0], c2[0], c3[0]]),
        row([c0[1], c1[1], c2[1], c3[1]]),
        row([c0[2], c1[2], c2[2], c3[2]]),
    )
}

/// A revolution's profile as a NURBS: its knots and homogeneous control points (a B-spline or
/// NURBS as it is, a line or conic as its form over `range`; `None` without a range).
fn profile_nurbs(profile: &Curve, range: Option<[f64; 2]>) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    let formed;
    let c = match profile {
        Curve::Spline { .. } | Curve::RationalSpline { .. } => profile,
        other => {
            formed = curve(other, range?)?.0;
            &formed
        }
    };
    match c {
        Curve::Spline { knots, points } => Some((
            knots.clone(),
            points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        )),
        Curve::RationalSpline { knots, homogeneous } => Some((knots.clone(), homogeneous.clone())),
        _ => None,
    }
}

/// A revolution's frame: the origin and unit axis, x the radial direction of the profile's
/// first control point off the axis (angle zero), y = axis × x.
fn revolution_frame(
    h: &[[f64; 4]],
    origin: [f64; 3],
    axis: [f64; 3],
) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let (o, k) = (v3(origin), v3(axis).normalized()?);
    let x = h.iter().find_map(|h| {
        let d = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]) - o;
        let r = d - k * d.dot(k);
        if r.len() > 1e-12 * (1.0 + d.len()) {
            r.normalized()
        } else {
            None
        }
    })?;
    Some((o, x, k.cross(x), k))
}

/// An edge's label and NURBS form: a circle about the axis of one of `frames` (the cylinder,
/// cone, sphere and torus faces it bounds) is labelled on that frame ([`on_frame`]), so its form
/// lies on that face's quarter grid; a circle or ellipse then has its form ([`curve`]). The label
/// with its interval, and the form with its own; `None` for an arc of more than one turn.
pub fn edge<'a>(
    c: &Curve,
    interval: [f64; 2],
    frames: impl IntoIterator<Item = &'a Frame>,
    tol: f64,
) -> Option<(Curve, [f64; 2], Option<(Curve, [f64; 2])>)> {
    edge_on(c, interval, frames, &[], tol)
}

/// [`edge`], with the torus faces the edge bounds: a circle round one of their tubes that is
/// about none of `frames`' axes is labelled from that torus's tube start ([`on_tube`]), so its
/// form lies on the face's quarter grid along the tube.
pub fn edge_on<'a>(
    c: &Curve,
    interval: [f64; 2],
    frames: impl IntoIterator<Item = &'a Frame>,
    tori: &[Surface],
    tol: f64,
) -> Option<(Curve, [f64; 2], Option<(Curve, [f64; 2])>)> {
    let (label, iv) = frames
        .into_iter()
        .find_map(|f| on_frame(c, interval, f, tol))
        .or_else(|| tori.iter().find_map(|t| on_tube(c, interval, t, tol)))
        .unwrap_or_else(|| (c.clone(), interval));
    let form = match label {
        Curve::Circle { .. } | Curve::Ellipse { .. } => Some(curve(&label, iv)?),
        _ => None,
    };
    Some((label, iv, form))
}

/// Points per edge along a loop taken for a face's extent.
const EXTENT_POINTS: usize = 32;

/// How many times the step that reads an edge's rate round a period is halved to keep it short of
/// a quarter period: from a millionth of the span, 2^-30 of that reads an edge of 2^50 turns.
const PROBE_HALVINGS: usize = 30;

/// The most samples one edge takes: 2048 turns at eight a turn (an edge that winds more is not
/// measured).
const MAX_EXTENT_POINTS: usize = 1 << 14;

/// The largest value of `f` on `[a, b]`, where it rises to one peak and falls (golden section, to
/// rounding; the ends count).
fn golden_max(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    const R: f64 = 0.618_033_988_749_894_8;
    let (mut a, mut b) = (a, b);
    let best = f(a).max(f(b));
    let (mut c, mut d) = (b - R * (b - a), a + R * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    while b - a > 1e-15 * (1.0 + a.abs().max(b.abs())) {
        if fc >= fd {
            b = d;
            d = c;
            fd = fc;
            c = b - R * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + R * (b - a);
            fd = f(d);
        }
    }
    best.max(fc).max(fd)
}

/// The parameter box of a face on `label` over its boundary: each loop's points (`loops`, in
/// order along each loop) projected onto the label, a periodic parameter unwrapped from one point
/// to the next, and each loop after the first a whole number of periods along where that keeps
/// the box smallest. What the face's NURBS form ([`surface`]) is built over; no pcurve is
/// involved.
///
/// A boundary point is on a pole when the label is collapsed there within `reach` mm (the
/// face's edge or vertex tolerance: a pole row an importer left coinciding only to its own
/// tolerance is a pole too), and its collapsed parameter then bounds nothing. A loop that turns
/// more than once on poles and runs along a seam both ways turns the way its face's side says
/// (the loops as the record keeps them, the face on their left in (u, v)), and a seam loop (a
/// meridian up to a pole and back) spans the whole turn whatever holes the face has. `Ok(None)`
/// where the label or a boundary point has no parameters; the classifier's refusal is the error.
pub fn extent(
    label: &Surface,
    loops: &[Vec<(&Curve, [f64; 2], bool)>],
    reach: f64,
) -> Result<Option<[[f64; 2]; 2]>, EvalError> {
    let mut refused = None;
    // On a cylinder or cone the box depends on which loop is placed first: a hole across the
    // seam, placed before the loop that goes round the axis, pulls the box a turn to the wrong
    // side (a tie between two placements of a turn each). The loops are taken widest first, so
    // the order a file lists a face's bounds in does not change the box.
    if loops.len() > 1 && matches!(label, Surface::Cylinder { .. } | Surface::Cone { .. }) {
        let widths: Vec<f64> = loops
            .iter()
            .map(|l| {
                // The raw box: a circle round the axis at one height has no height but a turn's width.
                let mut ignored = None;
                extent_raw(label, std::slice::from_ref(l), reach, &mut ignored)
                    .map_or(0.0, |(lo, hi)| (hi[0] - lo[0]).max(0.0))
            })
            .collect();
        let mut order: Vec<usize> = (0..loops.len()).collect();
        order.sort_by(|a, b| {
            let (wa, wb) = (
                widths.get(*a).copied().unwrap_or(0.0),
                widths.get(*b).copied().unwrap_or(0.0),
            );
            wb.total_cmp(&wa)
        });
        let ordered: Vec<Vec<(&Curve, [f64; 2], bool)>> = order
            .iter()
            .filter_map(|i| loops.get(*i).cloned())
            .collect();
        let out = extent_in(label, &ordered, reach, &mut refused);
        return match refused {
            Some(e) => Err(e),
            None => Ok(out),
        };
    }
    let out = extent_in(label, loops, reach, &mut refused);
    match refused {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// [`extent`]'s body: the classifier's refusal goes to `refused`.
fn extent_in(
    label: &Surface,
    loops: &[Vec<(&Curve, [f64; 2], bool)>],
    reach: f64,
    refused: &mut Option<EvalError>,
) -> Option<[[f64; 2]; 2]> {
    let (lo, hi) = extent_raw(label, loops, reach, refused)?;
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some([[lo[0], hi[0]], [lo[1], hi[1]]])
}

/// [`extent_in`]'s box as corners `(lo, hi)`, before it is checked to have room in both
/// directions: a full circle at one height is a turn wide and no height.
fn extent_raw(
    label: &Surface,
    loops: &[Vec<(&Curve, [f64; 2], bool)>],
    reach: f64,
    refused: &mut Option<EvalError>,
) -> Option<([f64; 2], [f64; 2])> {
    let eval = SurfaceEval::new(label).ok()?;
    let period = eval.domain().period;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    // The most any loop's angle goes back and forth round the axis: its box's width less its net
    // turns.
    let mut back = 0.0_f64;
    for l in loops {
        // A loop is unwrapped from its own first point, so it can come out whole periods away
        // from the loops before it (a hole beside the seam projects to the seam's other side),
        // and a box over both is more than a turn wide.
        let (mut l_lo, mut l_hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut prev: Option<[f64; 2]> = None;
        // The loop's bounds in each direction, each with the number of poles the loop had passed
        // through when it got there; and the first and last points off a pole.
        let mut bounds: Vec<(usize, usize, f64)> = Vec::new();
        let (mut crossings, mut on_pole) = (0usize, false);
        let mut first: Option<[f64; 2]> = None;
        // Whether the loop starts or ends on a pole (its turn from its last point to its first
        // is free there too), the directions a pole it passes collapses, the pole it is on, and
        // the jump each crossing makes by the loop's side ([`pole_jump`]).
        let (mut wraps, mut passes) = (false, [false; 2]);
        // Whether a crossing comes out half a turn round from where it went in ([`half_turn`]):
        // the nearest branch is then no answer.
        let mut half = false;
        let mut pole_at: Option<([f64; 2], [bool; 2])> = None;
        let mut jumps: Vec<[f64; 2]> = Vec::new();
        for (c, [t0, t1], forward) in l {
            let curve = CurveEval::new(c).ok()?;
            // The point at `t` on the label, a periodic parameter unwrapped next to `near`.
            let at = |t: f64, near: Option<[f64; 2]>| -> Option<[f64; 2]> {
                let mut q = eval.project(curve.point(t)?, near, f64::INFINITY)?;
                if let Some(p) = near {
                    for d in 0..2 {
                        if let Some(Some(per)) = period.get(d) {
                            *ixm(&mut q, d) += ((ix(&p, d) - ix(&q, d)) / per).round() * per;
                        }
                    }
                }
                Some(q)
            };
            // Samples enough that each step is well under a turn round a periodic direction: a
            // step of a turn or more reads as none (a thread's flank wound 32 times, sampled 32
            // times, would be one point). Each sample's rate round the direction is taken at the
            // sample itself, a step along the edge apart, so no aliasing is in it.
            let (lowest, highest) = (t0.min(*t1), t0.max(*t1));
            let span = highest - lowest;
            let mut points = EXTENT_POINTS;
            if span > 0.0
                && matches!(label, Surface::Cylinder { .. } | Surface::Cone { .. })
                && period.iter().any(Option::is_some)
            {
                let mut turns = 0.0_f64;
                let mut at_apex = false;
                for k in 0..=EXTENT_POINTS {
                    let t = lowest + span * k as f64 / EXTENT_POINTS as f64;
                    let Some(q) = at(t, None) else { continue };
                    // The angle round a cone's axis is not defined at its apex: a sample there
                    // reads no rate (an edge along a generator to the apex does not wind).
                    if matches!(label, Surface::Cone { .. }) {
                        match eval.pole_within(q, reach) {
                            Ok(p) if p.iter().any(|p| *p) => {
                                at_apex = true;
                                continue;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                *refused = Some(e);
                                return None;
                            }
                        }
                    }
                    // The rate is read over a step short enough that the point has advanced less
                    // than a quarter of a period: a step of a whole turn or more (an edge of a
                    // million turns, probed over a millionth of its span) projects back onto the
                    // point it started from and reads as no winding. A step is halved until it
                    // is short enough; an edge that still advances that far over the smallest
                    // step is wound beyond what can be measured and is refused.
                    let mut step = span * 1e-6;
                    let mut read = None;
                    let mut unplaced = false;
                    for _ in 0..PROBE_HALVINGS {
                        let other = if t + step <= highest {
                            t + step
                        } else {
                            t - step
                        };
                        let Some(r) = at(other, Some(q)) else {
                            unplaced = true;
                            break;
                        };
                        let too_far = (0..2).any(|d| {
                            matches!(period.get(d), Some(Some(per))
                                if (ix(&r, d) - ix(&q, d)).abs() >= 0.25 * per)
                        });
                        if !too_far {
                            read = Some((r, step));
                            break;
                        }
                        step *= 0.5;
                    }
                    let Some((r, step)) = read else {
                        // A point that could not be placed is skipped, as before; one that
                        // still advances a quarter period over the smallest step is wound
                        // beyond what can be measured: the edge is refused.
                        if unplaced {
                            continue;
                        }
                        return None;
                    };
                    for d in 0..2 {
                        if let Some(Some(per)) = period.get(d) {
                            turns = turns.max((ix(&r, d) - ix(&q, d)).abs() / step * span / per);
                        }
                    }
                }
                // An edge through the apex that winds elsewhere has no angle there to follow.
                if at_apex && turns > 1.0 {
                    *refused = Some(EvalError::Invalid(
                        "a face that winds round a cone's axis with an edge through its apex"
                            .into(),
                    ));
                    return None;
                }
                // Eight samples a turn: no step is more than an eighth.
                let needed = (8.0 * turns).ceil();
                if !needed.is_finite() || needed > MAX_EXTENT_POINTS as f64 {
                    return None;
                }
                points = points.max(needed as usize);
            }
            let mut samples: Vec<(f64, [f64; 2], [bool; 2], usize)> = Vec::new();
            for k in 0..=points {
                let f = k as f64 / points as f64;
                let t = if *forward {
                    t0 * (1.0 - f) + t1 * f
                } else {
                    t1 * (1.0 - f) + t0 * f
                };
                // Past a pole the point before it says nothing of where the loop goes on (it is
                // across the axis), and a nearest-point search started there settles on the
                // pole itself, the nearest point on that side: a meridian back out of the pole,
                // and the edge after it, would read as all pole and bound nothing. The point is
                // found over the whole surface until the loop is off the pole again, then moved
                // the whole periods that bring it nearest the point before the pole, so the loop
                // stays on one branch across the pole (a narrow lune across angle 0 keeps its
                // width instead of reading as the rest of the turn).
                let q = if on_pole {
                    let mut q = at(t, None)?;
                    if let Some(p) = prev {
                        for d in 0..2 {
                            if let Some(Some(per)) = period.get(d) {
                                let k = ((ix(&p, d) - ix(&q, d)) / per).round();
                                *ixm(&mut q, d) = ix(&q, d) + k * per;
                            }
                        }
                    }
                    q
                } else {
                    at(t, prev)?
                };
                // At a pole the collapsed parameter is anything: it bounds nothing.
                let pole = match eval.pole_within(q, reach) {
                    Ok(p) => p,
                    Err(e) => {
                        *refused = Some(e);
                        return None;
                    }
                };
                if pole.iter().any(|p| *p) {
                    on_pole = first.is_some();
                    wraps |= first.is_none();
                    for (s, p) in passes.iter_mut().zip(pole) {
                        *s |= p;
                    }
                    pole_at = Some((q, pole));
                } else {
                    if on_pole {
                        crossings += 1;
                        on_pole = false;
                        let jump = match (prev, pole_at) {
                            (Some(a), Some(on)) => {
                                half |= half_turn(&eval, a, q, on, period, reach);
                                pole_jump(a, q, on, period)
                            }
                            _ => [0.0; 2],
                        };
                        jumps.push(jump);
                    }
                    first = first.or(Some(q));
                    prev = Some(q);
                }
                for (d, x) in q.iter().enumerate() {
                    if !pole.get(d).copied().unwrap_or(false) {
                        bounds.push((crossings, d, *x));
                    }
                }
                samples.push((t, q, pole, crossings));
            }
            // A sample is a point of the boundary, not its extreme: each sample higher (or lower)
            // than its neighbours brackets an extreme of the edge, which is found there.
            for k in 0..samples.len() {
                let Some(&(_, q, pole, passed)) = samples.get(k) else {
                    continue;
                };
                let (Some(a), Some(b)) = (
                    samples.get(k.saturating_sub(1)),
                    samples.get((k + 1).min(samples.len() - 1)),
                ) else {
                    continue;
                };
                let (a, b) = (a.0, b.0);
                for d in 0..2 {
                    // Next to a pole the parameter it collapses is no function of the point: a
                    // meridian's angle there is rounding, not an extreme.
                    let near_pole = |j: Option<usize>| {
                        j.and_then(|j| samples.get(j)).is_some_and(|s| ix(&s.2, d))
                    };
                    if ix(&pole, d) || near_pole(k.checked_sub(1)) || near_pole(Some(k + 1)) {
                        continue;
                    }
                    let side = |j: usize| samples.get(j).map(|s| ix(&s.1, d));
                    let (before, after) = (if k > 0 { side(k - 1) } else { None }, side(k + 1));
                    for sign in [1.0, -1.0] {
                        // Not lower than either neighbour and higher than one (a level run, a
                        // seam's constant angle, has no extreme between its samples).
                        let higher =
                            |x: Option<f64>| x.is_none_or(|x| sign * ix(&q, d) >= sign * x);
                        let above = |x: Option<f64>| x.is_some_and(|x| sign * ix(&q, d) > sign * x);
                        if !(higher(before) && higher(after) && (above(before) || above(after))) {
                            continue;
                        }
                        let f =
                            |t: f64| at(t, Some(q)).map_or(f64::NEG_INFINITY, |p| sign * ix(&p, d));
                        let x = sign * golden_max(&f, a.min(b), a.max(b));
                        bounds.push((passed, d, x));
                    }
                }
            }
        }
        // Through a pole the loop goes on from anywhere round the periodic direction it
        // collapses, so continuing from the point before it is no measure of where: a loop
        // through a pole closes in (u, v), the pole taking up its turn (a hemisphere bounded by its
        // rim and its seam, down the seam to the pole and up it on the other side). Past the
        // first pole, the loop is moved the whole periods that close it.
        //
        // That holds for a loop with one free turn. One that passes a pole twice, counting the
        // turn from its last point to its first (a sphere less a bore that crosses its seam, up
        // the seam on one side of the bore, over the pole and down the other side), closes
        // whatever its turns are, so closing it says nothing. Where it also runs along an edge
        // both ways (a seam: the face spans the whole turn there), each crossing turns the way
        // the face's side says instead ([`pole_jump`]). A loop with no seam (a narrow lune, up
        // one meridian and down another) keeps closing: its width is the one it shows. A lune
        // half a turn wide (a half sphere whose rim is a meridian circle, both its halves a lune
        // of the same width) shows no width of its own: the nearest branch past each pole is a
        // tie that rounding settles, and either half could come out. It turns the way the
        // face's side says too.
        wraps |= on_pole;
        let by_side = crossings + usize::from(wraps) >= 2
            && (half
                || l.iter().enumerate().any(|(i, (c, t, f))| {
                    l.iter()
                        .skip(i + 1)
                        .any(|(c2, t2, f2)| f != f2 && t == t2 && c == c2)
                }));
        // The turn the points past each crossing are moved by: none before the first.
        let turned: Vec<[f64; 2]> = std::iter::once([0.0; 2])
            .chain(jumps.iter().scan([0.0; 2], |s, j| {
                *s = [s[0] + j[0], s[1] + j[1]];
                Some(*s)
            }))
            .collect();
        let last = prev;
        let mut net_turn = 0.0;
        for d in 0..2 {
            // A loop of no width round a periodic direction that passes a pole collapsing it
            // runs up a meridian and back down it, a seam: the face it bounds covers the whole
            // turn from there, whatever other loops (holes) it has.
            if let Some(Some(per)) = period.get(d) {
                let (raw_lo, raw_hi) = bounds
                    .iter()
                    .filter(|b| b.1 == d)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| {
                        (lo.min(b.2), hi.max(b.2))
                    });
                if ix(&passes, d) && raw_lo <= raw_hi && raw_hi - raw_lo <= 1e-9 * per {
                    *ixm(&mut l_lo, d) = raw_lo;
                    *ixm(&mut l_hi, d) = raw_lo + per;
                    continue;
                }
            }
            let shift = match (period.get(d), first, last) {
                (Some(Some(per)), Some(f), Some(e)) if crossings > 0 => {
                    -((ix(&e, d) - ix(&f, d)) / per).round() * per
                }
                _ => 0.0,
            };
            if d == 0 {
                // The loop's net angle, from its first point to its last (a whole number of turns
                // where it closes).
                if let (Some(f), Some(e)) = (first, last) {
                    net_turn = ix(&e, 0) - ix(&f, 0) + shift;
                }
            }
            for &(passed, dd, x) in &bounds {
                if dd == d {
                    let x = if by_side {
                        x + turned.get(passed).map_or(0.0, |s| ix(s, d))
                    } else if passed > 0 {
                        x + shift
                    } else {
                        x
                    };
                    *ixm(&mut l_lo, d) = ix(&l_lo, d).min(x);
                    *ixm(&mut l_hi, d) = ix(&l_hi, d).max(x);
                }
            }
        }
        if ix(&l_lo, 0) <= ix(&l_hi, 0) {
            // A turn of net winding is the face's own (a screw's cylinder goes round once, and
            // down its thread many times); more is not backing up.
            let own = period
                .first()
                .copied()
                .flatten()
                .map_or(0.0, |per| net_turn.abs().min(per));
            back = back.max(ix(&l_hi, 0) - ix(&l_lo, 0) - own);
        }
        for d in 0..2 {
            if !(ix(&l_lo, d) <= ix(&l_hi, d)) {
                continue;
            }
            let shift = match period.get(d) {
                Some(Some(per)) if ix(&lo, d) <= ix(&hi, d) => {
                    // The whole periods around the ones that bring the loop's middle to the box's,
                    // the one giving the smallest box.
                    let k = ((ix(&lo, d) + ix(&hi, d) - ix(&l_lo, d) - ix(&l_hi, d)) / (2.0 * per))
                        .round();
                    let span = |k: f64| {
                        ix(&hi, d).max(ix(&l_hi, d) + k * per)
                            - ix(&lo, d).min(ix(&l_lo, d) + k * per)
                    };
                    [k, k - 1.0, k + 1.0]
                        .into_iter()
                        .min_by(|a, b| span(*a).total_cmp(&span(*b)))
                        .unwrap_or(k)
                        * per
                }
                _ => 0.0,
            };
            *ixm(&mut lo, d) = ix(&lo, d).min(ix(&l_lo, d) + shift);
            *ixm(&mut hi, d) = ix(&hi, d).max(ix(&l_hi, d) + shift);
        }
    }
    // At most one turn round a periodic direction, to rounding; a boundary that never goes round
    // it (a face bounded by its seam alone) covers the whole turn from there. On a cylinder or
    // cone, a boundary that goes round the axis more than a turn keeps its box (a thread's flank
    // winds several): clamped to a turn, the face's edges would lie off any form made over it.
    // That is a loop that goes back and forth round the axis by more than a turn (`back`: its
    // box's width less its net turns): a loop that goes round once and backs up a little (along
    // a notch) has a box a little wider than a turn and is clamped to the turn it goes round.
    // "More than a turn" allows what a turn's box can come to: rounding, and twice the angle
    // `reach` makes at the face's widest circle (its edges lie off the surface by up to `reach`,
    // so each end of the box may be that far out). No other constant is chosen.
    let may_wind = matches!(label, Surface::Cylinder { .. } | Surface::Cone { .. });
    let (reach_angle, _) = reaches(label, [lo[1], hi[1]], reach);
    for d in 0..2 {
        if let Some(Some(per)) = period.get(d) {
            let span = ix(&hi, d) - ix(&lo, d);
            let room = per * TURN_ROUNDING + 2.0 * reach_angle;
            *ixm(&mut hi, d) = if span <= 1e-9 * per {
                ix(&lo, d) + per
            } else if !may_wind || d != 0 || span <= per + room || back <= per + room {
                ix(&hi, d).min(ix(&lo, d) + per)
            } else {
                ix(&hi, d)
            };
        }
    }
    Some((lo, hi))
}

/// Whether a loop through a pole comes out of it half a turn round from where it went in, so
/// that neither way round is the shorter: `a` the last point before the pole, `b` the first after,
/// `p` the pole's parameters and `collapsed` the directions collapsed there (as [`pole_jump`]).
/// Half a turn to within rounding and the angle `reach` (the face's tolerance) makes at the nearer
/// of the two points, each a tolerance off its place, taken at its distance from the pole (no
/// more than its distance from the axis there, so the angle is no smaller than it should be).
fn half_turn(
    eval: &SurfaceEval,
    a: [f64; 2],
    b: [f64; 2],
    (p, collapsed): ([f64; 2], [bool; 2]),
    period: [Option<f64>; 2],
    reach: f64,
) -> bool {
    let (Some(pa), Some(pb), Some(pole)) = (eval.point(a), eval.point(b), eval.point(p)) else {
        return false;
    };
    let near = pa.dist(pole).min(pb.dist(pole));
    if !(near > 0.0) {
        return false;
    }
    let angle = 2.0 * det::asin((reach.max(0.0) / near).min(1.0));
    (0..2).any(|d| {
        let per = ix(&period, d).filter(|p| p.is_finite() && *p > 0.0);
        let (true, Some(per)) = (ix(&collapsed, d), per) else {
            return false;
        };
        let gap = (ix(&b, d) - ix(&a, d)).rem_euclid(per);
        (gap - 0.5 * per).abs() <= per * TURN_ROUNDING + angle
    })
}

/// The whole periods a loop's points after a pole are moved by: `a` the last point before it,
/// `b` the first after (unwrapped next to `a`), `p` the pole's parameters and `collapsed` the
/// directions collapsed there. The loop has the face on its left in (u, v) (outer loops counter-clockwise
/// about the natural normal), so along a pole's line it runs the way that keeps the face inside:
/// with u collapsed, −u at the top of v and +u at the bottom; with v collapsed, +v at the top of
/// u and −v at the bottom. It turns by more than nothing and at most one period that way: a
/// meridian run up to the pole and straight back down (a seam) turns the whole period round.
fn pole_jump(
    a: [f64; 2],
    b: [f64; 2],
    (p, collapsed): ([f64; 2], [bool; 2]),
    period: [Option<f64>; 2],
) -> [f64; 2] {
    let mut out = [0.0; 2];
    for d in 0..2 {
        let per = ix(&period, d).filter(|p| p.is_finite() && *p > 0.0);
        let (true, Some(per)) = (ix(&collapsed, d), per) else {
            continue;
        };
        let e = 1 - d;
        let top = ix(&p, e) > ix(&a, e);
        let sign = if (d == 0) == top { -1.0 } else { 1.0 };
        let gap = sign * (ix(&b, d) - ix(&a, d));
        let least = 1e-9 * per;
        let k = if gap > least {
            -((gap - least) / per).ceil() + 1.0
        } else {
            ((least - gap) / per).floor() + 1.0
        };
        *ixm(&mut out, d) = sign * k * per;
    }
    out
}

/// Element `d` (0 or 1) of a pair.
fn ix<T: Copy>(a: &[T; 2], d: usize) -> T {
    let [x, y] = *a;
    if d == 0 {
        x
    } else {
        y
    }
}

/// Element `d` (0 or 1) of a pair, to write.
fn ixm<T>(a: &mut [T; 2], d: usize) -> &mut T {
    let [x, y] = a;
    if d == 0 {
        x
    } else {
        y
    }
}
