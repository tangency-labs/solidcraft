//! A sweep's stations from its laws: where along its path, and in what frame, the profile is
//! placed, for a path of lines, circular arcs and helices, with the profile's orientation,
//! twist, end scale and up to two guide rails.
//!
//! Upstream: none (not in SMLib). The laws are SolidCraft's (`crates/kernel/src/sweep_path.rs`
//! at 52afe21, `SweepControl`, `controlled_stations`, `rail_frame`), moved into the kernel so
//! no application computes a frame.
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`Path`], [`Piece`] | (none) |
//! | [`Laws`], [`Orientation`], [`RailFit`] | (none; SolidCraft's `SweepControl`) |
//! | [`Station`]       | (none; SolidCraft's `Station`) |
//! | [`stations`]      | (none; SolidCraft's `controlled_stations`) |
//!
//! **A station** is a point `at` of the path and a linear map `m` (its columns the images of
//! the axes): a point `q` of the profile at the first station is placed at
//! `at + m (q - at_0)`, `at_0` the first station's point.
//!
//! **The frame (exact).** Along the path the profile is carried by the path's
//! rotation-minimizing frame (parallel transport), which never turns about the tangent. The
//! map from the first station's frame to a station's is a rotation `R`, independent of the
//! frame the first station starts in, and in closed form on every piece:
//! - a **line**: the identity;
//! - a **circular arc** about the unit axis `n`: the rotation about `n` by the angle run, as
//!   the arc's tangent turns (a planar curve's rotation-minimizing frame is its Frenet frame's
//!   turn in its plane);
//! - a **helix** of radius `a` and rise `b` per radian about `n`: the screw's rotation about
//!   `n` by the angle run `phi`, then about the new tangent by `-tau s = -b phi / sqrt(a^2 +
//!   b^2)`. The Frenet frame of a helix is carried by the screw; the rotation-minimizing normal
//!   `U = cos psi N + sin psi B` keeps `U'` along the tangent where `psi' = -tau` (Frenet:
//!   `N' = -kappa T + tau B`), `tau = b / (a^2 + b^2)` and `s = phi sqrt(a^2 + b^2)` (Wang,
//!   Juettler, Zheng and Liu, *Computation of rotation minimizing frames*, ACM TOG 27, 2008, 2;
//!   Guggenheimer, *Computing frames along a trajectory*, CAGD 6, 1989);
//! - a **corner** between pieces, tangents `t_0` to `t_1`: the least rotation taking one to the
//!   other, about `t_0 x t_1` by the angle between them (the limit of the double reflection
//!   across a vanishing segment); none where the pieces meet G1, by the procedural sweep's
//!   rule (unit tangents within `1e-8`, docs/procedural-sweep.md section 3.1).
//!
//! The line and arc are the procedural sweep's closed forms
//! ([`crate::surface::procedural_sweep::path`], `frame_side`), and both use its one rotation
//! (`rodrigues`) and G1 rule (`G1_TANGENTS`). What is not shared is the span: there a span of
//! a B-spline path is classified by samples and carries one normal; here a piece is given as
//! a line, arc or helix and carries the whole transport, through helices and corners, which
//! the procedural sweep has no frame for.
//!
//! Each is a rigid rotation computed by Rodrigues' formula, so a frame is orthonormal to
//! rounding: its defect, the largest of `| |c_i|^2 - 1 |` and `|c_i . c_j|` over its columns,
//! is `O(k eps)` for `k` composed rotations.
//!
//! **The laws (SolidCraft's conventions, carried over, not derived here).** Only geometric
//! limits are refused here (a scale not positive, a path not crossing a parallel profile's
//! plane or running along the orientation direction, a corner doubling back); SolidCraft's
//! limits on them (four turns of twist, scales 0.01 to 100, 5 degrees and `1e-3`) are the
//! ops entry's, labelled there, and the last two reach this module as
//! [`Laws::min_crossing`] and [`Laws::min_across`].
//! - `twist` turns the profile about its normal (turned to point the way the path leaves it)
//!   by `twist u` radians, right-handed, and `end_scale` scales it about the path by
//!   `1 + (end_scale - 1) u`, `u` the fraction of the path's arc length run.
//! - A station asked as [`Stop::Corner`] is that corner's mitre (a fraction is never taken for
//!   one): the rotation by half the corner's angle, stretched by `1 / cos(theta / 2)` along
//!   `t_1 - t_0`.
//! - [`Orientation::Parallel`] keeps the profile as drawn (the identity), and needs the path
//!   to cross the profile's plane at every station, at more than [`Laws::min_crossing`] (the
//!   angle between the tangent and the plane); [`Orientation::Direction`] keeps the
//!   direction's projection square to the tangent, and needs the path to stay off the
//!   direction, the sine of the angle between them more than [`Laws::min_across`].
//! - Guide rails take the default orientation, no twist and scale 1, and a path without
//!   corners. The station's plane, square to the path, meets each rail once; with one rail the
//!   profile turns about the path to follow the line from the path to the rail and is scaled
//!   (or stretched across, [`RailFit::Stretch`]) by its length against the start's; with two,
//!   it moves with the first, turns with the line between them and scales with their distance.
//!
//! **Rails.** A rail is a kernel curve with a NURBS form (a line, an arc or circle, a
//! B-spline; a polyline is a degree-1 B-spline). The station's plane meets it at the roots of
//! the plane's signed distance along it, `(X(t) - p W(t)) . t_path` in its homogeneous form,
//! isolated on its Bezier spans by subdividing their Bernstein coefficients
//! ([`crate::query::tangent_edge`]'s `bernstein_roots`). A coefficient within its own
//! rounding (`gamma_5` of the sum of its terms' sizes, derived at `rail_crossing`) is taken
//! as 0: the rail's point there lies in the plane to its rounding; a rail's end point within
//! `1e-9 (1 + |P|)` of the plane (the point rule of docs/procedural-sweep.md section 3.1, by
//! which the path's pieces meet) reaches it there. A root at a knot is
//! one value for both spans that share it (only those exact duplicates merge; no window on
//! the parameter). A plane the rail meets other than exactly once (not at all, more than
//! once, touching it from one side, a double root the distance does not change sign over,
//! or so near it that the roots cannot be isolated at `MAX_DEPTH`) is refused, naming it.
//!
//! **Refused.** A path piece that is not a line, arc or helix: a spline path's
//! rotation-minimizing frame needs a certified enclosure (tangency-labs/solidcore#2572).
//!
//! **Platforms.** The frames use the platform's `sin_cos`, `atan2` and `acos`; they agree
//! across platforms to rounding, not bit for bit.
//!
//! Deviations: no upstream counterpart. At most `MAX_PIECES` pieces, `MAX_STATIONS`
//! stations, `MAX_TURNS` turns of an arc or helix; the root isolation's bounds are
//! `bernstein_roots`'.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::query::tangent_edge::{bernstein_roots, MAX_DEPTH, MAX_SUBDIVISIONS};
use crate::surface::procedural_sweep::path::{rodrigues, G1_TANGENTS};

const OP: &str = "sweep::stations";

/// Most path pieces.
pub const MAX_PIECES: usize = 1000;
/// Most stations asked at once.
pub const MAX_STATIONS: usize = 100_000;
/// Most turns of one arc or helix piece.
pub const MAX_TURNS: f64 = 1000.0;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// `v` turned about the unit `axis` by `angle`: the procedural sweep's rotation.
fn rotate(axis: Vec3, angle: f64, v: Vec3) -> Vec3 {
    rodrigues(v, axis, angle)
}

/// A linear map by its columns.
type Map = [Vec3; 3];

const IDENTITY: Map = [Vec3::X, Vec3::Y, Vec3::Z];

fn apply(m: &Map, v: Vec3) -> Vec3 {
    let [a, b, c] = *m;
    a * v.x + b * v.y + c * v.z
}

/// `a` after `b`.
fn compose(a: &Map, b: &Map) -> Map {
    b.map(|col| apply(a, col))
}

fn rotation(axis: Vec3, angle: f64) -> Map {
    IDENTITY.map(|e| rotate(axis, angle, e))
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn unit(v: Vec3, why: &'static str) -> Result<Vec3> {
    v.unitize().map_err(|_| invalid(why))
}

/// One piece of a sweep's path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    /// From `a` to `b`.
    Line {
        /// Its start.
        a: Point3,
        /// Its end.
        b: Point3,
    },
    /// From `start`, `angle` radians (positive) right-handed about `axis` (unit) through
    /// `centre`.
    Arc {
        /// A point of the axis.
        centre: Point3,
        /// The axis (unit).
        axis: Vec3,
        /// Its start, off the axis.
        start: Point3,
        /// The angle run (radians, positive).
        angle: f64,
    },
    /// From `start`, `angle` radians (positive) right-handed about `axis` (unit) through
    /// `centre`, rising `rise` along the axis per radian (negative: falling).
    Helix {
        /// A point of the axis.
        centre: Point3,
        /// The axis (unit).
        axis: Vec3,
        /// Its start, off the axis.
        start: Point3,
        /// The angle run (radians, positive).
        angle: f64,
        /// The rise per radian.
        rise: f64,
    },
}

impl Piece {
    /// The piece's radius vector at its start (square to the axis), its unit axis and rise.
    fn screw(&self) -> Option<(Point3, Vec3, Vec3, f64, f64)> {
        let (centre, axis, start, angle, rise) = match *self {
            Piece::Line { .. } => return None,
            Piece::Arc {
                centre,
                axis,
                start,
                angle,
            } => (centre, axis, start, angle, 0.0),
            Piece::Helix {
                centre,
                axis,
                start,
                angle,
                rise,
            } => (centre, axis, start, angle, rise),
        };
        // `check` accepts an axis within 1e-12 of unit length; it is used normalized, so the
        // rotations about it are rotations to rounding, not to that 1e-12
        let axis = axis.unitize().ok()?;
        let d = start - centre;
        let radial = d - axis * axis.dot(d);
        let foot = start - radial;
        Some((foot, axis, radial, angle, rise))
    }

    fn check(&self) -> Result<()> {
        match *self {
            Piece::Line { a, b } => {
                let ends_finite = finite(a) && finite(b);
                if !ends_finite || a.distance(b) <= 1e-9 * (1.0 + a.length()) {
                    return Err(invalid("a line piece with no length or not finite"));
                }
            }
            Piece::Arc { axis, angle, .. } | Piece::Helix { axis, angle, .. } => {
                let unit_axis = (axis.length() - 1.0).abs() <= 1e-12;
                if !unit_axis {
                    return Err(invalid("a piece's axis that is not a unit vector"));
                }
                let turns = angle / std::f64::consts::TAU;
                if !(angle > 0.0 && turns <= MAX_TURNS) {
                    return Err(invalid(
                        "a piece's angle not positive or past the most turns",
                    ));
                }
                let Some((_, _, radial, _, rise)) = self.screw() else {
                    return Err(invalid("a piece's axis"));
                };
                let size = radial.length();
                if !(size > 0.0 && size.is_finite() && rise.is_finite()) {
                    return Err(invalid("a piece's start on its axis or not finite"));
                }
            }
        }
        Ok(())
    }

    /// The piece's arc length.
    fn length(&self) -> f64 {
        if let Piece::Line { a, b } = *self {
            a.distance(b)
        } else {
            self.screw().map_or(0.0, |(_, _, radial, angle, rise)| {
                angle * radial.length().hypot(rise)
            })
        }
    }

    /// The point, unit tangent and the rotation-minimizing transport from the piece's start, at
    /// the fraction `f` of the piece's length.
    fn at(&self, f: f64) -> Result<(Point3, Vec3, Map)> {
        if let Piece::Line { a, b } = *self {
            let t = unit(b - a, "a line piece with no length")?;
            return Ok((a + (b - a) * f, t, IDENTITY));
        }
        let Some((foot, axis, radial, angle, rise)) = self.screw() else {
            return Err(invalid("a piece's axis"));
        };
        let phi = angle * f;
        let r = rotate(axis, phi, radial);
        let p = foot + r + axis * (rise * phi);
        let t = unit(axis.cross(r) + axis * rise, "a piece's tangent")?;
        let screw = rotation(axis, phi);
        let a = radial.length();
        let psi = -rise * phi / a.hypot(rise);
        let m = if rise == 0.0 {
            screw
        } else {
            compose(&rotation(t, psi), &screw)
        };
        Ok((p, t, m))
    }

    /// How far the piece's tangent turns along it (radians): 0 on a line, the angle run on an
    /// arc, `angle a / sqrt(a^2 + b^2)` on a helix (its curvature times its length).
    fn tangent_turn(&self) -> f64 {
        match self.screw() {
            None => 0.0,
            Some((_, _, radial, angle, rise)) => {
                let a = radial.length();
                angle * a / a.hypot(rise)
            }
        }
    }

    /// The unit tangents at the piece's start and end.
    fn ends(&self) -> Result<(Point3, Vec3, Point3, Vec3)> {
        let (p0, t0, _) = self.at(0.0)?;
        let (p1, t1, _) = self.at(1.0)?;
        Ok((p0, t0, p1, t1))
    }
}

/// A sweep's path: pieces end to end, each starting where the last ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// The pieces, in order.
    pub pieces: Vec<Piece>,
}

/// A corner between two pieces: the transport across it and the mitre.
#[derive(Clone, Copy)]
struct Corner {
    /// The least rotation from the arriving tangent to the leaving one.
    turn: Map,
    /// Half of it.
    half: Map,
    /// The mitre's stretch direction and factor (SolidCraft's).
    mitre: Option<(Vec3, f64)>,
}

/// A path measured: each piece's start and length along the whole, and the corner before it.
struct Measured {
    pieces: Vec<(Piece, f64, f64, Corner)>,
    total: f64,
}

impl Measured {
    /// The fraction of the path's length at which piece `j` starts: its start / the total, the
    /// one expression [`Path::joints`] and [`Path::corner_fractions`] give the caller, so a
    /// station asked at that value is compared with it exactly.
    fn fraction(&self, j: usize) -> Option<f64> {
        self.pieces
            .get(j)
            .map(|(_, start, _, _)| start / self.total)
    }
}

/// The corner where a piece ending along the unit `t0` meets one starting along the unit `t1`:
/// none where they meet G1, the procedural sweep's rule (`|t1 - t0| <= 1e-8`,
/// docs/procedural-sweep.md section 3.1, [`G1_TANGENTS`]).
fn corner(t0: Vec3, t1: Vec3) -> Result<Corner> {
    let angle = t0.cross(t1).length().atan2(t0.dot(t1));
    if (t1 - t0).length() <= G1_TANGENTS {
        return Ok(Corner {
            turn: IDENTITY,
            half: IDENTITY,
            mitre: None,
        });
    }
    // past G1 the turn's axis t0 x t1 vanishes only where the path doubles back; it is a
    // direction past the kernel's effective zero (`Vec3::unitize`, `EFF_ZERO` = 1e-12 in
    // |t0 x t1| = sin theta), and then so is the bisector t0 + t1, at least as long
    // (2 cos(theta / 2) >= sin theta). Short of that the mitre's stretch 1 / cos(theta / 2) is
    // the geometry's, however large: a cap on it is a caller's convention
    let Ok(axis) = t0.cross(t1).unitize() else {
        return Err(invalid("a path that doubles back at a corner"));
    };
    let stretch = unit(t1 - t0, "a corner's mitre")?;
    Ok(Corner {
        turn: rotation(axis, angle),
        half: rotation(axis, 0.5 * angle),
        mitre: Some((stretch, 1.0 / (0.5 * angle).cos())),
    })
}

impl Path {
    fn measure(&self) -> Result<Measured> {
        if self.pieces.is_empty() || self.pieces.len() > MAX_PIECES {
            return Err(invalid("a path of 1 to MAX_PIECES pieces"));
        }
        let mut out = Vec::new();
        let mut run = 0.0;
        let mut last: Option<(Point3, Vec3)> = None;
        for piece in &self.pieces {
            piece.check()?;
            let (p0, t0, p1, t1) = piece.ends()?;
            let c = match last {
                None => corner(t0, t0)?,
                Some((end, t_end)) => {
                    // two points are one by the procedural sweep's rule for a path's ends
                    // (docs/procedural-sweep.md section 3.1: 1e-9 (1 + |P|))
                    if end.distance(p0) > 1e-9 * (1.0 + end.length()) {
                        return Err(invalid("a path whose pieces do not meet end to start"));
                    }
                    corner(t_end, t0)?
                }
            };
            let len = piece.length();
            out.push((*piece, run, len, c));
            run += len;
            last = Some((p1, t1));
        }
        if !(run > 0.0 && run.is_finite()) {
            return Err(invalid("a path with no length"));
        }
        Ok(Measured {
            pieces: out,
            total: run,
        })
    }

    /// The fractions of the path's arc length at which its pieces meet.
    ///
    /// # Errors
    ///
    /// As [`stations`] for the path.
    pub fn joints(&self) -> Result<Vec<f64>> {
        let m = self.measure()?;
        Ok((1..m.pieces.len()).filter_map(|j| m.fraction(j)).collect())
    }

    /// The fractions of the path's arc length at its corners (joints where the pieces do not
    /// meet G1), in order: corner `j`'s mitre is asked as [`Stop::Corner`]`(j)`.
    ///
    /// # Errors
    ///
    /// As [`stations`] for the path.
    pub fn corner_fractions(&self) -> Result<Vec<f64>> {
        let m = self.measure()?;
        Ok((1..m.pieces.len())
            .filter(|&j| {
                m.pieces
                    .get(j)
                    .is_some_and(|(_, _, _, c)| c.mitre.is_some())
            })
            .filter_map(|j| m.fraction(j))
            .collect())
    }
}

impl Path {
    /// Whether the path ends where it starts (within `1e-9 (1 + |p|)`, as its pieces must meet).
    ///
    /// # Errors
    ///
    /// As [`stations`] for the path.
    pub fn is_closed(&self) -> Result<bool> {
        self.measure()?;
        let (Some(first), Some(last)) = (self.pieces.first(), self.pieces.last()) else {
            return Err(invalid("a path without pieces"));
        };
        let (p0, _, _, _) = first.ends()?;
        let (_, _, p1, _) = last.ends()?;
        Ok(p0.distance(p1) <= 1e-9 * (1.0 + p0.length()))
    }
}

/// How the profile turns as it follows the path.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Orientation {
    /// Square to the path, carried by its rotation-minimizing frame.
    #[default]
    Perpendicular,
    /// As drawn: the profile only moves along the path.
    Parallel,
    /// Square to the path, its turn about the path fixed by a direction: the direction's
    /// projection square to the tangent.
    Direction(Vec3),
}

/// How guide rails size the profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RailFit {
    /// Evenly, by the distance across.
    #[default]
    Scale,
    /// Only across to the rail (between the rails).
    Stretch,
}

/// A sweep's laws (SolidCraft's conventions; module docs). The default is a plain sweep.
#[derive(Debug, Clone)]
pub struct Laws<'a> {
    /// How the profile turns.
    pub orientation: Orientation,
    /// The twist over the path (radians).
    pub twist: f64,
    /// The profile's scale at the end.
    pub end_scale: f64,
    /// Up to two guide rails.
    pub rails: Vec<&'a Curve>,
    /// How the rails size the profile (ignored without rails).
    pub rail_fit: RailFit,
    /// With [`Orientation::Parallel`], the least angle (radians, `[0, pi / 2)`) between the
    /// path's tangent and the profile's plane at a station: the caller's limit (SolidCraft's is
    /// 5 degrees). At 0, only a path that does not cross the plane is refused.
    pub min_crossing: f64,
    /// With [`Orientation::Direction`], the least sine (`[0, 1)`) of the angle between the
    /// path's tangent and the direction at a station: the caller's limit (SolidCraft's is
    /// `1e-3`). At 0, only a path along the direction is refused.
    pub min_across: f64,
}

impl Default for Laws<'_> {
    fn default() -> Self {
        Laws {
            orientation: Orientation::Perpendicular,
            twist: 0.0,
            end_scale: 1.0,
            rails: Vec::new(),
            rail_fit: RailFit::Scale,
            min_crossing: 0.0,
            min_across: 0.0,
        }
    }
}

impl Laws<'_> {
    fn check(&self) -> Result<()> {
        // the geometric limits only: the caller's conventions (SolidCraft's four turns of
        // twist, its 0.01 to 100 end scale) are the caller's to check
        if !self.twist.is_finite() {
            return Err(invalid("a twist not finite"));
        }
        if !(self.end_scale.is_finite() && self.end_scale > 0.0) {
            return Err(invalid("an end scale not finite and positive"));
        }
        if let Orientation::Direction(d) = self.orientation {
            if !(finite(d) && d.length_squared().is_finite()) {
                return Err(invalid("an orientation direction not finite"));
            }
        }
        let crossing = (0.0..std::f64::consts::FRAC_PI_2).contains(&self.min_crossing);
        if !(crossing && (0.0..1.0).contains(&self.min_across)) {
            return Err(invalid(
                "a least crossing angle outside [0, pi / 2) or least sine outside [0, 1)",
            ));
        }
        if self.rails.len() > 2 {
            return Err(invalid("more than two guide rails"));
        }
        let defaults = self.orientation == Orientation::Perpendicular
            && self.twist == 0.0
            && self.end_scale == 1.0;
        if !self.rails.is_empty() && !defaults {
            return Err(invalid(
                "guide rails with an orientation, twist or end scale: the rails set them",
            ));
        }
        Ok(())
    }
}

/// Where a station is asked: at a fraction of the path's arc length, or at a corner's mitre.
/// A corner is named, never found by comparing fractions: a fraction at a corner places the
/// station by its piece (the arriving piece's end below the corner's fraction, the leaving
/// piece's start from it), never at the mitre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stop {
    /// At this fraction of the path's arc length, in `[0, 1]`.
    At(f64),
    /// At the mitre of the path's corner `j` (in order: [`Path::corner_fractions`]).
    Corner(usize),
}

/// A station: its point `at` and the linear map `m` placing the profile there (module docs).
/// Without rails `at` is a point of the path; with rails it is where the map's origin goes,
/// off the path (`anchor - m (a_0 - s_0)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Station {
    /// The point.
    pub at: Point3,
    /// The map's columns, the images of the axes.
    pub m: [Vec3; 3],
}

impl Station {
    /// The translation of the station's affine map `q -> m q + t` with `first` the path's first
    /// station: `t = at - m at_0`, so that `q` at the first station lands at
    /// `at + m (q - at_0)`.
    #[must_use]
    pub fn translation(&self, first: &Station) -> Point3 {
        self.at - apply(&self.m, first.at)
    }
}

/// The smallest number of stations [`fractions`] gives (SolidCraft's).
pub const MIN_SKIN_STATIONS: usize = 16;
/// The largest number of stations [`fractions`] gives (SolidCraft's).
pub const MAX_SKIN_STATIONS: usize = 2048;

/// Where a skin through a sweep's stations puts them: SolidCraft's fit convention, carried
/// over, not derived here. One station per degree of the path's tangent turn plus the twist
/// (corners' turns included), at least as many as a rail's control points less one, between
/// [`MIN_SKIN_STATIONS`] and [`MAX_SKIN_STATIONS`], evenly in arc length, with every joint's
/// fraction ([`Path::joints`]) included exactly. SolidCraft measured the cubic skin's error
/// near its ends falling as the step squared (a square of reach 7.1 twisted a quarter turn
/// over 40 is 2e-7 off in volume at 1.4 degree steps); nothing here bounds the skin's distance
/// from the law surface between the stations (tangency-labs/solidcore#3193).
///
/// # Errors
///
/// As [`stations`] for the path and laws; [`SolidError::InvalidInput`] for a rail with no
/// NURBS form.
pub fn fractions(path: &Path, laws: &Laws<'_>) -> Result<Vec<f64>> {
    laws.check()?;
    let m = path.measure()?;
    let mut turn = laws.twist.abs();
    for (i, (piece, _, _, c)) in m.pieces.iter().enumerate() {
        turn += piece.tangent_turn();
        if i > 0 {
            if let Some((_, s)) = c.mitre {
                // the corner's angle back from its mitre factor 1 / cos(theta / 2)
                turn += 2.0 * (1.0 / s).acos();
            }
        }
    }
    let mut rail_points = 0;
    for rail in &laws.rails {
        let base = rail
            .b_spline_base()
            .ok_or_else(|| unsupported("a guide rail with no NURBS form"))?;
        rail_points = rail_points.max(base.control_polygon()?.0.len());
    }
    let step = std::f64::consts::PI / 180.0;
    let by_turn = (turn / step).ceil();
    let by_turn = if by_turn.is_finite() && by_turn >= 0.0 {
        by_turn as usize
    } else {
        MAX_SKIN_STATIONS
    };
    let n = by_turn
        .max(rail_points.saturating_sub(1))
        .clamp(MIN_SKIN_STATIONS, MAX_SKIN_STATIONS);
    let mut out: Vec<f64> = (0..=n).map(|j| j as f64 / n as f64).collect();
    out.extend((1..m.pieces.len()).filter_map(|j| m.fraction(j)));
    out.sort_by(f64::total_cmp);
    out.dedup();
    Ok(out)
}

/// Where along the measured path a stop is: the point, unit tangent, transport from the
/// path's start, and the corner for a [`Stop::Corner`].
fn locate(m: &Measured, stop: Stop) -> Result<(Point3, Vec3, Map, Option<Corner>)> {
    // a corner is named by its index: which piece a fraction lies on is decided by comparing
    // it with the pieces' start fractions, never a mitre
    let (u, corner) = match stop {
        Stop::At(u) => (u, None),
        Stop::Corner(j) => {
            let i = corner_piece(m, j)?;
            (m.fraction(i).unwrap_or(0.0), Some(i))
        }
    };
    let mut transport = IDENTITY;
    let count = m.pieces.len();
    for (i, (piece, start, len, c)) in m.pieces.iter().enumerate() {
        let before = compose(&c.turn, &transport);
        if corner == Some(i) {
            let (p, t0_piece, _) = piece.at(0.0)?;
            let half = compose(&c.half, &transport);
            let t_prev = apply(&transpose_rotation(&c.turn), t0_piece);
            let bisector = unit(t_prev + t0_piece, "a corner's bisector")?;
            return Ok((p, bisector, half, Some(*c)));
        }
        let last = i + 1 == count;
        let before_next = m.fraction(i + 1).is_some_and(|next| u < next);
        if before_next || last {
            let s = u * m.total;
            let f = if *len > 0.0 {
                ((s - start) / len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (p, t, local) = piece.at(f)?;
            return Ok((p, t, compose(&local, &before), None));
        }
        let (_, _, whole) = piece.at(1.0)?;
        transport = compose(&whole, &before);
    }
    Err(invalid("a path without pieces"))
}

/// The fraction of the path's arc length a stop is at (a corner's: its start fraction).
fn fraction_of(m: &Measured, stop: Stop) -> Result<f64> {
    match stop {
        Stop::At(u) => Ok(u),
        Stop::Corner(j) => m
            .fraction(corner_piece(m, j)?)
            .ok_or_else(|| invalid("a corner the path does not have")),
    }
}

/// The index of the piece that starts at the path's corner `j` (corners in order).
fn corner_piece(m: &Measured, j: usize) -> Result<usize> {
    (1..m.pieces.len())
        .filter(|&i| {
            m.pieces
                .get(i)
                .is_some_and(|(_, _, _, c)| c.mitre.is_some())
        })
        .nth(j)
        .ok_or_else(|| invalid("a corner the path does not have"))
}

/// The inverse of a rotation: its transpose.
fn transpose_rotation(r: &Map) -> Map {
    let [a, b, c] = *r;
    [
        Vec3::new(a.x, b.x, c.x),
        Vec3::new(a.y, b.y, c.y),
        Vec3::new(a.z, b.z, c.z),
    ]
}

/// Where `rail` crosses the plane through `p` square to the unit `t`: the root in its
/// parameter of its signed distance from the plane, taken span by span in Bernstein form.
/// Refused unless exactly one root, at which the distance changes sign over the rail (a touch
/// is not a crossing), and refused where the roots could not be isolated.
fn rail_crossing(k: &Kernel, rail: &Curve, p: Point3, t: Vec3) -> Result<Point3> {
    let base = rail
        .b_spline_base()
        .ok_or_else(|| unsupported("a guide rail with no NURBS form"))?;
    let r = base.degree()?;
    let (unique, mult) = base.knots(None)?;
    let mut knots = Vec::new();
    for (&x, &n) in unique.iter().zip(&mult) {
        knots.extend(std::iter::repeat_n(x, n));
    }
    let (points, weights) = base.control_polygon()?;
    if !weights.is_empty() && weights.len() != points.len() {
        return Err(invalid("a rail's weights do not match its control points"));
    }
    let mut pw = Vec::new();
    for (i, q) in points.iter().enumerate() {
        if weights.is_empty() {
            pw.push(CPoint::new(q.x, q.y, q.z, NOW));
            continue;
        }
        let Some(&w) = weights.get(i) else {
            return Err(invalid("a rail's weights do not match its control points"));
        };
        let positive = w > 0.0;
        if !positive {
            return Err(invalid("a rail's weight that is not positive"));
        }
        pw.push(CPoint::new(q.x * w, q.y * w, q.z * w, w));
    }
    let nurb = NurbCurve::new(pw, r, knots)?;
    let mut roots: Vec<f64> = Vec::new();
    // the signed distances at the rail's two ends (coefficients within their rounding zeroed)
    let mut ends: Option<(f64, f64)> = None;
    let spans = nurb.decompose_bez()?;
    let last_span = spans.len().saturating_sub(1);
    for (k_span, span) in spans.into_iter().enumerate() {
        let (Some(&a), Some(&b)) = (span.knots().first(), span.knots().last()) else {
            continue;
        };
        let n_cp = span.cpts().len();
        let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
        // d_i = sum_j (x_ij - p_j w_i) t_j: per term a product, a difference and a product,
        // then two sums, so |fl(d_i) - d_i| <= gamma_5 S_i, S_i = sum_j (|x_ij| + |p_j| w_i) |t_j|.
        // S_i is itself computed in 5 roundings and the product with gamma_5 in one more, so
        // the floor below is at least the bound. A coefficient within it is indistinguishable
        // from 0 and is taken as 0: the rail's point there lies in the plane to its own rounding.
        let c: Vec<f64> = span
            .cpts()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let d =
                    (c.x - p.x * w(c)) * t.x + (c.y - p.y * w(c)) * t.y + (c.z - p.z * w(c)) * t.z;
                let size = (c.x.abs() + p.x.abs() * w(c)) * t.x.abs()
                    + (c.y.abs() + p.y.abs() * w(c)) * t.y.abs()
                    + (c.z.abs() + p.z.abs() * w(c)) * t.z.abs();
                let floor = gamma(5) * size * (1.0 + gamma(6));
                // the rail's own ends (its first and last control points, the clamped curve's
                // end points) lie in the plane where they are one point with their foot on it,
                // by the rule the path's pieces meet by (1e-9 (1 + |P|), docs/procedural-sweep.md
                // section 3.1): a rail made to start on the path's first station, whose end is
                // off it by its construction's rounding (an arc's cos 90 degrees), reaches it
                let rail_end = (k_span == 0 && i == 0) || (k_span == last_span && i + 1 == n_cp);
                let wc = w(c);
                let at = Vec3::new(c.x / wc, c.y / wc, c.z / wc);
                let touches = rail_end && (d / wc).abs() <= 1e-9 * (1.0 + at.length());
                if d.abs() <= floor || touches {
                    0.0
                } else {
                    d
                }
            })
            .collect();
        if c.iter().all(|x| *x == 0.0) {
            return Err(invalid("a guide rail lying in a station's plane"));
        }
        if let (Some(&first), Some(&last)) = (c.first(), c.last()) {
            ends = Some((ends.map_or(first, |(s, _)| s), last));
        }
        let mut found = Vec::new();
        let mut work = MAX_SUBDIVISIONS;
        if !bernstein_roots(&c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut found) {
            return Err(invalid(
                "a guide rail running too near a station's plane to tell where it crosses",
            ));
        }
        for s in found {
            // a span's ends are its knots exactly, so a root at a knot is one value for both
            // spans that share it; only those exact duplicates merge
            let x = if s == 0.0 {
                a
            } else if s == 1.0 {
                b
            } else {
                a + (b - a) * s
            };
            if !roots.contains(&x) {
                roots.push(x);
            }
        }
    }
    match roots.as_slice() {
        [x] => {
            // one root, but a crossing only where the distance changes sign over the rail: a
            // rail touching the plane from one side (a root of even multiplicity) does not
            let touches = ends.is_some_and(|(s, e)| (s > 0.0 && e > 0.0) || (s < 0.0 && e < 0.0));
            if touches {
                return Err(invalid(
                    "a guide rail touching a station's plane without crossing it",
                ));
            }
            rail.evaluate_point(k, *x)
        }
        [] => Err(invalid(
            "a guide rail that does not reach a station's plane: it must run the path's length",
        )),
        _ => Err(invalid(
            "a guide rail that crosses a station's plane more than once",
        )),
    }
}

/// How guide rails turn and size a section (SolidCraft's `rail_frame`): `along` (unit) turns
/// onto `along_to`, `across` (unit, square to `along`) onto `across_to` sized by `k`, and the
/// third way follows, sized by `k` ([`RailFit::Scale`]) or kept ([`RailFit::Stretch`]).
fn rail_frame(
    along: Vec3,
    across: Vec3,
    along_to: Vec3,
    across_to: Vec3,
    k: f64,
    fit: RailFit,
) -> Map {
    let k_other = match fit {
        RailFit::Scale => k,
        RailFit::Stretch => 1.0,
    };
    let (w0, w1) = (along.cross(across), along_to.cross(across_to));
    let map = |v: Vec3| {
        along_to * v.dot(along) + across_to * (k * v.dot(across)) + w1 * (k_other * v.dot(w0))
    };
    IDENTITY.map(map)
}

/// The stations of a sweep along `path` at the fractions `at` of its arc length, placing a
/// profile whose plane's normal is `profile_normal`, by `laws` (module docs).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a path or laws out of range (pieces not meeting, a corner
/// doubling back, a twist not finite, an end scale not finite and positive, a parallel
/// profile the path crosses at no more than `min_crossing`, a path along the orientation
/// direction (within `min_across`), a rail not crossing a station's plane exactly once (one
/// touching it, lying in it, or too near it to tell), rails meeting each other or the path),
/// fractions outside `[0, 1]`, a corner the path does not have, or more than `MAX_STATIONS`
/// stops; [`SolidError::Unsupported`] for rails along a path with corners and a rail with no
/// NURBS form; those of the rails' evaluations.
pub fn stations(
    k: &Kernel,
    path: &Path,
    profile_normal: Vec3,
    laws: &Laws<'_>,
    at: &[Stop],
) -> Result<Vec<Station>> {
    laws.check()?;
    if at.is_empty() || at.len() > MAX_STATIONS {
        return Err(invalid("1 to MAX_STATIONS stations"));
    }
    if at
        .iter()
        .any(|s| matches!(s, Stop::At(u) if !(0.0..=1.0).contains(u)))
    {
        return Err(invalid("a station's fraction outside [0, 1]"));
    }
    let measured = path.measure()?;
    let n = unit(profile_normal, "a profile normal of no length")?;
    let (p0, t0, _, _) = locate(&measured, Stop::At(0.0))?;
    let axis = if n.dot(t0) < 0.0 { n * -1.0 } else { n };
    if !laws.rails.is_empty() {
        return railed(k, &measured, laws, at, p0, t0);
    }
    let mut out = Vec::new();
    for &stop in at {
        let u = fraction_of(&measured, stop)?;
        let (p, t, transport, at_corner) = locate(&measured, stop)?;
        let turn = match laws.orientation {
            Orientation::Perpendicular => transport,
            Orientation::Parallel => {
                // the angle between the tangent and the plane is asin(t . axis)
                if t.dot(axis) <= laws.min_crossing.sin() {
                    return Err(invalid(
                        "a parallel profile the path does not cross by the least angle asked",
                    ));
                }
                IDENTITY
            }
            Orientation::Direction(d) => {
                let frame = |t: Vec3| -> Result<Map> {
                    let d = unit(d, "an orientation direction of no length")?;
                    // |across| is the sine of the angle between the tangent and the direction
                    let across = d - t * t.dot(d);
                    if across.length() <= laws.min_across {
                        return Err(invalid("a path running along the orientation direction"));
                    }
                    let r = unit(across, "a path running along the orientation direction")?;
                    Ok([t, r, t.cross(r)])
                };
                let (f0, fi) = (frame(t0)?, frame(t)?);
                compose(&fi, &transpose_rotation(&f0))
            }
        };
        let (sin, cos) = (laws.twist * u).sin_cos();
        let scale = 1.0 + (laws.end_scale - 1.0) * u;
        let shape = |v: Vec3| {
            let along = axis * v.dot(axis);
            let r = (v - along) * scale;
            along + r * cos + axis.cross(r) * sin
        };
        let mut m = IDENTITY.map(|e| apply(&turn, shape(e)));
        if laws.orientation != Orientation::Parallel {
            if let Some(Corner {
                mitre: Some((b, s)),
                ..
            }) = at_corner
            {
                for c in &mut m {
                    *c += b * ((s - 1.0) * c.dot(b));
                }
            }
        }
        out.push(Station { at: p, m });
    }
    Ok(out)
}

/// The stations shaped by guide rails (module docs).
fn railed(
    k: &Kernel,
    measured: &Measured,
    laws: &Laws<'_>,
    at: &[Stop],
    s0: Point3,
    t0: Vec3,
) -> Result<Vec<Station>> {
    if measured.pieces.iter().any(|(_, _, _, c)| c.mitre.is_some()) {
        return Err(unsupported("guide rails along a path with corners"));
    }
    let near = |d: Vec3| d.length() <= 1e-9 * (1.0 + measured.total);
    let across_at = |stop: Stop| -> Result<(Point3, Vec3, Point3, Vec3)> {
        let (p, t, _, _) = locate(measured, stop)?;
        let mut hits = Vec::new();
        for rail in &laws.rails {
            hits.push(rail_crossing(k, rail, p, t)?);
        }
        let (anchor, v) = match hits.as_slice() {
            [r] => (p, *r - p),
            [a, b] => (*a, *b - *a),
            _ => return Err(invalid("one or two guide rails")),
        };
        Ok((p, t, anchor, v - t * v.dot(t)))
    };
    let (_, _, a0, d0) = across_at(Stop::At(0.0))?;
    if near(d0) {
        return Err(invalid(
            "a guide rail starting on the path, or two starting at one point",
        ));
    }
    let u0 = unit(d0, "a guide rail's start")?;
    let mut out = Vec::new();
    for &stop in at {
        let (_, t, anchor, d) = across_at(stop)?;
        if near(d) {
            return Err(invalid(
                "a guide rail meeting the path, or two meeting: the profile shrinks to nothing",
            ));
        }
        let ud = unit(d, "a guide rail's direction across")?;
        let m = rail_frame(t0, u0, t, ud, d.length() / d0.length(), laws.rail_fit);
        // the anchor's start lands on the anchor
        out.push(Station {
            at: anchor - apply(&m, a0 - s0),
            m,
        });
    }
    Ok(out)
}
