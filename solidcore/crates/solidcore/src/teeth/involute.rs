//! Involute splines: the teeth of the shaft and the tooth spaces of its hub.
//!
//! An external tooth ([`External`]) is generated: it is what a straight-sided rack with a rounded
//! tip leaves when it rolls on the pitch circle. Its flank is the involute, its root fillet is the
//! trochoid the tip round cuts, and where the rack reaches too deep for the count of teeth the
//! fillet cuts into the flank (undercut). Nothing of that is approximated: each point of the
//! outline is the point of the rack that touches the tooth at one instant of the rolling, found
//! from the rule that the common normal passes through the point of rolling contact.
//!
//! An internal tooth space ([`Internal`]) is formed: the same involute flanks, with circular rounds
//! at the root corners, as a broach leaves them.
//!
//! What is decided about a tooth is proved, not sampled: the root fillets are proved apart (each
//! stays strictly between the middle of its tooth and the middle of the space), and every chord of
//! a flank or fillet is proved within the tolerance of the curve (`outline::sampled`), each in
//! rounded intervals. A point of a curve is computed with the platform's sine, cosine, arc tangent
//! and `hypot`, whose precision nothing promises, and is then measured against an enclosure of the
//! true point made without them (`teeth::bound`): its distance from the curve is proved, not
//! assumed.
//!
//! Lengths are millimetres. Heights of the tooth are given in modules, as usual for involute
//! splines.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::blend::fillet_section::interval::Ivl;
use crate::error::teeth::{TeethFailure, Why, FILLET_SEPARATION};
use crate::geom::certified::next_up;
use crate::teeth::bound::{
    atan_over, exact, half_width, magnitude, neg, off, pi, rotated_ivl, sqr, taylor,
    taylor_from_end, Bend,
};
use crate::teeth::mesh::Material;
use crate::teeth::outline::{
    checked_tolerance, curve_tolerance, on_circle, sampled, Outline, Piece, Point, Role,
};
use crate::teeth::pair::{Motion, Pair, Part};
use crate::teeth::{in_range, zero_up_to, MAX_LENGTH, MAX_TEETH};
use crate::SolidError;

/// The involute function `tan a - a` of an angle in radians.
pub fn inv(angle: f64) -> f64 {
    angle.tan() - angle
}

/// What module, count of teeth and pressure angle fix.
#[derive(Clone, Copy, Debug)]
struct Base {
    m: f64,
    z: f64,
    sin: f64,
    cos: f64,
    /// Radius of the pitch circle.
    r: f64,
    /// Radius of the base circle.
    rb: f64,
    /// The pitch along the pitch circle.
    pitch: f64,
    /// The involute function of the pressure angle.
    inv: f64,
}

fn base(module: f64, teeth: u32, pressure_angle: f64) -> Result<Base, TeethFailure> {
    let m = in_range("module", module, 0.0, 1000.0)?;
    if !(3..=MAX_TEETH).contains(&teeth) {
        return Err(TeethFailure::OutOfRange {
            what: "teeth",
            value: f64::from(teeth),
            above: 2.0,
            up_to: f64::from(MAX_TEETH),
        });
    }
    let alpha = in_range("pressure angle", pressure_angle, 1.0, 50.0)?.to_radians();
    let (sin, cos) = alpha.sin_cos();
    let z = f64::from(teeth);
    let r = m * z / 2.0;
    Ok(Base {
        m,
        z,
        sin,
        cos,
        r,
        rb: r * cos,
        pitch: PI * m,
        inv: inv(alpha),
    })
}

impl Base {
    /// How far an involute has turned from its start on the base circle when it reaches `radius`.
    fn turned(&self, radius: f64) -> Option<f64> {
        if radius.is_finite() && radius >= self.rb {
            let roll = (radius * radius - self.rb * self.rb).sqrt() / self.rb;
            Some(roll - roll.atan())
        } else {
            None
        }
    }
}

/// The teeth of a splined shaft, as a rack-shaped cutting tool cuts them.
///
/// The rack is the usual straight-sided one: its flanks lean by the pressure angle, its teeth are
/// as thick as its spaces on its reference line, and the corners of its teeth are rounded with
/// `tool_tip_radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct External {
    /// The module in millimetres: the pitch diameter divided by the count of teeth.
    pub module: f64,
    /// How many teeth, 3 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The pressure angle in degrees, above 1 and at most 50.
    pub pressure_angle: f64,
    /// How far the tooth reaches above the pitch circle when there is no shift, in modules.
    pub addendum: f64,
    /// How far the root lies below the pitch circle when there is no shift, in modules.
    pub dedendum: f64,
    /// The profile shift in modules: the rack is drawn back from the axis by this much, which
    /// thickens the tooth and moves tip and root outward.
    pub shift: f64,
    /// The radius of the round at the corners of the rack's teeth, in modules. It makes the root
    /// fillet.
    pub tool_tip_radius: f64,
    /// Millimetres taken from the tooth's thickness on the pitch circle, for play in the hub
    /// (negative thickens).
    pub thinning: f64,
}

/// An external tooth worked out: the rack that cuts it, and where its pieces begin and end.
#[derive(Clone, Copy, Debug)]
struct Cut {
    base: Base,
    /// Thickness of the tooth on the pitch circle.
    s: f64,
    /// Radius of the rack's tip round, and its centre in the rack: along and across the line of
    /// rolling.
    rho: f64,
    uc: f64,
    vc: f64,
    ra: f64,
    rf: f64,
    /// The angle of rolling at which the fillet reaches the root circle.
    phi_root: f64,
    /// The angle of rolling at which the fillet meets the flank, and the height in the rack of that
    /// point.
    phi_join: f64,
    v_join: f64,
    /// The height in the rack of the flank's point on the tip circle.
    v_tip: f64,
    undercut: bool,
    /// Half the angle the top of the tooth spans.
    tip_half_angle: f64,
}

const NO_JOIN: TeethFailure = TeethFailure::Invalid(Why::FilletMissesFlank);

/// The most stretches one test of [`Fillet::separation`] looks at before it gives up.
const MAX_SEPARATION_PIECES: usize = 4096;

/// The upper flank of an external tooth: the involute that the rack's straight flank cuts, by the
/// height `v` in the rack of its cutting point.
#[derive(Clone, Copy, Debug)]
pub(super) struct Flank {
    /// The radius of the pitch circle.
    pub(super) r: f64,
    /// The thickness of the tooth on the pitch circle.
    pub(super) s: f64,
    /// The sine and cosine of the pressure angle.
    pub(super) sin: f64,
    /// See `sin`.
    pub(super) cos: f64,
    /// The height in the rack where the flank begins (where it meets the fillet).
    pub(super) from: f64,
    /// The height in the rack where the flank ends (on the tip circle).
    pub(super) to: f64,
}

impl Flank {
    /// The point of the upper flank cut by the point of the rack's flank at height `v` above the
    /// line of rolling.
    ///
    /// That point of the rack touches the tooth when its normal passes through the point of rolling
    /// contact, which is at the angle `phi` below.
    pub(super) fn point(&self, v: f64) -> Point {
        let phi = (self.s / 2.0 - v / (self.sin * self.cos)) / self.r;
        let (sn, cs) = phi.sin_cos();
        let along = v * self.cos / self.sin;
        [
            (self.r + v) * cs - along * sn,
            (self.r + v) * sn + along * cs,
        ]
    }

    /// What the flank does over the heights `[lo, hi]`, for `outline::sampled`.
    ///
    /// The point is `R(phi(v)) l(v)` with `l(v) = (r + v, v cot)`, `phi(v) = (s/2 - v/(sin cos))/r`
    /// and `k = 1/(r sin cos)`, so `P' = R(phi) (1 + k v cot, cot - k (r + v))` and
    /// `P'' = R(phi) (2 k cot - k^2 (r + v), -2 k - k^2 v cot)`, each enclosed in rounded
    /// intervals over the heights (the doubles `r`, `s`, `sin`, `cos` are the curve's definition);
    /// the size of `P''` is that of the unrotated vector.
    pub(super) fn bend(&self, lo: f64, hi: f64) -> Bend {
        let (r, sin, cos) = (exact(self.r), exact(self.sin), exact(self.cos));
        let (one, two) = (exact(1.0), exact(2.0));
        let v = Ivl::new(lo, hi);
        let sc = sin * cos;
        let k = one / (r * sc);
        let cot = cos / sin;
        let phi = (exact(self.s) * exact(0.5) - v / sc) / r;
        let rv = r + v;
        let first = [one + k * v * cot, cot - k * rv];
        let second = [two * k * cot - sqr(k) * rv, neg(two * k) - sqr(k) * v * cot];
        Bend {
            first: rotated_ivl(phi, first),
            second: rotated_ivl(phi, second),
            second_size: magnitude(second),
        }
    }

    /// The true point of the flank at the height `v`, enclosed: `R(phi(v)) (r + v, v cos / sin)`
    /// in rounded intervals, the turn's sine and cosine from their series (`teeth::bound`).
    fn held(&self, v: f64) -> [Ivl; 2] {
        let (r, sin, cos, v) = (exact(self.r), exact(self.sin), exact(self.cos), exact(v));
        let phi = (exact(self.s) * exact(0.5) - v / (sin * cos)) / r;
        rotated_ivl(phi, [r + v, v * cos / sin])
    }

    /// A proved bound of the distance of `p`, the point computed for the height `v`
    /// ([`Flank::point`], with the platform's sine and cosine), from the flank's true point there.
    pub(super) fn off(&self, v: f64, p: Point) -> f64 {
        off(p, self.held(v))
    }
}

/// Which side of the wedge between the middle of a tooth and the middle of the next space a test
/// of [`Fillet::separation`] keeps the upper root fillet on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wedge {
    /// Above the middle of the tooth: every angle above 0, else the two fillets of one tooth
    /// meet. The test's function is the height `y` of the fillet's point.
    AboveMiddle,
    /// Below the middle of the space: every angle below `pi / z`, else the fillets of neighbouring
    /// teeth cross. The test's function is `sin(pi/z) x - cos(pi/z) y`.
    BelowSpace,
}

/// The upper root fillet of an external tooth: what defines it and the stretch of rolling it
/// spans.
///
/// The doubles `r`, `uc`, `vc` and `rho` are its definition: with `w = r phi - uc`,
/// `len = sqrt(w^2 + vc^2)` and `lam = 1 + rho/len`, its point at the angle of rolling `phi` is
/// `R(phi) (r + vc lam, -w lam)`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Fillet {
    /// The radius of the pitch circle.
    pub(super) r: f64,
    /// The centre of the rack's tip round, along the line of rolling.
    pub(super) uc: f64,
    /// The centre of the rack's tip round, across the line of rolling (below it: negative).
    pub(super) vc: f64,
    /// The radius of the rack's tip round.
    pub(super) rho: f64,
    /// The count of teeth, as a number.
    pub(super) z: f64,
    /// The angle of rolling at which the fillet reaches the root circle.
    pub(super) phi_root: f64,
    /// The angle of rolling at which the fillet meets the flank.
    pub(super) phi_join: f64,
}

impl Fillet {
    /// The point of the upper root fillet cut by the rack's tip round at the angle of rolling
    /// `phi`: the point of the round on the line from the point of rolling contact through the
    /// round's centre, on the far side.
    pub(super) fn point(&self, phi: f64) -> Point {
        let (du, dv) = (self.r * phi - self.uc, -self.vc);
        let len = du.hypot(dv);
        let (qu, qv) = (self.uc - self.rho * du / len, self.vc - self.rho * dv / len);
        let (sn, cs) = phi.sin_cos();
        let along = qu - self.r * phi;
        [
            (self.r + qv) * cs - along * sn,
            (self.r + qv) * sn + along * cs,
        ]
    }

    /// The fillet's point before its turn by `phi`, and the vectors that its first and second
    /// derivatives turn, enclosed for every angle of rolling of `phi`: `q = (a, b)`,
    /// `p1 = q' + J q` and `p2 = q'' + 2 J q' - q`, so that `P = R(phi) q`, `P' = R(phi) p1` and
    /// `P'' = R(phi) p2` (`J` the quarter turn). With `lam' = -rho r w / len^3` and
    /// `lam'' = -rho r^2 (vc^2 - 2 w^2) / len^5`: `q' = (vc lam', -(r lam + w lam'))` and
    /// `q'' = (vc lam'', -(2 r lam' + w lam''))`. Every operation is a rounded interval one.
    fn local(&self, phi: Ivl) -> [[Ivl; 2]; 3] {
        let (r, vc, rho) = (exact(self.r), exact(self.vc), exact(self.rho));
        let (one, two) = (exact(1.0), exact(2.0));
        let w = r * phi - exact(self.uc);
        let len2 = sqr(w) + sqr(vc);
        let len = len2.sqrt();
        let lam = one + rho / len;
        let lam1 = neg(rho * r * w / (len2 * len));
        let lam2 = neg(rho * sqr(r) * (sqr(vc) - two * sqr(w)) / (len2 * len2 * len));
        let (a, b) = (r + vc * lam, neg(w * lam));
        let (q1x, q1y) = (vc * lam1, neg(r * lam + w * lam1));
        let (q2x, q2y) = (vc * lam2, neg(two * r * lam1 + w * lam2));
        [
            [a, b],
            [q1x - b, q1y + a],
            [q2x - two * q1y - a, q2y + two * q1x - b],
        ]
    }

    /// The test's function of a vector `v` turned by `angle`: the height of `R(angle) v` for
    /// [`Wedge::AboveMiddle`]; for [`Wedge::BelowSpace`], minus the height of
    /// `R(angle - pi/z) v`, which is `sin(pi/z) x - cos(pi/z) y` of `R(angle) v`.
    fn signed_height(&self, wedge: Wedge, angle: Ivl, v: [Ivl; 2]) -> Ivl {
        match wedge {
            Wedge::AboveMiddle => {
                let [_, y] = rotated_ivl(angle, v);
                y
            }
            Wedge::BelowSpace => {
                let beta = pi() / exact(self.z);
                let [_, y] = rotated_ivl(angle + neg(beta), v);
                neg(y)
            }
        }
    }

    /// The test's function at the angle of rolling `phi`, enclosed (no Taylor form).
    pub(super) fn value(&self, wedge: Wedge, phi: f64) -> Ivl {
        let at = exact(phi);
        let [q, _, _] = self.local(at);
        self.signed_height(wedge, at, q)
    }

    /// The test's function and its derivative at `phi`, enclosed.
    fn value_and_slope(&self, wedge: Wedge, phi: f64) -> (Ivl, Ivl) {
        let at = exact(phi);
        let [q, first, _] = self.local(at);
        (
            self.signed_height(wedge, at, q),
            self.signed_height(wedge, at, first),
        )
    }

    /// An enclosure of the test's function over every angle of rolling of `[p0, p1]`
    /// (`p0 <= p1`): the second-order Taylor form about the middle, with the value and slope at
    /// the middle and the second derivative enclosed over the whole stretch. Where the stretch
    /// starts at the fillet's first end (`at_start`) or ends at its last (`at_end`), the same form
    /// anchored at that end is formed too, and the enclosure with the larger lower end is
    /// returned (each is sound): it settles a stretch whose end is where the margin is smallest.
    pub(super) fn enclosure(
        &self,
        wedge: Wedge,
        p0: f64,
        p1: f64,
        at_start: bool,
        at_end: bool,
    ) -> Ivl {
        let (m, r) = half_width(p0, p1);
        let (value, slope) = self.value_and_slope(wedge, m);
        let stretch = Ivl::new(p0, p1);
        let [_, _, second] = self.local(stretch);
        let curvature = self.signed_height(wedge, stretch, second);
        let mut best = taylor(value, slope, curvature, r);
        let h = next_up(p1 - p0);
        if at_start {
            let (value, slope) = self.value_and_slope(wedge, p0);
            best = higher(best, taylor_from_end(value, slope, curvature, h, true));
        }
        if at_end {
            let (value, slope) = self.value_and_slope(wedge, p1);
            best = higher(best, taylor_from_end(value, slope, curvature, h, false));
        }
        best
    }

    /// Proves that every point of the upper root fillet over its whole stretch of rolling,
    /// `[phi_root, phi_join]` in either order, lies strictly inside the wedge `0 < theta < pi/z`
    /// between the middle of its tooth and the middle of the next space; refuses where a point is
    /// shown outside it.
    ///
    /// First the whole stretch is kept above the middle of the tooth (else
    /// [`Why::CutThrough`]: the two fillets of one tooth meet), then below the middle of the space
    /// (else [`Why::FilletsCross`]). For each, a stack of stretches starting with the whole: a
    /// stretch whose enclosure ([`Fillet::enclosure`]) has its lower end above 0 is proved; one
    /// whose function is shown at most 0 at its start, middle or end (a point enclosure whose
    /// upper end is at most 0) refuses; another is halved. A stretch that cannot be halved, or
    /// more than [`MAX_SEPARATION_PIECES`] stretches, refuses with
    /// `NotProved` naming `FILLET_SEPARATION` and the least lower end found.
    ///
    /// Counted: the operations are rounded interval ones (`Ivl`, outward rounded) on the fillet's
    /// doubles as its definition; the sine and cosine of a stretch's middle are enclosed from
    /// their series (`teeth::bound`); pi is `[PI, next_up(PI)]`; the half width is
    /// rounded up; the Taylor remainder comes from the enclosure of the second derivative over the
    /// whole stretch.
    pub(super) fn separation(&self) -> Result<(), TeethFailure> {
        self.keeps(Wedge::AboveMiddle)?;
        self.keeps(Wedge::BelowSpace)
    }

    /// One test of [`Fillet::separation`].
    fn keeps(&self, wedge: Wedge) -> Result<(), TeethFailure> {
        let refused = TeethFailure::Invalid(match wedge {
            Wedge::AboveMiddle => Why::CutThrough,
            Wedge::BelowSpace => Why::FilletsCross,
        });
        let unproved = |reached: f64| TeethFailure::NotProved {
            what: FILLET_SEPARATION,
            asked: 0.0,
            reached,
        };
        if !(self.phi_root.is_finite() && self.phi_join.is_finite()) {
            return Err(unproved(f64::INFINITY));
        }
        let (lo, hi) = if self.phi_root <= self.phi_join {
            (self.phi_root, self.phi_join)
        } else {
            (self.phi_join, self.phi_root)
        };
        if lo == hi {
            let at = self.value(wedge, lo);
            if at.lo > 0.0 {
                return Ok(());
            }
            if at.hi <= 0.0 {
                return Err(refused);
            }
            return Err(unproved(if at.lo.is_nan() { f64::INFINITY } else { at.lo }));
        }
        let mut waiting = vec![(lo, hi)];
        let mut reached = f64::INFINITY;
        let mut popped = 0_usize;
        while let Some((p0, p1)) = waiting.pop() {
            if popped >= MAX_SEPARATION_PIECES {
                return Err(unproved(reached));
            }
            popped += 1;
            let enclosed = self.enclosure(wedge, p0, p1, p0 == lo, p1 == hi);
            if enclosed.lo > 0.0 {
                continue;
            }
            if enclosed.lo < reached {
                reached = enclosed.lo;
            }
            let (m, _) = half_width(p0, p1);
            if [p0, m, p1]
                .into_iter()
                .any(|t| self.value(wedge, t).hi <= 0.0)
            {
                return Err(refused);
            }
            if m > p0 && m < p1 {
                waiting.push((m, p1));
                waiting.push((p0, m));
            } else {
                return Err(unproved(reached));
            }
        }
        Ok(())
    }

    /// What the fillet does over the angles of rolling `[lo, hi]`, for `outline::sampled`:
    /// `P' = R(phi) p1(phi)`, `P'' = R(phi) p2(phi)` with the vectors of [`Fillet::local`]
    /// enclosed over the stretch, and the size of `P''` that of the unrotated `p2`.
    pub(super) fn bend(&self, lo: f64, hi: f64) -> Bend {
        let stretch = Ivl::new(lo, hi);
        let [_, first, second] = self.local(stretch);
        Bend {
            first: rotated_ivl(stretch, first),
            second: rotated_ivl(stretch, second),
            second_size: magnitude(second),
        }
    }

    /// A proved bound of the distance of `p`, the point computed for the angle of rolling `phi`
    /// ([`Fillet::point`], with the platform's `hypot`, sine and cosine), from the fillet's true
    /// point there: `p` measured against `R(phi) q(phi)` enclosed in rounded intervals
    /// ([`Fillet::local`], its length by a square root and the turn's sine and cosine from their
    /// series).
    ///
    /// The enclosure carries the fillet's conditioning by itself. Where the centre of the tool's
    /// tip round lies close to the line of rolling, the unit vector `(w, -vc) / len` is ill
    /// conditioned (its parts' slopes in `w` reach `1 / |vc|`), the rounding of `w = r phi - uc`
    /// widens the enclosure by about `rho / |vc|` times it, and the bound grows to match; with
    /// the centre on the line (`vc = 0`) the enclosure is the whole plane and nothing is proved.
    pub(super) fn off(&self, phi: f64, p: Point) -> f64 {
        let at = exact(phi);
        let [q, _, _] = self.local(at);
        off(p, rotated_ivl(at, q))
    }
}

/// Of two enclosures of one value, the one with the larger lower end (a NaN lower end never
/// wins).
fn higher(a: Ivl, b: Ivl) -> Ivl {
    if b.lo > a.lo || (a.lo.is_nan() && !b.lo.is_nan()) {
        b
    } else {
        a
    }
}

impl Cut {
    /// The upper flank, from where it meets the fillet to the tip circle.
    fn upper_flank(&self) -> Flank {
        let b = &self.base;
        Flank {
            r: b.r,
            s: self.s,
            sin: b.sin,
            cos: b.cos,
            from: self.v_join,
            to: self.v_tip,
        }
    }

    /// The upper root fillet, from the root circle to where it meets the flank.
    fn upper_fillet(&self) -> Fillet {
        Fillet {
            r: self.base.r,
            uc: self.uc,
            vc: self.vc,
            rho: self.rho,
            z: self.base.z,
            phi_root: self.phi_root,
            phi_join: self.phi_join,
        }
    }

    /// The point of the upper flank cut by the point of the rack's flank at height `v` above the
    /// line of rolling ([`Flank::point`]).
    fn flank(&self, v: f64) -> Point {
        self.upper_flank().point(v)
    }

    /// The point of the upper root fillet at the angle of rolling `phi` ([`Fillet::point`]).
    fn fillet(&self, phi: f64) -> Point {
        self.upper_fillet().point(phi)
    }

    /// Half the angle the tooth spans at `radius`, were its flanks involutes all the way down to
    /// the base circle.
    fn half_angle(&self, radius: f64) -> Option<f64> {
        Some(self.s / (2.0 * self.base.r) + self.base.inv - self.base.turned(radius)?)
    }

    /// The height in the rack of the flank's point at `radius` (at or above the base circle).
    fn flank_height(&self, radius: f64) -> f64 {
        let b = &self.base;
        -b.r * b.sin * b.sin + b.sin * (radius * radius - b.rb * b.rb).max(0.0).sqrt()
    }

    /// Where an undercutting fillet crosses the flank: the angle of rolling of the fillet's point
    /// and the height of the flank's.
    ///
    /// From the root upward the fillet is the tooth's boundary for as long as it runs inside the
    /// involute. It starts below the base circle, where there is no involute at all.
    fn crossing(&self, phi_tangent: f64) -> Result<(f64, f64), TeethFailure> {
        const STEPS: u32 = 4096;
        let outside = |phi: f64| {
            let [x, y] = self.fillet(phi);
            self.half_angle(x.hypot(y))
                .is_some_and(|half| y.atan2(x) >= half)
        };
        let at = |i: u32| {
            self.phi_root + (phi_tangent - self.phi_root) * f64::from(i) / f64::from(STEPS)
        };
        if outside(at(0)) {
            return Err(NO_JOIN);
        }
        let first = (1..=STEPS).find(|&i| outside(at(i))).ok_or(NO_JOIN)?;
        let (mut inside_at, mut outside_at) = (at(first - 1), at(first));
        for _ in 0..200 {
            let mid = 0.5 * (inside_at + outside_at);
            if outside(mid) {
                outside_at = mid;
            } else {
                inside_at = mid;
            }
        }
        let [x, y] = self.fillet(outside_at);
        let v = self.flank_height(x.hypot(y));
        let [fx, fy] = self.flank(v);
        if (fx - x).hypot(fy - y) > 1e-9 * (1.0 + self.base.r) {
            return Err(NO_JOIN);
        }
        Ok((outside_at, v))
    }
}

impl External {
    /// The tooth worked out up to where its root fillet meets the flank: the numbers checked, the
    /// rack placed and the fillet's stretch of rolling found, before the fillets are proved apart.
    fn joined(&self) -> Result<Cut, TeethFailure> {
        let b = base(self.module, self.teeth, self.pressure_angle)?;
        let addendum = in_range("addendum", self.addendum, 0.0, 5.0)?;
        let dedendum = in_range("dedendum", self.dedendum, 0.0, 5.0)?;
        let shift = in_range("profile shift", self.shift, -5.0, 5.0)?;
        let rho = zero_up_to("tool tip radius", self.tool_tip_radius, 5.0)? * b.m;
        let thinning = in_range("thinning", self.thinning, -MAX_LENGTH, MAX_LENGTH)?;
        let tan = b.sin / b.cos;

        let s = b.pitch / 2.0 + 2.0 * shift * b.m * tan - thinning;
        if !(s > 0.0 && s < b.pitch) {
            return Err(TeethFailure::Invalid(Why::ToothThickness));
        }
        let depth = (dedendum - shift) * b.m; // of the rack's tip below the line of rolling
        let vc = rho - depth;
        if !(vc < 0.0) {
            return Err(TeethFailure::Invalid(Why::RootAboveTipRound));
        }
        let rf = b.r - depth;
        if !(rf > 1e-9 * b.m) {
            return Err(TeethFailure::Invalid(Why::RootInsideAxis));
        }
        let uc = s / 2.0 + (rho - vc * b.sin) / b.cos;
        if uc > b.pitch / 2.0 * (1.0 + 1e-12) {
            return Err(TeethFailure::Invalid(Why::TipRoundsOverlap));
        }
        let ra = b.r + (addendum + shift) * b.m;

        let mut cut = Cut {
            base: b,
            s,
            rho,
            uc,
            vc,
            ra,
            rf,
            phi_root: uc / b.r,
            phi_join: 0.0,
            v_join: 0.0,
            v_tip: 0.0,
            undercut: false,
            tip_half_angle: 0.0,
        };
        // Where the rack's flank ends and its tip round begins: the lowest point of the flank that
        // cuts.
        let v_tangent = vc - rho * b.sin;
        let phi_tangent = (uc - vc * b.cos / b.sin) / b.r;
        // The flank's point at this height in the rack cuts the tooth on the base circle; lower
        // ones cut it away.
        let v_base = -b.r * b.sin * b.sin;
        cut.undercut = v_tangent < v_base - 1e-12 * b.r;
        (cut.phi_join, cut.v_join) = if cut.undercut {
            cut.crossing(phi_tangent)?
        } else {
            (phi_tangent, v_tangent)
        };
        Ok(cut)
    }

    fn cut(&self) -> Result<Cut, TeethFailure> {
        let mut cut = self.joined()?;
        cut.upper_fillet().separation()?;

        let (ra, rb) = (cut.ra, cut.base.rb);
        let [jx, jy] = cut.flank(cut.v_join);
        if !(ra > jx.hypot(jy) * (1.0 + 1e-12) && ra > rb) {
            return Err(TeethFailure::Invalid(Why::NoFlankLeft));
        }
        cut.v_tip = cut.flank_height(ra);
        let [tx, ty] = cut.flank(cut.v_tip);
        cut.tip_half_angle = ty.atan2(tx);
        if !(cut.tip_half_angle > 0.0) {
            return Err(TeethFailure::Invalid(Why::PointedBelowTip));
        }
        Ok(cut)
    }

    /// The upper root fillet of the tooth, worked out up to where it meets the flank (the fillets
    /// not yet proved apart): the hook of the tests of the fillet's proof.
    #[cfg(test)]
    pub(super) fn fillet(&self) -> Result<Fillet, TeethFailure> {
        Ok(self.joined()?.upper_fillet())
    }

    /// The upper flank of the tooth, from the fillet to the tip circle: the hook of the tests of
    /// the flank's chords.
    #[cfg(test)]
    pub(super) fn flank(&self) -> Result<Flank, TeethFailure> {
        Ok(self.cut()?.upper_flank())
    }

    /// The radius of the pitch circle: half of module times teeth.
    pub fn pitch_radius(&self) -> crate::Result<f64> {
        self.pitch_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::pitch_radius", e))
    }

    /// [`Self::pitch_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn pitch_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.cut()?.base.r)
    }

    /// The radius of the base circle, from which the flanks unwind.
    pub fn base_radius(&self) -> crate::Result<f64> {
        self.base_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::base_radius", e))
    }

    /// [`Self::base_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn base_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.cut()?.base.rb)
    }

    /// The radius of the tip circle.
    pub fn tip_radius(&self) -> crate::Result<f64> {
        self.tip_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::tip_radius", e))
    }

    /// [`Self::tip_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn tip_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.cut()?.ra)
    }

    /// The radius of the root circle.
    pub fn root_radius(&self) -> crate::Result<f64> {
        self.root_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::root_radius", e))
    }

    /// [`Self::root_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn root_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.cut()?.rf)
    }

    /// The thickness of the tooth as an arc on the pitch circle.
    pub fn tooth_thickness(&self) -> crate::Result<f64> {
        self.tooth_thickness_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::tooth_thickness", e))
    }

    /// [`Self::tooth_thickness`] with its failure as a [`TeethFailure`], for the tooth module's
    /// own callers.
    pub(crate) fn tooth_thickness_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.cut()?.s)
    }

    /// The radius at which the involute flank starts: below it the tooth is root fillet. The hub
    /// must not reach below this radius.
    pub fn form_radius(&self) -> crate::Result<f64> {
        self.form_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::form_radius", e))
    }

    /// [`Self::form_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn form_radius_typed(&self) -> Result<f64, TeethFailure> {
        let cut = self.cut()?;
        let [x, y] = cut.flank(cut.v_join);
        Ok(x.hypot(y))
    }

    /// Whether the rack cuts into the flank near the base circle, as it does when the teeth are
    /// few.
    pub fn is_undercut(&self) -> crate::Result<bool> {
        self.is_undercut_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::External::is_undercut", e))
    }

    /// [`Self::is_undercut`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn is_undercut_typed(&self) -> Result<bool, TeethFailure> {
        Ok(self.cut()?.undercut)
    }

    /// Half the angle (radians) the tooth spans at `radius`, for a radius on the flank.
    pub fn half_angle_at(&self, radius: f64) -> crate::Result<f64> {
        self.half_angle_at_typed(radius)
            .map_err(|e| SolidError::teeth("teeth::involute::External::half_angle_at", e))
    }

    /// [`Self::half_angle_at`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn half_angle_at_typed(&self, radius: f64) -> Result<f64, TeethFailure> {
        let cut = self.cut()?;
        let [x, y] = cut.flank(cut.v_join);
        let r = in_range(
            "radius",
            radius,
            x.hypot(y) * (1.0 - 1e-12),
            cut.ra * (1.0 + 1e-12),
        )?;
        cut.half_angle(r.max(cut.base.rb)).ok_or(NO_JOIN)
    }

    /// The outline of the part, a tooth centred on the x axis. Flanks and fillets are given as
    /// points whose chords stay within `tolerance` (millimetres) of the true curves.
    ///
    /// One tooth is six pieces, counter-clockwise: fillet, flank, tip arc, flank, fillet, root arc.
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::involute::External::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        let tolerance = checked_tolerance(tolerance)?;
        let cut = self.cut()?;
        // The upper side, each from the lower end upward, each chord proved within the share of
        // the tolerance a curve given as points has: the rest is for the gap where it joins the
        // next piece, which the outline checks.
        let within = curve_tolerance(tolerance);
        let upper_fillet = cut.upper_fillet();
        let upper_flank = cut.upper_flank();
        let fillet = sampled(
            &|phi| upper_fillet.point(phi),
            &|phi, p| upper_fillet.off(phi, p),
            &|lo, hi| upper_fillet.bend(lo, hi),
            upper_fillet.phi_root,
            upper_fillet.phi_join,
            within,
        )?;
        let flank = sampled(
            &|v| upper_flank.point(v),
            &|v, p| upper_flank.off(v, p),
            &|lo, hi| upper_flank.bend(lo, hi),
            upper_flank.from,
            upper_flank.to,
            within,
        )?;
        let mirrored =
            |points: &[Point]| points.iter().map(|&[x, y]| [x, -y]).collect::<Vec<Point>>();
        let reversed = |points: &[Point]| points.iter().rev().copied().collect::<Vec<Point>>();

        let mut tooth = vec![
            Piece::Curve {
                points: mirrored(&fillet),
                role: Role::Fillet,
            },
            Piece::Curve {
                points: mirrored(&flank),
                role: Role::Flank,
            },
            Piece::Arc {
                centre: [0.0, 0.0],
                radius: cut.ra,
                start: -cut.tip_half_angle,
                sweep: 2.0 * cut.tip_half_angle,
                role: Role::Tip,
            },
            Piece::Curve {
                points: reversed(&flank),
                role: Role::Flank,
            },
            Piece::Curve {
                points: reversed(&fillet),
                role: Role::Fillet,
            },
        ];
        let root_sweep = TAU / cut.base.z - 2.0 * cut.phi_root;
        if root_sweep > 1e-14 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: cut.rf,
                start: cut.phi_root,
                sweep: root_sweep,
                role: Role::Root,
            });
        }
        Outline::new_typed(self.teeth, tooth, tolerance)
    }
}

/// The opening of the hub that goes on a splined shaft.
///
/// The spaces between its teeth have involute flanks, like the teeth of the shaft, and circular
/// rounds where the flanks meet the root circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Internal {
    /// The module in millimetres.
    pub module: f64,
    /// How many teeth, 3 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The pressure angle in degrees, above 1 and at most 50.
    pub pressure_angle: f64,
    /// The width of the space between two teeth, as an arc on the pitch circle, in millimetres.
    pub space_width: f64,
    /// The radius of the tips of the teeth (they point inward), in millimetres. Not below the base
    /// circle.
    pub minor_radius: f64,
    /// The radius of the roots, in millimetres.
    pub major_radius: f64,
    /// The radius of the rounds at the root corners, in millimetres (0 for sharp corners).
    pub fillet_radius: f64,
}

/// An internal part worked out.
#[derive(Clone, Copy, Debug)]
struct Formed {
    base: Base,
    /// Half the angle a space would span on the base circle.
    half_at_base: f64,
    minor: f64,
    major: f64,
    rho: f64,
    /// Half the angle a space spans at the tips of the teeth.
    half_at_minor: f64,
    /// How much of the flank is unwound from the base circle at the tips, and where the root round
    /// begins.
    roll_minor: f64,
    roll_round: f64,
    /// The centre of a root round: its distance from the axis and its angle from the middle of the
    /// space.
    centre_radius: f64,
    centre_angle: f64,
}

/// The upper flank of a hub's tooth space: the involute of the base circle, by the length `roll`
/// unwound from it.
#[derive(Clone, Copy, Debug)]
pub(super) struct HubFlank {
    /// The radius of the base circle.
    pub(super) rb: f64,
    /// Half the angle a space would span on the base circle.
    pub(super) half_at_base: f64,
    /// The length unwound where the flank begins (at the tips of the hub's teeth).
    pub(super) from: f64,
    /// The length unwound where the flank ends (where the root round begins).
    pub(super) to: f64,
}

impl HubFlank {
    /// The point of the upper flank at which a length `roll` is unwound from the base circle.
    pub(super) fn point(&self, roll: f64) -> Point {
        let rb = self.rb;
        let angle = self.half_at_base - (roll / rb - (roll / rb).atan());
        on_circle([0.0, 0.0], rb.hypot(roll), angle)
    }

    /// What the flank does over the unwound lengths `[lo, hi]`, for `outline::sampled`.
    ///
    /// The point is `rho (cos psi, sin psi)` with `rho = sqrt(rb^2 + L^2)` and
    /// `psi = c - L/rb + atan(L/rb)` (`c` the half angle at the base circle), so with
    /// `rho' = L/rho`, `rho'' = rb^2/rho^3`, `psi' = -L^2/(rb rho^2)` and
    /// `psi'' = -2 L rb / rho^4`, in the turning frame of `psi`: `P' = (rho', rho psi')` and
    /// `P'' = (rho'' - rho psi'^2, 2 rho' psi' + rho psi'')`, each enclosed in rounded intervals
    /// over the lengths and turned by the enclosure of `psi` (with `atan` over an interval,
    /// `teeth::bound::atan_over`); the size of `P''` is that of the unturned vector. At the cusp on
    /// the base circle `P'` is 0 and `P''` stays finite.
    pub(super) fn bend(&self, lo: f64, hi: f64) -> Bend {
        let (rb, two) = (exact(self.rb), exact(2.0));
        let l = Ivl::new(lo, hi);
        let rb2 = sqr(rb);
        let rho2 = rb2 + sqr(l);
        let rho = rho2.sqrt();
        let rho1 = l / rho;
        let rho_second = rb2 / (rho2 * rho);
        let psi1 = neg(sqr(l) / (rb * rho2));
        let psi2 = neg(two * l * rb / sqr(rho2));
        let ratio = l / rb;
        let psi = exact(self.half_at_base) - ratio + atan_over(ratio);
        let first = [rho1, rho * psi1];
        let second = [rho_second - rho * sqr(psi1), two * rho1 * psi1 + rho * psi2];
        Bend {
            first: rotated_ivl(psi, first),
            second: rotated_ivl(psi, second),
            second_size: magnitude(second),
        }
    }

    /// The true point of the flank at the unwound length `roll`, enclosed:
    /// `rho (cos psi, sin psi)` in rounded intervals, `rho` by a square root and the arc tangent,
    /// the sine and the cosine from their series (`teeth::bound`).
    fn held(&self, roll: f64) -> [Ivl; 2] {
        let (rb, l) = (exact(self.rb), exact(roll));
        let ratio = l / rb;
        let psi = exact(self.half_at_base) - ratio + atan_over(ratio);
        let rho = (sqr(rb) + sqr(l)).sqrt();
        rotated_ivl(psi, [rho, exact(0.0)])
    }

    /// A proved bound of the distance of `p`, the point computed for the unwound length `roll`
    /// ([`HubFlank::point`], with the platform's `atan`, `hypot`, sine and cosine), from the
    /// flank's true point there.
    pub(super) fn off(&self, roll: f64, p: Point) -> f64 {
        off(p, self.held(roll))
    }
}

impl Formed {
    /// The upper flank, from the tips of the hub's teeth to the root round.
    fn upper_flank(&self) -> HubFlank {
        HubFlank {
            rb: self.base.rb,
            half_at_base: self.half_at_base,
            from: self.roll_minor,
            to: self.roll_round,
        }
    }
}

impl Internal {
    fn formed(&self) -> Result<Formed, TeethFailure> {
        let b = base(self.module, self.teeth, self.pressure_angle)?;
        let space = in_range("space width", self.space_width, 0.0, MAX_LENGTH)?;
        let minor = in_range("minor radius", self.minor_radius, 0.0, MAX_LENGTH)?;
        let major = in_range("major radius", self.major_radius, 0.0, MAX_LENGTH)?;
        let rho = zero_up_to("fillet radius", self.fillet_radius, MAX_LENGTH)?;
        if !(space < b.pitch) {
            return Err(TeethFailure::Invalid(Why::SpacesAsWideAsPitch));
        }
        if minor < b.rb {
            return Err(TeethFailure::Invalid(Why::TipsBelowBase));
        }
        let centre_radius = major - rho;
        if !(centre_radius > minor) {
            return Err(TeethFailure::Invalid(Why::RootsInsideRound));
        }
        let half_at_base = space / (2.0 * b.r) + b.inv;
        let roll_of = |radius: f64| (radius * radius - b.rb * b.rb).max(0.0).sqrt();
        let (roll_minor, roll_centre) = (roll_of(minor), roll_of(centre_radius));
        let turned = |roll: f64| roll / b.rb - (roll / b.rb).atan();
        let half_at_minor = half_at_base - turned(roll_minor);
        if !(half_at_minor < PI / b.z) {
            return Err(TeethFailure::Invalid(Why::SpacesMeetAtTips));
        }
        // The round's centre is on the involute that runs beside the flank at the round's radius,
        // inside the space: the same involute, turned back by the radius over the base radius.
        let centre_angle = half_at_base - rho / b.rb - turned(roll_centre);
        if !(centre_angle >= 0.0) {
            return Err(TeethFailure::Invalid(Why::RootRoundsOverlap));
        }
        Ok(Formed {
            base: b,
            half_at_base,
            minor,
            major,
            rho,
            half_at_minor,
            roll_minor,
            roll_round: roll_centre + rho,
            centre_radius,
            centre_angle,
        })
    }

    /// The upper flank of a space, from the tips to the root round: the hook of the tests of the
    /// flank's chords.
    #[cfg(test)]
    pub(super) fn flank(&self) -> Result<HubFlank, TeethFailure> {
        Ok(self.formed()?.upper_flank())
    }

    /// The radius of the pitch circle.
    pub fn pitch_radius(&self) -> crate::Result<f64> {
        self.pitch_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::Internal::pitch_radius", e))
    }

    /// [`Self::pitch_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn pitch_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.formed()?.base.r)
    }

    /// The radius of the base circle.
    pub fn base_radius(&self) -> crate::Result<f64> {
        self.base_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::Internal::base_radius", e))
    }

    /// [`Self::base_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn base_radius_typed(&self) -> Result<f64, TeethFailure> {
        Ok(self.formed()?.base.rb)
    }

    /// The radius at which the flank ends and the root round begins.
    pub fn form_radius(&self) -> crate::Result<f64> {
        self.form_radius_typed()
            .map_err(|e| SolidError::teeth("teeth::involute::Internal::form_radius", e))
    }

    /// [`Self::form_radius`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn form_radius_typed(&self) -> Result<f64, TeethFailure> {
        let f = self.formed()?;
        Ok(f.base.rb.hypot(f.roll_round))
    }

    /// The outline of the opening, a space centred on the x axis. The flanks are given as points
    /// whose chords stay within `tolerance` (millimetres) of the true curves.
    ///
    /// One pitch is six pieces, counter-clockwise: flank, root round, root arc, root round, flank,
    /// tip arc (four when the corners are sharp).
    pub fn outline(&self, tolerance: f64) -> crate::Result<Outline> {
        self.outline_typed(tolerance)
            .map_err(|e| SolidError::teeth("teeth::involute::Internal::outline", e))
    }

    /// [`Self::outline`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn outline_typed(&self, tolerance: f64) -> Result<Outline, TeethFailure> {
        let tolerance = checked_tolerance(tolerance)?;
        let f = self.formed()?;
        // From the tip outward, each chord proved within the share of the tolerance a curve given
        // as points has: the rest is for the gap where it joins the next piece.
        let flank = f.upper_flank();
        let upper = sampled(
            &|roll| flank.point(roll),
            &|roll, p| flank.off(roll, p),
            &|lo, hi| flank.bend(lo, hi),
            flank.from,
            flank.to,
            curve_tolerance(tolerance),
        )?;
        let lower: Vec<Point> = upper.iter().map(|&[x, y]| [x, -y]).collect();
        let centre = on_circle([0.0, 0.0], f.centre_radius, f.centre_angle);
        // From the root circle's direction to the flank's normal: a quarter turn less what the
        // centre's own involute has unwound.
        let round_sweep = FRAC_PI_2 - (((f.roll_round - f.rho) / f.base.rb).atan());

        let mut tooth = vec![Piece::Curve {
            points: lower,
            role: Role::Flank,
        }];
        if f.rho > 0.0 {
            tooth.push(Piece::Arc {
                centre: [centre[0], -centre[1]],
                radius: f.rho,
                start: -f.centre_angle - round_sweep,
                sweep: round_sweep,
                role: Role::Fillet,
            });
        }
        if f.centre_angle > 1e-14 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: f.major,
                start: -f.centre_angle,
                sweep: 2.0 * f.centre_angle,
                role: Role::Root,
            });
        }
        if f.rho > 0.0 {
            tooth.push(Piece::Arc {
                centre,
                radius: f.rho,
                start: f.centre_angle,
                sweep: round_sweep,
                role: Role::Fillet,
            });
        }
        tooth.push(Piece::Curve {
            points: upper.iter().rev().copied().collect(),
            role: Role::Flank,
        });
        tooth.push(Piece::Arc {
            centre: [0.0, 0.0],
            radius: f.minor,
            start: f.half_at_minor,
            sweep: TAU / f.base.z - 2.0 * f.half_at_minor,
            role: Role::Tip,
        });
        Outline::new_typed(self.teeth, tooth, tolerance)
    }
}

/// How a hub fits its shaft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The play between the flanks, both sides together, as an arc on the pitch circle, in
    /// millimetres.
    pub side_clearance: f64,
    /// How far the hub's teeth reach inward from the pitch circle, in modules.
    pub hub_addendum: f64,
    /// The gap between the tips of the shaft's teeth and the hub's roots, in millimetres.
    pub tip_clearance: f64,
    /// The radius of the rounds at the hub's root corners, in millimetres.
    pub hub_fillet_radius: f64,
    /// The least distance kept between the tips of the hub's teeth and the start of the shaft's
    /// root fillet, in millimetres.
    pub form_clearance: f64,
}

/// The hub that fits a shaft: the same flanks moved apart by the side clearance, roots clear of the
/// shaft's tips, and tips that stay on the shaft's flanks, clear of its root fillets.
///
/// Refused where the hub's teeth would reach into the shaft's root fillets (closer than the form
/// clearance), and where the rounds at the hub's root corners would reach the shaft's tip corners
/// with the hub turned on the shaft as far as the side clearance lets it.
pub fn hub_for(shaft: &External, fit: &Fit) -> crate::Result<Internal> {
    hub_for_typed(shaft, fit).map_err(|e| SolidError::teeth("teeth::involute::hub_for", e))
}

/// [`hub_for`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn hub_for_typed(shaft: &External, fit: &Fit) -> Result<Internal, TeethFailure> {
    let cut = shaft.cut()?;
    let side = zero_up_to("side clearance", fit.side_clearance, MAX_LENGTH)?;
    let reach = in_range("hub addendum", fit.hub_addendum, 0.0, 5.0)? * cut.base.m;
    let tip = in_range("tip clearance", fit.tip_clearance, 0.0, MAX_LENGTH)?;
    let form = zero_up_to("form clearance", fit.form_clearance, MAX_LENGTH)?;
    let minor = cut.base.r - reach;
    if !(minor >= shaft.form_radius_typed()? + form) {
        return Err(TeethFailure::Invalid(Why::HubReachesFillet));
    }
    let hub = Internal {
        module: shaft.module,
        teeth: shaft.teeth,
        pressure_angle: shaft.pressure_angle,
        space_width: cut.s + side,
        minor_radius: minor,
        major_radius: cut.ra + tip,
        fillet_radius: fit.hub_fillet_radius,
    };
    let f = hub.formed()?;
    // Beyond its form radius the hub's space narrows into its root rounds; the shaft's tip corners,
    // the shaft's points nearest to them there, must stay inside the rounds.
    if cut.ra > f.base.rb.hypot(f.roll_round) && f.rho > 0.0 {
        let (ra, c) = (cut.ra, f.centre_radius);
        let reach_of_round = ((ra * ra + c * c - f.rho * f.rho) / (2.0 * ra * c))
            .clamp(-1.0, 1.0)
            .acos();
        // The hub is free to turn on the shaft by half the side clearance either way; turned as far
        // as it goes, the flanks touching, a tip corner stands that much nearer a round.
        let turned = side / (2.0 * cut.base.r);
        if !(cut.tip_half_angle + turned <= (f.centre_angle + reach_of_round) * (1.0 + 1e-12)) {
            return Err(TeethFailure::Invalid(Why::HubRoundsReachTips));
        }
    }
    Ok(hub)
}

/// A splined shaft and the hub that fits it ([`hub_for`]) as a pair on one axis. They turn as one;
/// adding to [`Pair::second_start`] turns the hub on the shaft, to find the play.
///
/// Refused as [`hub_for`] refuses the hub. Centred, the flanks are half the side clearance apart on
/// each side; centred or turned on the shaft until the flanks touch, the parts do not overlap: the
/// hub's tips clear the shaft's fillets by the form clearance, its roots clear the shaft's tips,
/// and its root rounds clear the shaft's tip corners.
pub fn spline_pair(shaft: &External, fit: &Fit, tolerance: f64) -> crate::Result<Pair> {
    spline_pair_typed(shaft, fit, tolerance)
        .map_err(|e| SolidError::teeth("teeth::involute::spline_pair", e))
}

/// [`spline_pair`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn spline_pair_typed(
    shaft: &External,
    fit: &Fit,
    tolerance: f64,
) -> Result<Pair, TeethFailure> {
    let hub = hub_for_typed(shaft, fit)?;
    Ok(Pair {
        first: Part {
            outline: shaft.outline_typed(tolerance)?,
            material: Material::Inside,
            axis: [0.0, 0.0],
        },
        second: Part {
            outline: hub.outline_typed(tolerance)?,
            material: Material::Outside,
            axis: [0.0, 0.0],
        },
        motion: Motion::Turning { ratio: 1.0 },
        second_start: 0.0,
        cycle: TAU / f64::from(shaft.teeth),
    })
}
