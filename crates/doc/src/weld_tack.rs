//! Tack welds: the short welds that hold parts in place before they are welded, placed along a
//! joint ([`tack_weld`]) and spaced by [`tack_spans`].
//!
//! SolidCraft's own, as the spot welds are ([`crate::weld_spot`]): every body is a kernel call
//! that reads the parts and never changes them; this module adds the tack's rules. The tack is
//! its own weld type in the saved record ([`crate::weld::WeldSettings`]: type `tack`, its
//! `tack_mode` and sizes `l`, `e`, `n`); the `weld.tack` command comes with #15131.
//!
//! | Rust              | What it is                                                         |
//! |-------------------|--------------------------------------------------------------------|
//! | [`TackJoint`]     | The joint: its path of lines and arcs, and the two faces it joins  |
//! | [`TackPlacement`] | Length + pitch, count + length, or one tack at each picked point   |
//! | [`tack_spans`]    | Where the tacks start and end along the path                       |
//! | [`tack_weld`]     | The tacks' bodies, their count and length, and the notices         |
//! | [`TackRefusal`]   | Why tacks are refused                                              |
//!
//! # The geometry
//!
//! A tack is a dome lying in the joint's corner: an ellipsoid centred on the joint at the
//! tack's middle, `l / 2` along the joint's tangent there and `r = l / 2.25`
//! ([`LENGTH_OVER_RADIUS`]) across it, with both parts taken away, then kept on the inner side
//! of each face's tangent plane at that point, so it stays in the angle between the faces (a
//! thin plate does not let it through to the far side). On a T joint that is a quarter of the
//! ellipsoid, on a butt joint a half; on a tube standing on a plate it follows the tube.
//!
//! Length + pitch puts the first tack at the joint's start and leaves what is over at its end;
//! count + length puts the first and last tack at the joint's ends (one tack: in the middle); a
//! picked point gets a tack centred on the joint's nearest place to it.
//!
//! # Off by a little
//!
//! A tack running past the joint's end is cut there, tacks that overlap are built, and a point
//! off the joint is moved onto it: each with a notice. A point farther from the joint than a
//! tack is long is refused.
//!
//! # Not covered
//!
//! Joints along free-form curves (a spline edge) are refused as not built yet.

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::Plane;
use solidcraft_geom::PlaneGeom;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, KernelError, affine, half_space};

use crate::weld_seam::{add, len, scale, sub};

/// A tack's length over the radius of its section: the dome chosen for the tack.
pub const LENGTH_OVER_RADIUS: f64 = 2.25;

/// The most tacks one weld makes, as the saved spans allow ([`crate::weld::WeldSpans`]).
pub const MAX_TACKS: usize = 10_000;

/// Why tacks are refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TackRefusal {
    /// Input the tacks cannot take: a length, pitch, count, point or path out of range.
    #[error("{0}")]
    Invalid(&'static str),
    /// Piece `piece` of the joint's path is not a line or an arc.
    #[error("piece {piece} of the joint is a free-form curve: tacks along it are not built yet")]
    NotLineOrArc { piece: usize },
    /// Picked point `point` lies `distance` from the joint, farther than a tack is long: pick it
    /// nearer the joint.
    #[error("point {point} lies {distance} from the joint, farther than a tack is long; pick it nearer the joint")]
    PointOffJoint { point: usize, distance: f64 },
    /// At tack `tack` one of the joint's faces has no normal (the point is off the face, or on a
    /// pole or an apex).
    #[error("at tack {tack} a face of the joint has no normal there")]
    NoNormal { tack: usize },
    /// Tack `tack` lies wholly inside the parts.
    #[error("tack {tack} lies wholly inside the parts")]
    Buried { tack: usize },
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

type Result<T> = std::result::Result<T, TackRefusal>;

fn bad<T>(why: &'static str) -> Result<T> {
    Err(TackRefusal::Invalid(why))
}

/// Where the tacks go along the joint.
#[derive(Debug, Clone, PartialEq)]
pub enum TackPlacement {
    /// Tacks of `length` every `pitch` (start to start) along the joint, the first at its start.
    Pitch { length: f64, pitch: f64 },
    /// `count` tacks of `length`, the first and last at the joint's ends; one tack in the middle.
    Count { count: usize, length: f64 },
    /// One tack of `length` centred on the joint's nearest place to each point.
    Points { length: f64, points: Vec<[f64; 3]> },
}

impl TackPlacement {
    fn length(&self) -> f64 {
        match *self {
            Self::Pitch { length, .. } | Self::Count { length, .. } | Self::Points { length, .. } => length,
        }
    }
}

/// The joint the tacks hold: its path, and the two faces it joins (a part and its face index;
/// both may be faces of one part).
#[derive(Debug, Clone)]
pub struct TackJoint<'a> {
    /// The joint's path: lines and arcs, end to end, as the kernel gives an edge's pieces.
    pub path: Vec<Curve>,
    pub faces: [(&'a Body, usize); 2],
}

/// Something about the tacks the user should know, though they build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TackNotice {
    /// Tack `tack` ran past an end of the joint and was cut there to `length`.
    Cut { tack: usize, length: f64 },
    /// Tack `tack` overlaps the tack before it.
    Overlap { tack: usize },
    /// Tack `tack`'s point lay `distance` off the joint and was moved onto it.
    Moved { tack: usize, distance: f64 },
}

/// The tacks of one weld.
#[derive(Debug, Clone)]
pub struct Tacks {
    /// One body per tack, in placement order; each for display only, never united.
    pub bodies: Vec<Body>,
    /// The tacks' total length along the joint, for the weld table, where tacks are counted on
    /// their own.
    pub length: f64,
    pub notices: Vec<TackNotice>,
}

/// The joint's path laid out: each piece with where it starts along the path and its length.
struct Path {
    pieces: Vec<(f64, Curve, f64)>,
    length: f64,
}

impl Path {
    fn new(path: &[Curve], tol: f64) -> Result<Self> {
        if path.is_empty() {
            return bad("a tack joint with no path");
        }
        let mut pieces = Vec::new();
        let mut along = 0.0;
        for (i, piece) in path.iter().enumerate() {
            let length = match *piece {
                Curve::Line { start, end } => len(sub(end, start)),
                Curve::Arc { radius, start, end, .. } => radius * (end - start),
                Curve::Nurbs { .. } => return Err(TackRefusal::NotLineOrArc { piece: i }),
            };
            if !length.is_finite() || length < tol {
                return bad("a joint piece of no length, or not finite");
            }
            pieces.push((along, piece.clone(), length));
            along += length;
        }
        let path = Self { pieces, length: along };
        for w in path.pieces.windows(2) {
            let [(_, before, length), (_, after, _)] = w else { continue };
            if len(sub(point(before, *length)?.0, point(after, 0.0)?.0)) > tol {
                return bad("joint pieces that do not meet end to end");
            }
        }
        Ok(path)
    }

    /// The point `s` along the path and the path's unit tangent there.
    fn at(&self, s: f64) -> Result<([f64; 3], [f64; 3])> {
        let i = self.pieces.iter().rposition(|(start, _, _)| *start <= s).unwrap_or(0);
        match self.pieces.get(i) {
            Some((start, curve, length)) => point(curve, (s - start).clamp(0.0, *length)),
            None => bad("a tack joint with no path"),
        }
    }

    /// The path's nearest place to `p` (how far along it) and its distance from `p`.
    fn nearest(&self, p: [f64; 3]) -> Result<(f64, f64)> {
        let mut best = (0.0, f64::INFINITY);
        for (begun, curve, length) in &self.pieces {
            let k = match *curve {
                Curve::Line { start, end } => affine::distance_along(start, scale(sub(end, start), 1.0 / length), p).clamp(0.0, *length),
                Curve::Arc { center, axis, reference, radius, start, end } => {
                    let sweep = end - start;
                    // a point on the arc's axis is as near every point of it: take its start
                    let turned = match affine::orthonormal_axes(axis, sub(p, center)) {
                        Ok([_, u]) => (affine::signed_unit_angle(axis, reference, u)? - start).rem_euclid(TAU),
                        Err(_) => 0.0,
                    };
                    // past the arc's end: whichever end is nearer round the circle
                    let turned = if turned <= sweep {
                        turned
                    } else if turned - sweep < TAU - turned {
                        sweep
                    } else {
                        0.0
                    };
                    radius * turned
                }
                Curve::Nurbs { .. } => continue,
            };
            let d = len(sub(p, self.at(begun + k)?.0));
            if d < best.1 {
                best = (begun + k, d);
            }
        }
        Ok(best)
    }
}

/// The point `k` along `curve` from its start and its unit tangent there.
fn point(curve: &Curve, k: f64) -> Result<([f64; 3], [f64; 3])> {
    match *curve {
        Curve::Line { start, end } => {
            let d = sub(end, start);
            let t = scale(d, 1.0 / len(d));
            Ok((add(start, scale(t, k)), t))
        }
        Curve::Arc { center, axis, reference, radius, start, .. } => {
            let turn = start + k / radius;
            let p = affine::apply_point(&affine::rotation(center, axis, turn)?, add(center, scale(reference, radius)));
            let t = affine::apply_point(&affine::rotation([0.0; 3], axis, turn + FRAC_PI_2)?, reference);
            Ok((p, t))
        }
        Curve::Nurbs { .. } => bad("a free-form joint piece"),
    }
}

/// Where the tacks start and end along a path of `length` (Pitch and Count; Points need the
/// path, [`tack_weld`]), and the notices: tacks cut at the path's ends, tacks that overlap.
///
/// # Errors
///
/// [`TackRefusal::Invalid`] for a length, pitch or count that is not finite and positive, or
/// more tacks than [`MAX_TACKS`].
pub fn tack_spans(length: f64, placement: &TackPlacement) -> Result<(Vec<[f64; 2]>, Vec<TackNotice>)> {
    let l = placement.length();
    if !(length.is_finite() && length > 0.0 && l.is_finite() && l > 0.0) {
        return bad("a tack or joint length that is not finite and positive");
    }
    let starts: Vec<f64> = match *placement {
        TackPlacement::Pitch { pitch, .. } => {
            if !(pitch.is_finite() && pitch > 0.0) {
                return bad("a tack pitch that is not finite and positive");
            }
            let room = (length - l).max(0.0);
            // a hair under a whole pitch still fits its tack
            let n = (room / pitch + 1e-9).floor() + 1.0;
            if n > MAX_TACKS as f64 {
                return bad("a tack pitch so small that the joint has too many tacks");
            }
            (0..n as usize).map(|i| i as f64 * pitch).collect()
        }
        TackPlacement::Count { count, .. } => {
            if count == 0 || count > MAX_TACKS {
                return bad("a tack count of none, or more than the most tacks");
            }
            if count == 1 {
                vec![(length - l) / 2.0]
            } else {
                let pitch = (length - l) / (count - 1) as f64;
                (0..count).map(|i| i as f64 * pitch).collect()
            }
        }
        TackPlacement::Points { .. } => return bad("tacks at points are placed by tack_weld"),
    };
    Ok(clip(length, starts.into_iter().map(|s| [s, s + l]).collect()))
}

/// The spans cut to `[0, length]`, with a notice for each one cut and each one that overlaps the
/// one before it.
fn clip(length: f64, spans: Vec<[f64; 2]>) -> (Vec<[f64; 2]>, Vec<TackNotice>) {
    let mut notices = Vec::new();
    let mut out: Vec<[f64; 2]> = Vec::new();
    for (i, [a, b]) in spans.into_iter().enumerate() {
        let span = [a.max(0.0), b.min(length)];
        if span != [a, b] {
            notices.push(TackNotice::Cut { tack: i, length: span[1] - span[0] });
        }
        if out.last().is_some_and(|before| span[0] < before[1]) {
            notices.push(TackNotice::Overlap { tack: i });
        }
        out.push(span);
    }
    (out, notices)
}

/// The tacks of `placement` along `joint`: one dome per tack (see the module's notes), the
/// parts never changed.
///
/// # Errors
///
/// [`TackRefusal::Invalid`] for input out of range (see [`tack_spans`]), a path whose pieces do
/// not meet, or more points than [`MAX_TACKS`]; [`TackRefusal::NotLineOrArc`];
/// [`TackRefusal::PointOffJoint`]; [`TackRefusal::NoNormal`]; [`TackRefusal::Buried`]; the
/// errors of the kernel calls otherwise.
pub fn tack_weld(joint: &TackJoint, placement: &TackPlacement) -> Result<Tacks> {
    let tol = profiles::zone_tolerance()?;
    let path = Path::new(&joint.path, tol)?;
    let l = placement.length();
    let (spans, mut notices) = match placement {
        TackPlacement::Points { points, .. } => {
            if points.is_empty() || points.len() > MAX_TACKS || !(l.is_finite() && l > 0.0) {
                return bad("tacks at no points or too many, or a tack length that is not finite and positive");
            }
            let mut moved = Vec::new();
            let mut spans = Vec::new();
            for (i, &p) in points.iter().enumerate() {
                if !p.iter().all(|c| c.is_finite()) {
                    return bad("a tack point that is not finite");
                }
                let (s, distance) = path.nearest(p)?;
                if distance > l {
                    return Err(TackRefusal::PointOffJoint { point: i, distance });
                }
                if distance > tol {
                    moved.push(TackNotice::Moved { tack: i, distance });
                }
                spans.push([s - l / 2.0, s + l / 2.0]);
            }
            let (spans, mut notices) = clip(path.length, spans);
            notices.extend(moved);
            (spans, notices)
        }
        _ => tack_spans(path.length, placement)?,
    };
    notices.sort_by_key(|n| match *n {
        TackNotice::Cut { tack, .. } | TackNotice::Overlap { tack } | TackNotice::Moved { tack, .. } => tack,
    });
    let r = l / LENGTH_OVER_RADIUS;
    let mut bodies = Vec::new();
    for (i, &[a, b]) in spans.iter().enumerate() {
        bodies.push(dome(joint, &path, i, (a + b) / 2.0, b - a, r)?);
    }
    Ok(Tacks { bodies, length: spans.iter().map(|[a, b]| b - a).sum(), notices })
}

/// Tack `i`: the ellipsoid of `length` along the path at `s` and radius `r` across it, less the
/// parts, kept on the inner side of each face's tangent plane there.
fn dome(joint: &TackJoint, path: &Path, i: usize, s: f64, length: f64, r: f64) -> Result<Body> {
    let (p, t) = path.at(s)?;
    let mut normals = [[0.0; 3]; 2];
    for (n, &(body, face)) in normals.iter_mut().zip(&joint.faces) {
        let found = body.face_normal_at(face, p.into())?.ok_or(TackRefusal::NoNormal { tack: i })?;
        *n = [found.x, found.y, found.z];
    }
    // across the joint: into the corner (both faces' normals point out of their parts, into it),
    // and square to that
    let [t, into] = affine::orthonormal_axes(t, add(normals[0], normals[1]))?;
    let side = affine::apply_point(&affine::rotation([0.0; 3], t, -FRAC_PI_2)?, into);
    let axis = |v: [f64; 3], k: f64| [v[0] * k, v[1] * k, v[2] * k, 0.0];
    let place = [axis(t, length / 2.0), axis(into, r), axis(side, r), [p[0], p[1], p[2], 1.0]];
    let mut body = profiles::ball([0.0; 3], 1.0, Some(&place))?;
    let (a, b) = (joint.faces[0].0, joint.faces[1].0);
    for part in if std::ptr::eq(a, b) { vec![a] } else { vec![a, b] } {
        body = solidcraft_kernel::boolean(&body, part, BoolOp::Cut)?.ok_or(TackRefusal::Buried { tack: i })?;
    }
    for n in normals {
        // each face's tangent plane through p; keep the side its outward normal points to
        let plane = Plane::from_normal(p.into(), n).ok_or(TackRefusal::Invalid("a face's tangent plane the kernel cannot make"))?;
        body = half_space(&body, &plane, true)?.0.ok_or(TackRefusal::Buried { tack: i })?;
    }
    Ok(body)
}

#[cfg(test)]
#[path = "weld_tack_tests.rs"]
mod tests;
