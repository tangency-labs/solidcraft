//! Checked-success evaluation of the retained screw definition.
//!
//! Candidates retain the existing scalar formulas. An interval enclosure verifies
//! every returned transcendental value, and an end-to-end point check enforces a
//! uniform model-space cap. The cap is an accuracy requirement, not an assumed
//! libm error estimate: every successful call proves it, otherwise it refuses.

use std::sync::{Mutex, OnceLock};

use super::screw_offset::ScrewOffsetSection;
use super::{Ivl, ProceduralSweep, TAU};
use crate::blend::fillet_section::interval::{
    sin_cos_certified, sin_cos_point_certified, tan_certified,
};
use crate::error::{Result, SolidError};
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::math::gamma;

const OP: &str = "surface::procedural_sweep::checked";
// A fixed requested accuracy, verified at each call. It is never enlarged after
// a failed check and is included explicitly in the model-space certificate.
const ACCURACY_UNITS: usize = 256;
type Box3 = [Ivl; 3];

/// The exact screw section and its source interval
/// ([`ProceduralSweep::screw_offset_section`]).
pub(super) type KeptSection = (ScrewOffsetSection, [f64; 2]);

/// What the checked evaluator derives from the definition alone, kept so that each
/// point check stops re-deriving it: the exact screw section (`None` for a class the
/// evaluator does not cover) and each station span's turn constants. Every slot holds
/// the value of the same expression a fresh derivation evaluates, so a hit cannot
/// change an answer; a failed derivation is not kept. Cleared with the runtime
/// allowance ([`ProceduralSweep::invalidate_runtime_allowance`]).
///
/// It also remembers the latest checked turns and points ([`Memo`]): each is a pure
/// function of the definition and its exact input bits, kept only when it succeeded, so a
/// hit returns what the check computes afresh.
#[derive(Debug, Default)]
pub(super) struct CheckedCache {
    // (boxed: the section is large, and a definition is kept in every swept face)
    section: OnceLock<Option<Box<KeptSection>>>,
    /// One slot per station span, each filled on its first use.
    spans: OnceLock<Vec<OnceLock<SpanTurn>>>,
    /// [`ProceduralSweep::checked_turn_rotation`] by `(t, ending)`.
    turns: Memo<(u64, bool), (Turn, Rotation)>,
    /// The points [`ProceduralSweep::verify_runtime_point`] passed, by `(u, v, point)`.
    points: Memo<[u64; 5], ()>,
    /// [`MapPowers`], formed on first use.
    map_powers: OnceLock<MapPowers>,
}

/// The checked powers [`powi`]`(b, n)`, `n <= 4`, of the map's two scales `b`, kept with
/// the scales' bits: a pure function of them, so a kept power is the one formed afresh;
/// a refused power is `None` and formed afresh, so it carries the same error.
#[derive(Debug)]
struct MapPowers {
    bits: [u64; 2],
    powers: [[Option<f64>; MAP_POWERS]; 2],
}

/// How many powers of each map scale are kept: orders `0..=4`, the partials' bound.
const MAP_POWERS: usize = 5;

/// Slots of a [`Memo`].
const MEMO_SLOTS: usize = 1024;

/// The latest results of a pure function of exact bits, at most one per slot of a table
/// filled on first use: a key's slot is chosen from its bits alone (never an address), and a
/// later key in the same slot replaces it. A hit is a result the function gave for the same
/// key, so it is the result it gives again; which keys are kept never changes an answer,
/// only how often it is computed. The evaluator's own refusals are never kept. The table is
/// locked for each look-up, so a definition shared between threads stays consistent; a
/// poisoned lock is a miss.
pub(super) struct Memo<K, V> {
    slots: Mutex<Option<Box<Slots<K, V>>>>,
}

/// A [`Memo`]'s table: a key and its result per slot, or nothing yet.
type Slots<K, V> = [Option<(K, V)>; MEMO_SLOTS];

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self {
            slots: Mutex::new(None),
        }
    }
}

impl<K, V> std::fmt::Debug for Memo<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Memo")
    }
}

/// The slot of a key whose bits are `bits` (a multiplicative mix, deterministic).
fn memo_slot(bits: &[u64]) -> usize {
    let h = bits.iter().fold(0x243f_6a88_85a3_08d3_u64, |h, &b| {
        (h ^ b).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29)
    });
    // the top bits are the best mixed; the mask keeps the slot below MEMO_SLOTS
    usize::try_from(h >> 48).map_or(0, |slot| slot & (MEMO_SLOTS - 1))
}

impl<K: Copy + PartialEq, V: Copy> Memo<K, V> {
    fn get(&self, key: K, slot: usize) -> Option<V> {
        let slots = self.slots.lock().ok()?;
        match slots.as_ref()?.get(slot)? {
            Some((k, v)) if *k == key => Some(*v),
            _ => None,
        }
    }

    fn put(&self, key: K, slot: usize, value: V) {
        if let Ok(mut slots) = self.slots.lock() {
            let table = slots.get_or_insert_with(|| Box::new([None; MEMO_SLOTS]));
            if let Some(entry) = table.get_mut(slot) {
                *entry = Some((key, value));
            }
        }
    }
}

/// A station span's turn constants: `tan(width / 4)` enclosed, and its scalar candidate
/// when that candidate passed [`verify`] (`None` keeps the refusal for a fresh call); and
/// the span's rotation certificate, the sine and cosine of its middle `(a + b) / 2`
/// enclosed ([`span_mid_sin_cos`]), from which [`ProceduralSweep::span_rotation`] encloses
/// the rotation at every turn in the span with no series.
#[derive(Clone, Copy, Debug)]
struct SpanTurn {
    tau_box: Ivl,
    tau: Option<f64>,
    mid_sin_cos: (Ivl, Ivl),
}

/// The sine and cosine of the middle `(a + b) / 2` of the station span `[a, b]`, enclosed
/// (the twenty-step series over the middle's enclosure).
fn span_mid_sin_cos(a: f64, b: f64) -> (Ivl, Ivl) {
    sin_cos_certified((Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0))
}

/// The enclosure of `tan(width / 4)` for the station span `[a, b]`.
fn span_tau_box(a: f64, b: f64) -> Ivl {
    let width = Ivl::from(b) - Ivl::from(a);
    tan_certified(width / Ivl::from(4.0))
}

/// The checked scalar `tan(width / 4)` for the station span `[a, b]`.
fn span_tau(a: f64, b: f64) -> Result<f64> {
    let width = b - a;
    tan(0.25 * width)
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}
/// The requested accuracy `gamma(ACCURACY_UNITS)` enclosed, formed once: a function of
/// constants, so the kept interval is the one formed afresh.
fn accuracy() -> Ivl {
    static ACCURACY: OnceLock<Ivl> = OnceLock::new();
    *ACCURACY.get_or_init(|| {
        let g = gamma(ACCURACY_UNITS);
        Ivl::checked(g, g)
    })
}
fn scalar_cap(scale: f64) -> f64 {
    (accuracy() * (Ivl::from(1.0) + Ivl::from(scale.abs()))).hi
}

fn verify(candidate: f64, enclosure: Ivl, scale: f64) -> Result<f64> {
    let error = (Ivl::from(candidate) - enclosure).mag();
    if !candidate.is_finite()
        || !enclosure.lo.is_finite()
        || !enclosure.hi.is_finite()
        || !error.is_finite()
        || error > scalar_cap(scale)
    {
        return Err(bad(
            "scalar evaluation is not enclosed within its accuracy requirement",
        ));
    }
    Ok(candidate)
}

/// The enclosure of `atan` at the point `v`, either sign.
fn atan_point_enclosure(v: f64) -> Ivl {
    if v < 0.0 {
        let a = super::atan_point(-v);
        Ivl::new(-a.hi, -a.lo)
    } else {
        super::atan_point(v)
    }
}

/// The enclosure of `atan` at the point `v`, either sign, from the series at the half
/// angle: `atan v = 2 atan(v / (1 + sqrt(1 + v^2)))` for every real `v`, and for
/// `|v| <= 1/2` the half-angle argument is at most `0.237`, so its series (which
/// [`super::alternating`] encloses over the interval holding it, its terms falling) needs
/// about half the terms. Outside `1e-100 <= |v| <= 1/2` it is the whole-angle enclosure.
fn atan_point_halved(v: f64) -> Ivl {
    let a = v.abs();
    if !(1.0e-100..=0.5).contains(&a) {
        return atan_point_enclosure(v);
    }
    let x = Ivl::from(a);
    let one = Ivl::from(1.0);
    let y = x / (one + (one + x * x).sqrt());
    if !(y.lo > 0.0 && y.hi <= 0.25) {
        return atan_point_enclosure(v);
    }
    let half = super::alternating(y, y * y, |n| super::reciprocal(2 * n + 1));
    let whole = Ivl::from(2.0) * half;
    if v < 0.0 {
        Ivl::new(-whole.hi, -whole.lo)
    } else {
        whole
    }
}

fn atan_enclosure(x: Ivl) -> Ivl {
    Ivl::new(atan_point_enclosure(x.lo).lo, atan_point_enclosure(x.hi).hi)
}

pub(super) fn tan(x: f64) -> Result<f64> {
    verify(x.tan(), tan_certified(Ivl::from(x)), x)
}
#[cfg(test)]
fn atan(x: f64) -> Result<f64> {
    // atan_enclosure of the point interval [x, x], its one endpoint enclosed once
    verify(x.atan(), atan_point_enclosure(x), x)
}

pub(super) fn powi(x: f64, n: usize) -> Result<f64> {
    if n > 4 {
        return Err(bad("checked screw powers are bounded at four"));
    }
    let mut enclosure = Ivl::from(1.0);
    for _ in 0..n {
        enclosure = enclosure * Ivl::from(x);
    }
    let candidate = x.powi(n as i32);
    verify(candidate, enclosure, enclosure.mag())
}

/// The candidate sine and cosine of `theta`, formed in one place for every check of a
/// rotation: a compiler may form a sine and a cosine of the same angle together (one
/// `sincos` call), whose last bit can differ from a lone `sin`, so each check takes its
/// candidates from this one function and they are the same bits whichever check runs.
#[inline(never)]
fn candidates(theta: f64) -> (f64, f64) {
    (theta.sin(), theta.cos())
}

/// A checked rotation by `theta`: its sine and cosine, and the enclosures of the exact
/// sine and cosine of `theta` that checked them (which a point check reuses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rotation {
    sine: f64,
    cosine: f64,
    sine_box: Ivl,
    cosine_box: Ivl,
}
impl Rotation {
    /// The checked rotation by `theta`: its sine and cosine checked against the
    /// enclosures of a series stopped once its tail is negligible
    /// ([`sin_cos_point_certified`]), and only where those cannot prove them, against
    /// the twenty-step ones ([`sin_cos_certified`]); it passes when either proves them,
    /// so the order never changes an answer.
    pub(super) fn new(theta: f64) -> Result<Self> {
        let (s, c) = sin_cos_point_certified(theta);
        let (sine, cosine) = candidates(theta);
        match (verify(sine, s, theta), verify(cosine, c, theta)) {
            (Ok(sine), Ok(cosine)) => Ok(Self {
                sine,
                cosine,
                sine_box: s,
                cosine_box: c,
            }),
            _ => Self::new_full(theta),
        }
    }

    /// [`Self::new`] against the twenty-step enclosures alone.
    fn new_full(theta: f64) -> Result<Self> {
        let (s, c) = sin_cos_certified(Ivl::from(theta));
        let (sine, cosine) = candidates(theta);
        Ok(Self {
            sine: verify(sine, s, theta)?,
            cosine: verify(cosine, c, theta)?,
            sine_box: s,
            cosine_box: c,
        })
    }
    pub(super) fn turn(&self, k: Vec3, v: Vec3) -> Vec3 {
        v * self.cosine + k.cross(v) * self.sine + k * (k.dot(v) * (1.0 - self.cosine))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Turn {
    pub theta: f64,
    pub rate: f64,
    pub tau: f64,
    /// What proved `theta`, for a point check at the same parameters.
    proof: TurnProof,
}

/// The proof of a [`Turn`]'s angle: the clamped parameter `t` it was taken at, in the
/// station span `[a, b]` whose `tan(width / 4)` is enclosed by `tau_box`, the enclosure
/// `x_box` of the exact `x` at `t` ([`turn_x_box`]), and the enclosure of the exact turn
/// at `t` that `theta` was checked against.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TurnProof {
    t: f64,
    index: usize,
    a: f64,
    b: f64,
    tau_box: Ivl,
    x_box: Ivl,
    theta_box: Ivl,
}

fn boxed(p: Vec3) -> Box3 {
    [Ivl::from(p.x), Ivl::from(p.y), Ivl::from(p.z)]
}
fn add(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x + u, y + v, z + w]
}
fn sub(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x - u, y - v, z - w]
}
fn scale(a: Box3, b: Ivl) -> Box3 {
    a.map(|x| x * b)
}
fn dot(a: Box3, b: Box3) -> Ivl {
    let [x, y, z] = a;
    let [u, v, w] = b;
    x * u + y * v + z * w
}
fn cross(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [y * w - z * v, z * u - x * w, x * v - y * u]
}
fn magnitude(a: Box3) -> Ivl {
    let [x, y, z] = a.map(|v| Ivl::from(v.mag()));
    (x * x + y * y + z * z).sqrt()
}
fn finite_box(a: Box3) -> bool {
    a.iter().all(|v| v.lo.is_finite() && v.hi.is_finite())
}

/// The exact turn at `t` in the station span `[a, b]` and its rate, enclosed as
/// [`ProceduralSweep::turn_enclosure`] does but for the atan: `x_box` encloses the exact
/// `x = tan(width / 4) (2 (t - a) / width - 1)`, `atan_x` the exact `atan` of the double
/// `x` near it, and as `|atan' | <= 1`, `atan(x_box)` lies within `|x_box - x|` of
/// `atan(x)` (one series for the turn, not one at each end of `x_box`). Outside
/// `|x| <= 1/2`, where the series is not taken, the angle's enclosure is the whole line,
/// as there. The rate is the same expression as there.
fn turn_near(t: f64, a: f64, b: f64, tau_box: Ivl, x: f64, atan_x: Ivl) -> (Ivl, Ivl) {
    let x_box = turn_x_box(t, a, b, tau_box);
    let width = Ivl::from(b) - Ivl::from(a);
    let rate = Ivl::from(4.0) * tau_box / (width * (Ivl::from(1.0) + x_box * x_box));
    if !(x_box.lo >= -0.5 && x_box.hi <= 0.5) {
        return (Ivl::whole(), rate);
    }
    let off = (x_box - Ivl::from(x)).mag();
    let atan = atan_x + Ivl::new(-off, off);
    let theta = (Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0) + Ivl::from(2.0) * atan;
    (theta, rate)
}

/// The enclosure of the exact `x = tan(width / 4) (2 (t - a) / width - 1)` at `t` in the
/// station span `[a, b]`, `tau_box` enclosing `tan(width / 4)`.
fn turn_x_box(t: f64, a: f64, b: f64, tau_box: Ivl) -> Ivl {
    let width = Ivl::from(b) - Ivl::from(a);
    tau_box * (Ivl::from(2.0) * (Ivl::from(t) - Ivl::from(a)) / width - Ivl::from(1.0))
}

impl ProceduralSweep {
    /// Station span `index`'s turn constants, derived on its first use and kept (a long
    /// coil has many spans, and a face or a query meets few of them); `None` past the
    /// spans. `[a, b]` is that span, as [`Self::checked_span_index`] reads it.
    fn span_turn(&self, index: usize, a: f64, b: f64) -> Option<SpanTurn> {
        let slots = self.checked_cache.spans.get_or_init(|| {
            (0..self.stations.len().saturating_sub(1))
                .map(|_| OnceLock::new())
                .collect()
        });
        let slot = slots.get(index)?;
        Some(*slot.get_or_init(|| SpanTurn {
            tau_box: span_tau_box(a, b),
            tau: span_tau(a, b).ok(),
            mid_sin_cos: span_mid_sin_cos(a, b),
        }))
    }

    /// [`span_tau_box`] of span `index`, `[a, b]`, from the kept table.
    fn span_tau_box_at(&self, index: usize, a: f64, b: f64) -> Ivl {
        match self.span_turn(index, a, b) {
            Some(span) => span.tau_box,
            None => span_tau_box(a, b),
        }
    }

    /// [`span_tau`] of span `index`, `[a, b]`, from the kept table; a refusal is
    /// derived afresh, so it carries the same error.
    fn span_tau_at(&self, index: usize, a: f64, b: f64) -> Result<f64> {
        match self.span_turn(index, a, b).and_then(|span| span.tau) {
            Some(tau) => Ok(tau),
            None => span_tau(a, b),
        }
    }

    /// The exact screw section and its source interval
    /// ([`ProceduralSweep::screw_offset_section`]), derived once per definition.
    ///
    /// # Errors
    /// Those of the derivation, which is not kept when it fails.
    pub(super) fn cached_screw_offset_section(&self) -> Result<Option<&KeptSection>> {
        if let Some(section) = self.checked_cache.section.get() {
            return Ok(section.as_deref());
        }
        let section = self.derive_screw_offset_section()?.map(Box::new);
        // Concurrent readers may derive the same section; either is the same value.
        let _ = self.checked_cache.section.set(section);
        match self.checked_cache.section.get() {
            Some(section) => Ok(section.as_deref()),
            None => Err(bad("screw section cache was not filled")),
        }
    }

    fn checked_span(&self, t: f64, ending: bool) -> Result<(f64, f64, f64)> {
        self.checked_span_index(t, ending)
            .map(|(t, a, b, _)| (t, a, b))
    }

    fn checked_span_index(&self, t: f64, ending: bool) -> Result<(f64, f64, f64, usize)> {
        if !t.is_finite() {
            return Err(bad("screw parameter must be finite"));
        }
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("missing screw stations"));
        };
        if !first.is_finite() || !last.is_finite() || first >= last {
            return Err(bad("invalid screw station bounds"));
        }
        let t = crate::math::clamp_between(t, first, last);
        let index = self
            .stations
            .partition_point(|&s| if ending { s < t } else { s <= t })
            .saturating_sub(1)
            .min(self.stations.len().saturating_sub(2));
        let (Some(&a), Some(&b)) = (self.stations.get(index), self.stations.get(index + 1)) else {
            return Err(bad("missing screw span"));
        };
        Ok((t, a, b, index))
    }

    /// The exact turn at `t` and its rate, enclosed from the atan series at both ends of
    /// the enclosure of `x`.
    fn turn_enclosure(&self, t: f64, ending: bool) -> Result<(Ivl, Ivl)> {
        let (t, a, b, index) = self.checked_span_index(t, ending)?;
        let width = Ivl::from(b) - Ivl::from(a);
        let tau = self.span_tau_box_at(index, a, b);
        let x = tau * (Ivl::from(2.0) * (Ivl::from(t) - Ivl::from(a)) / width - Ivl::from(1.0));
        let theta =
            (Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0) + Ivl::from(2.0) * atan_enclosure(x);
        let rate = Ivl::from(4.0) * tau / (width * (Ivl::from(1.0) + x * x));
        Ok((theta, rate))
    }

    /// The checked turn at `t`, on the span that ends there when `ending`.
    ///
    /// The angle is checked first against the exact turn enclosed from the series that
    /// checked `atan(x)` ([`Self::turn_near`]), and only where that cannot prove it,
    /// against the endpoint enclosure [`Self::turn_enclosure`]: it passes when either
    /// proves it, so the order never changes an answer.
    pub(super) fn checked_turn_at_side(&self, t: f64, ending: bool) -> Result<Turn> {
        let (t, a, b, index) = self.checked_span_index(t, ending)?;
        let width = b - a;
        let tau = self.span_tau_at(index, a, b)?;
        let x = tau * (2.0 * (t - a) / width - 1.0);
        // the half-angle series first; where it cannot prove the candidate, the
        // whole-angle one (either proves it)
        let halved = atan_point_halved(x);
        let (atan, atan_x) = if let Ok(atan) = verify(x.atan(), halved, x) {
            (atan, halved)
        } else {
            let whole = atan_point_enclosure(x);
            (verify(x.atan(), whole, x)?, whole)
        };
        let theta = 0.5 * (a + b) + 2.0 * atan;
        let rate = 2.0 * tau / (1.0 + x * x) * (2.0 / width);
        let tau_box = self.span_tau_box_at(index, a, b);
        let (near_box, rate_box) = turn_near(t, a, b, tau_box, x, atan_x);
        let scale = a.abs().max(b.abs());
        let (theta, theta_box) = if let Ok(theta) = verify(theta, near_box, scale) {
            (theta, near_box)
        } else {
            let (theta_box, _) = self.turn_enclosure(t, ending)?;
            (verify(theta, theta_box, scale)?, theta_box)
        };
        Ok(Turn {
            theta,
            rate: verify(rate, rate_box, rate_box.mag())?,
            tau,
            proof: TurnProof {
                t,
                index,
                a,
                b,
                tau_box,
                x_box: turn_x_box(t, a, b, tau_box),
                theta_box,
            },
        })
    }

    /// [`Self::checked_turn_at_side`] at `(t, ending)` and the checked [`Rotation`] by its
    /// angle, kept by the exact bits of `t` ([`CheckedCache`]). `ending` only chooses between
    /// the two spans that meet at a station (a `t` clamped to the ends included), so it is part
    /// of the key there alone: strictly inside a span both sides are the same computation.
    ///
    /// # Errors
    /// Those of the turn's check, then of the rotation's.
    pub(super) fn checked_turn_rotation(&self, t: f64, ending: bool) -> Result<(Turn, Rotation)> {
        let at_station = match (self.stations.first(), self.stations.last()) {
            (Some(&first), Some(&last)) if t > first && t < last => {
                // the comparisons `checked_span_index` takes the span with
                let at = self.stations.partition_point(|&s| s < t);
                self.stations.get(at).is_some_and(|&s| s == t)
            }
            _ => true,
        };
        let key = (t.to_bits(), ending && at_station);
        let slot = memo_slot(&[key.0, u64::from(key.1)]);
        if let Some(kept) = self.checked_cache.turns.get(key, slot) {
            return Ok(kept);
        }
        let turn = self.checked_turn_at_side(t, ending)?;
        // the span's certificate first; where it cannot prove the rotation, the series at
        // the angle (either proves it, and both check the same candidates)
        let rotation = match self.span_rotation(&turn) {
            Some(rotation) => rotation,
            None => Rotation::new(turn.theta)?,
        };
        self.checked_cache.turns.put(key, slot, (turn, rotation));
        Ok((turn, rotation))
    }

    /// The checked power [`powi`]`(b, n)` of the map's scale `b` on the profile's
    /// (`axis` 0) or the turn's (`axis` 1) parameter, from the definition's kept powers
    /// ([`MapPowers`]) when they were formed for the scales the map has now.
    ///
    /// # Errors
    /// Those of [`powi`]; an `axis` past the two.
    pub(super) fn checked_map_power(&self, axis: usize, n: usize) -> Result<f64> {
        let base = *self
            .map
            .b
            .get(axis)
            .ok_or_else(|| bad("a sweep map has two scales"))?;
        let bits = self.map.b.map(f64::to_bits);
        let kept = self.checked_cache.map_powers.get_or_init(|| MapPowers {
            bits,
            powers: self.map.b.map(|b| std::array::from_fn(|n| powi(b, n).ok())),
        });
        let hit = (kept.bits == bits)
            .then(|| {
                kept.powers
                    .get(axis)
                    .and_then(|p| p.get(n))
                    .copied()
                    .flatten()
            })
            .flatten();
        match hit {
            Some(power) => Ok(power),
            None => powi(base, n),
        }
    }

    /// The checked rotation by `turn`'s angle from its station span's certificate, no
    /// series taken: the candidates `sin theta` and `cos theta` checked against
    /// enclosures formed from the span's middle sine and cosine ([`SpanTurn`]) by a few
    /// interval products; `None` where those cannot prove them.
    ///
    /// Soundness: the exact turn at the turn's `t` is `theta(t) = m + 2 atan x`, `m = (a +
    /// b) / 2` the span's middle and `x` the exact `x(t)`, which `x_box` encloses. With
    /// `cos(2 atan x) = (1 - x^2) / (1 + x^2)` and `sin(2 atan x) = 2 x / (1 + x^2)` (the
    /// rational turn the station spans are built on), `sin theta(t) = sin m (1 - x^2) / (1 +
    /// x^2) + cos m 2 x / (1 + x^2)` and `cos theta(t) = cos m (1 - x^2) / (1 + x^2) - sin m 2
    /// x / (1 + x^2)`, which interval arithmetic over the enclosures of `sin m`, `cos m` and
    /// `x` encloses. Sine and cosine move by at most the change of their angle, and the
    /// candidate angle `theta` lies within `|theta_box - theta|` of `theta(t)` (`theta_box`
    /// encloses `theta(t)`), so widened by that the boxes enclose the exact `sin theta` and
    /// `cos theta` the candidates are checked against, as [`Rotation::new`]'s do; the check
    /// is the same [`verify`] with the same scale.
    fn span_rotation(&self, turn: &Turn) -> Option<Rotation> {
        let proof = &turn.proof;
        let span = self.span_turn(proof.index, proof.a, proof.b)?;
        let (sin_m, cos_m) = span.mid_sin_cos;
        let x = proof.x_box;
        let square = x * x;
        // the exact square is not negative
        let square = Ivl::new(square.lo.max(0.0), square.hi);
        let one = Ivl::from(1.0);
        let den = one + square;
        let cos_2 = (one - square) / den;
        let sin_2 = Ivl::from(2.0) * x / den;
        let moved = (proof.theta_box - Ivl::from(turn.theta)).mag();
        let widen = Ivl::new(-moved, moved);
        let sine_box = sin_m * cos_2 + cos_m * sin_2 + widen;
        let cosine_box = cos_m * cos_2 - sin_m * sin_2 + widen;
        if !(sine_box.is_finite() && cosine_box.is_finite()) {
            return None;
        }
        let theta = turn.theta;
        let (sine, cosine) = candidates(theta);
        Some(Rotation {
            sine: verify(sine, sine_box, theta).ok()?,
            cosine: verify(cosine, cosine_box, theta).ok()?,
            sine_box,
            cosine_box,
        })
    }

    /// Monotone mathematical turn range, including exact station endpoints.
    pub(crate) fn mathematical_turn_range(&self, range: Ivl) -> Result<Ivl> {
        if !range.lo.is_finite() || !range.hi.is_finite() || range.lo > range.hi {
            return Err(bad("invalid mathematical turn range"));
        }
        let endpoint = |value: f64, ending: bool| -> Result<Ivl> {
            let (t, a, b) = self.checked_span(value, ending)?;
            if t == a || t == b {
                return Ok(Ivl::from(t));
            }
            Ok(self.turn_enclosure(t, ending)?.0)
        };
        let lo = endpoint(range.lo, false)?;
        let hi = endpoint(range.hi, true)?;
        let result = Ivl::new(lo.lo, hi.hi);
        if !result.lo.is_finite() || !result.hi.is_finite() {
            return Err(bad("turn range could not be enclosed"));
        }
        Ok(result)
    }

    pub(super) fn checked_source_class(&self) -> Result<bool> {
        // canonical() dehomogenizes rational controls with rounded division.
        // Until those quotient intervals are retained, they cannot define the
        // independent exact source used by this runtime certificate.
        let source = self
            .offset
            .as_ref()
            .map_or(&self.profile, |offset| &offset.source);
        Ok(!source.is_rational()?)
    }

    /// Uniform cap for every successful retained affine screw point evaluation
    /// at the same face parameters. Includes source evaluation, signed/swapped
    /// map arithmetic, raw-axis Rodrigues motion and lift. This is enforced by
    /// an end-to-end interval check; no legacy floor or libm accuracy is assumed.
    /// Exact-real net/profile approximation and unit-axis reference differences
    /// are separate certificate terms and are not included here.
    pub(crate) fn runtime_evaluation_allowance(&self) -> Result<f64> {
        if let Some(&cap) = self.runtime_allowance.get() {
            return Ok(cap);
        }
        let cap = self.compute_runtime_evaluation_allowance()?;
        // Concurrent immutable readers may prove the same cap independently;
        // either identical successful proof is valid. No failed proof is cached.
        let _ = self.runtime_allowance.set(cap);
        Ok(cap)
    }

    /// The runtime evaluation allowance when this definition's class has one:
    /// `Ok(None)` for the classes the checked evaluator does not cover (rational
    /// source controls; no invariant affine screw section, such as a varying
    /// radial law on a tapered thread or a path sweep), and the allowance's own
    /// error for a covered class whose proof fails (a nonfinite cap, missing
    /// stations, an enclosure that cannot be formed).
    ///
    /// # Errors
    /// Those of inspecting the class and of [`Self::runtime_evaluation_allowance`].
    pub(crate) fn runtime_evaluation_allowance_if_proved(&self) -> Result<Option<f64>> {
        if !self.checked_source_class()? || self.cached_screw_offset_section()?.is_none() {
            return Ok(None);
        }
        self.runtime_evaluation_allowance().map(Some)
    }

    /// Definition-field editors must call this before changing S or its map.
    /// Stored records contain no runtime cache and always prove their own cap.
    pub(super) fn invalidate_runtime_allowance(&mut self) {
        let _ = self.runtime_allowance.take();
        self.checked_cache = std::sync::Arc::default();
    }

    fn compute_runtime_evaluation_allowance(&self) -> Result<f64> {
        if !self.checked_source_class()? {
            return Err(bad(
                "checked point allowance needs exact nonrational source controls",
            ));
        }
        let Some((section, [lo, hi])) = self.cached_screw_offset_section()? else {
            return Err(bad(
                "checked point allowance needs an invariant affine source",
            ));
        };
        let (lo, hi) = (*lo, *hi);
        let motion = self.screw().ok_or_else(|| bad("missing screw motion"))?;
        let domain = self.profile.natural_interval();
        let width = Ivl::from(hi) - Ivl::from(lo);
        let source = (Ivl::new(domain.min(), domain.max()) - Ivl::from(lo)) / width;
        let point = section.point_enclosure(source)?;
        let [profile_speed, turn_speed] = section.speed_upper_bounds(source)?;
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("missing screw stations"));
        };
        let map_size = |range: Ivl, a: f64, b: f64| -> Result<Ivl> {
            if !a.is_finite() || !b.is_finite() || b == 0.0 {
                return Err(bad("invalid screw parameter map"));
            }
            let native = (range - Ivl::from(a)) / Ivl::from(b);
            Ok(Ivl::from(a.abs()) + Ivl::from(b.abs()) * Ivl::from(native.mag()))
        };
        let [a0, a1] = self.map.a;
        let [b0, b1] = self.map.b;
        let mw = map_size(Ivl::new(domain.min(), domain.max()), a0, b0)?;
        let mt = map_size(Ivl::new(first, last), a1, b1)?;
        let mut rate = 0.0_f64;
        for pair in self.stations.windows(2) {
            let [a, b] = *pair else { continue };
            let w = Ivl::from(b) - Ivl::from(a);
            rate = rate.max((Ivl::from(4.0) * tan_certified(w / Ivl::from(4.0)) / w).hi);
        }
        let theta = Ivl::from(first.abs().max(last.abs()));
        let k = magnitude(boxed(motion.axis()));
        let arm = magnitude(sub(point, boxed(motion.origin())));
        let angular = arm * (Ivl::from(1.0) + k + k * k)
            + Ivl::from(motion.pitch().abs()) / Ivl::checked(TAU, TAU) * k;
        let size = Ivl::from(1.0)
            + magnitude(point)
            + magnitude(boxed(motion.origin()))
            + angular * (Ivl::from(1.0) + theta)
            + Ivl::from(profile_speed) / width * (Ivl::from(1.0) + mw)
            + Ivl::from(turn_speed) * Ivl::from(rate) * (Ivl::from(1.0) + mt);
        let cap = (accuracy() * size).hi;
        if !cap.is_finite() || cap <= 0.0 {
            return Err(bad("checked screw allowance is not finite"));
        }
        Ok(cap)
    }

    /// Encloses every successful definition evaluation in a whole UV rectangle.
    /// Unlike an active-control hull, the source and turn intervals contract with
    /// the requested patch. The uniform runtime cap encloses actual S evaluations.
    pub(crate) fn definition_patch_enclosure(
        &self,
        uv: crate::geom::extent2d::Extent2d,
    ) -> Result<Option<crate::geom::extent3d::Extent3d>> {
        let Some(point) = self.checked_patch_enclosure([
            Ivl::new(uv.min().x, uv.max().x),
            Ivl::new(uv.min().y, uv.max().y),
        ])?
        else {
            return Ok(None);
        };
        let cap = self.runtime_evaluation_allowance()?;
        let [x, y, z] = point.map(|coordinate| coordinate + Ivl::new(-cap, cap));
        Ok(Some(crate::geom::extent3d::Extent3d::from_bounds(
            x.lo, y.lo, z.lo, x.hi, y.hi, z.hi,
        )))
    }

    fn checked_point_enclosure(&self, uv: Point2) -> Result<Option<Box3>> {
        self.checked_patch_enclosure([Ivl::from(uv.x), Ivl::from(uv.y)])
    }

    fn checked_patch_enclosure(&self, [u, v]: [Ivl; 2]) -> Result<Option<Box3>> {
        let Some((point, t)) = self.checked_source_and_turn_parameter([u, v])? else {
            return Ok(None);
        };
        // theta(t) is monotone and continuous across all retained arc joints.
        // Endpoint enclosures therefore cover even a rounded map crossing a knot.
        let theta = self.mathematical_turn_range(t)?;
        // A whole patch can cross many quadrants. A width at most one radian
        // leaves the nearest-quadrant remainder inside [-1.5,1.5]; wider
        // patches use the mathematical global range, not a failed local proof.
        let (s, c) = if (Ivl::from(theta.hi) - Ivl::from(theta.lo)).hi > 1.0 {
            (Ivl::new(-1.0, 1.0), Ivl::new(-1.0, 1.0))
        } else {
            sin_cos_certified(theta)
        };
        self.screw_box(point, theta, s, c).map(Some)
    }

    /// The enclosure of the exact source point over the face's `[u, v]` and of the exact
    /// turn parameter `t` there, or `None` for a class the evaluator does not cover.
    fn checked_source_and_turn_parameter(&self, [u, v]: [Ivl; 2]) -> Result<Option<(Box3, Ivl)>> {
        if !self.checked_source_class()? {
            return Ok(None);
        }
        let Some((section, [lo, hi])) = self.cached_screw_offset_section()? else {
            return Ok(None);
        };
        let (lo, hi) = (*lo, *hi);
        if ![u.lo, u.hi, v.lo, v.hi].iter().all(|x| x.is_finite()) || u.lo > u.hi || v.lo > v.hi {
            return Err(bad("face parameters must be finite"));
        }
        if self.screw().is_none() {
            return Err(bad("missing screw motion"));
        }
        let (p, q) = if self.map.swapped { (v, u) } else { (u, v) };
        let [a0, a1] = self.map.a;
        let [b0, b1] = self.map.b;
        let w = Ivl::from(a0) + Ivl::from(b0) * p;
        let t = Ivl::from(a1) + Ivl::from(b1) * q;
        let domain = self.profile.natural_interval();
        if !domain.min().is_finite() || !domain.max().is_finite() || domain.min() >= domain.max() {
            return Err(bad("invalid screw profile bounds"));
        }
        let w = Ivl::new(
            crate::math::clamp_between(w.lo, domain.min(), domain.max()),
            crate::math::clamp_between(w.hi, domain.min(), domain.max()),
        );
        let point =
            section.point_enclosure((w - Ivl::from(lo)) / (Ivl::from(hi) - Ivl::from(lo)))?;
        Ok(Some((point, t)))
    }

    /// The screw motion of the source box `point` by the turn `theta`, whose sine and
    /// cosine `s` and `c` enclose.
    fn screw_box(&self, point: Box3, theta: Ivl, s: Ivl, c: Ivl) -> Result<Box3> {
        let motion = self.screw().ok_or_else(|| bad("missing screw motion"))?;
        let k = boxed(motion.axis());
        let origin = boxed(motion.origin());
        let r = sub(point, origin);
        let rotated = add(
            add(scale(r, c), scale(cross(k, r), s)),
            scale(k, dot(k, r) * (Ivl::from(1.0) - c)),
        );
        let result = add(
            add(origin, rotated),
            scale(
                k,
                Ivl::from(motion.pitch()) * theta / Ivl::checked(TAU, TAU),
            ),
        );
        if !finite_box(result) {
            return Err(bad("screw point cannot be enclosed"));
        }
        Ok(result)
    }

    /// The enclosure of the exact definition point at the face's `uv` from the proofs of
    /// the turn and rotation the candidate was evaluated with, no series taken; `None`
    /// where they do not cover it (a class the evaluator does not cover, an exact turn
    /// parameter past the spans next to the turn's, a failed enclosure).
    ///
    /// Soundness: the exact turn parameter lies in `t_box` (the map in interval
    /// arithmetic), clamped to the stations in `reached`, where the exact turn is
    /// `theta(t') = (a + b) / 2 + 2 atan(x(t'))` of the span holding `t'`, its rate at
    /// most `slope` ([`Self::turn_slope_over`]). So it lies within `slope |t' - t|` of
    /// the exact turn at the turn's `t`, which `theta_box` encloses: `theta` below
    /// encloses it. Sine and cosine move by at most
    /// the change of their angle, so the exact sine and cosine there lie within
    /// `|theta - theta_c|` of those of the candidate's angle `theta_c`, which the
    /// rotation's boxes enclose. The rest is [`Self::screw_box`], as for a patch.
    fn turned_point_enclosure(
        &self,
        uv: Point2,
        turn: &Turn,
        rotation: &Rotation,
    ) -> Result<Option<Box3>> {
        let Some((point, t_box)) =
            self.checked_source_and_turn_parameter([Ivl::from(uv.x), Ivl::from(uv.y)])?
        else {
            return Ok(None);
        };
        let proof = &turn.proof;
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Ok(None);
        };
        // the exact parameters the definition takes there, clamped as it clamps
        let reached = Ivl::new(
            crate::math::clamp_between(t_box.lo, first, last),
            crate::math::clamp_between(t_box.hi, first, last),
        );
        let Some(slope) = self.turn_slope_over(proof, reached) else {
            return Ok(None);
        };
        let off = (reached - Ivl::from(proof.t)).mag();
        let theta = proof.theta_box + Ivl::from(slope) * Ivl::new(-off, off);
        let moved = (theta - Ivl::from(turn.theta)).mag();
        let s = rotation.sine_box + Ivl::new(-moved, moved);
        let c = rotation.cosine_box + Ivl::new(-moved, moved);
        if !(theta.is_finite() && s.is_finite() && c.is_finite()) {
            return Ok(None);
        }
        Ok(self.screw_box(point, theta, s, c).ok())
    }

    /// An upper bound on the rate of the exact turn over `reached` and the turn's own
    /// parameter: the largest `4 tan(width / 4) / width`, enclosed above, of the turn's
    /// station span and of the span on either side that `reached` runs into; `None` past
    /// those.
    ///
    /// Soundness: on a span the exact turn's rate `4 tau / (width (1 + x^2))` is at most
    /// `4 tau / width`, and the turn is continuous at each station (`theta(a) = a` from
    /// both sides), so the largest of the bounds bounds its change between any two
    /// parameters of the spans taken.
    fn turn_slope_over(&self, proof: &TurnProof, reached: Ivl) -> Option<f64> {
        let slope = |index: usize| -> Option<f64> {
            let (&a, &b) = (self.stations.get(index)?, self.stations.get(index + 1)?);
            let tau = self.span_tau_box_at(index, a, b);
            let width = Ivl::from(b) - Ivl::from(a);
            let bound = (Ivl::from(4.0) * tau / width).hi;
            (tau.lo > 0.0 && width.lo > 0.0 && bound.is_finite()).then_some(bound)
        };
        let mut bound = slope(proof.index)?;
        if reached.lo < proof.a {
            let before = proof.index.checked_sub(1)?;
            if !(*self.stations.get(before)? <= reached.lo) {
                return None;
            }
            bound = bound.max(slope(before)?);
        }
        if reached.hi > proof.b {
            let after = proof.index + 1;
            if !(*self.stations.get(after + 1)? >= reached.hi) {
                return None;
            }
            bound = bound.max(slope(after)?);
        }
        Some(bound)
    }

    /// [`Self::verify_runtime_point`] for a candidate evaluated with `turn` and
    /// `rotation`: the enclosure from their proofs ([`Self::turned_point_enclosure`])
    /// first, and only where it does not prove the candidate, the full check. It passes
    /// when either proves the candidate, so the order never changes an answer.
    ///
    /// # Errors
    /// Those of [`Self::verify_runtime_point`].
    pub(super) fn verify_turned_point(
        &self,
        uv: Point2,
        candidate: Point3,
        turn: &Turn,
        rotation: &Rotation,
    ) -> Result<()> {
        let key = [uv.x, uv.y, candidate.x, candidate.y, candidate.z].map(f64::to_bits);
        let slot = memo_slot(&key);
        if self.checked_cache.points.get(key, slot).is_some() {
            return Ok(());
        }
        if let Ok(Some(enclosure)) = self.turned_point_enclosure(uv, turn, rotation) {
            let error = magnitude(sub(boxed(candidate), enclosure)).hi;
            let cap = self.runtime_evaluation_allowance();
            if error.is_finite() && cap.is_ok_and(|cap| error <= cap) {
                self.checked_cache.points.put(key, slot, ());
                return Ok(());
            }
        }
        self.verify_runtime_point(uv, candidate)
    }

    pub(super) fn verify_runtime_point(&self, uv: Point2, candidate: Point3) -> Result<()> {
        let key = [uv.x, uv.y, candidate.x, candidate.y, candidate.z].map(f64::to_bits);
        let slot = memo_slot(&key);
        if self.checked_cache.points.get(key, slot).is_some() {
            return Ok(());
        }
        let Some(enclosure) = self.checked_point_enclosure(uv)? else {
            return Ok(());
        };
        let error = magnitude(sub(boxed(candidate), enclosure)).hi;
        if !error.is_finite() || error > self.runtime_evaluation_allowance()? {
            return Err(bad("screw point exceeds its checked runtime allowance"));
        }
        self.checked_cache.points.put(key, slot, ());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidates_are_verified_not_replaced() {
        let x = 0.371_f64;
        assert_eq!(tan(x).unwrap(), x.tan());
        assert_eq!(atan(x).unwrap(), x.atan());
        let r = Rotation::new(x).unwrap();
        assert_eq!(r.sine, x.sin());
        assert_eq!(r.cosine, x.cos());
        let (s, _) = sin_cos_certified(Ivl::from(x));
        assert!(verify(0.5, s, x).is_err());
        assert!(verify(f64::NAN, s, x).is_err());
        assert!(Rotation::new(3e15).is_err());
    }

    /// The half-angle atan meets the whole-angle one, within some roundings, on both
    /// signs and across `|v| <= 1/2`, and is the whole-angle one outside.
    #[test]
    fn the_half_angle_atan_meets_the_whole_angle_one() {
        for i in -5000..=5000 {
            let v = f64::from(i) * 1.0e-4;
            for v in [v, v * 1.0e-90, v + 3.3e-5] {
                if v.abs() > 0.5 {
                    continue;
                }
                let (h, w) = (atan_point_halved(v), atan_point_enclosure(v));
                let slack = 1.0e-14 * v.abs() + 1.0e-300;
                assert!(h.lo <= w.hi && w.lo <= h.hi, "{v}: {h:?} {w:?}");
                assert!(
                    (h.lo - w.lo).abs() <= slack && (h.hi - w.hi).abs() <= slack,
                    "{v}"
                );
            }
        }
        for v in [0.0, 0.6, -0.7, 1.0e-120, f64::NAN] {
            let (h, w) = (atan_point_halved(v), atan_point_enclosure(v));
            assert_eq!(
                [h.lo, h.hi].map(f64::to_bits),
                [w.lo, w.hi].map(f64::to_bits)
            );
        }
    }

    /// The rotation from the span's certificate checks the same candidates as the
    /// series at the angle, and proves them at every turn of short and long spans,
    /// small and large angles, the stations and the clamped ends included; its boxes
    /// meet the series' boxes.
    #[test]
    fn span_rotations_meet_the_series_ones() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let far = (0..=8)
            .map(|i| 4000.0 + f64::from(i) * 0.45)
            .collect::<Vec<_>>();
        for stations in [vec![0.0, 0.3, 0.7, 1.2, 1.25], far] {
            let definition =
                ProceduralSweep::create(&profile, motion.clone(), &stations, 0.3).unwrap();
            let (first, last) = (stations[0], stations[stations.len() - 1]);
            let mut covered = 0;
            for i in -10..=1010 {
                let t = first + (last - first) * f64::from(i) / 1000.0;
                for t in stations.iter().copied().chain([t, t + 1.0e-13]) {
                    for ending in [false, true] {
                        let turn = definition.checked_turn_at_side(t, ending).unwrap();
                        let series = Rotation::new(turn.theta).unwrap();
                        let span = definition.span_rotation(&turn).unwrap();
                        assert_eq!(
                            [span.sine, span.cosine].map(f64::to_bits),
                            [series.sine, series.cosine].map(f64::to_bits)
                        );
                        let meet = |a: Ivl, b: Ivl| a.lo <= b.hi && b.lo <= a.hi;
                        assert!(meet(span.sine_box, series.sine_box), "t {t}");
                        assert!(meet(span.cosine_box, series.cosine_box), "t {t}");
                        covered += 1;
                    }
                }
            }
            assert!(covered > 2000);
        }
    }

    /// The point check from the turn's and rotation's proofs covers the points inside
    /// spans, at stations and past the ends (where the map's interval runs into the next
    /// span or is clamped), and refuses what the full check refuses: a candidate off by
    /// twice the allowance.
    #[test]
    fn turned_point_checks_agree_with_the_full_check() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.3, 0.7, 1.2, 1.25];
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.3).unwrap();
        let cap = definition.runtime_evaluation_allowance().unwrap();
        let mut covered = 0;
        for &t in &[
            -0.5,
            0.0,
            0.1,
            0.3,
            0.30000000000000004,
            0.55,
            0.7,
            1.2,
            1.249,
            1.25,
            3.0,
        ] {
            for ending in [false, true] {
                let uv = Point2::new(0.37, t);
                let (turn, rotation) = definition.checked_turn_rotation(t, ending).unwrap();
                let point = definition
                    .evaluate_definition_side(uv, 0, 0, ending)
                    .unwrap()[0][0];
                // copies with nothing kept, so neither check meets the other's pass
                let fresh = || {
                    let mut copy = definition.clone();
                    copy.invalidate_runtime_allowance();
                    copy
                };
                let (turned, full) = (fresh(), fresh());
                turned
                    .verify_turned_point(uv, point, &turn, &rotation)
                    .unwrap();
                full.verify_runtime_point(uv, point).unwrap();
                if let Some(enclosure) =
                    turned.turned_point_enclosure(uv, &turn, &rotation).unwrap()
                {
                    // the proofs alone prove the candidate
                    assert!(magnitude(sub(boxed(point), enclosure)).hi <= cap, "t {t}");
                    covered += 1;
                }
                let off = point + Vec3::X * (2.0 * cap);
                assert!(turned
                    .verify_turned_point(uv, off, &turn, &rotation)
                    .is_err());
                assert!(full.verify_runtime_point(uv, off).is_err());
            }
        }
        // the proofs cover every one of them, the stations and the clamped ends included
        assert_eq!(covered, 22);
    }

    /// A class without a checked evaluator (rational controls, a varying radial
    /// law) has no allowance, and says so with `None`; a covered class whose
    /// proof fails is an error, never mistaken for a class refusal.
    #[test]
    fn allowance_if_proved_separates_class_refusals_from_failed_proofs() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;
        use crate::surface::procedural_sweep::{RadialLaw, ScrewMotion};

        let (start, end) = (Vec3::new(8.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 1.0));
        let stations = [0.0, 0.3, 0.6];
        let line = BSplineCurve::create_line_segment(3, start, end).unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let plain = ProceduralSweep::create(&line, motion.clone(), &stations, 0.15).unwrap();
        assert_eq!(
            plain.runtime_evaluation_allowance_if_proved().unwrap(),
            Some(plain.compute_runtime_evaluation_allowance().unwrap())
        );

        // the same line, both controls homogeneous with weight 2
        let pw = vec![
            CPoint::new(16.0, 0.0, 0.0, 2.0),
            CPoint::new(20.0, 0.0, 2.0, 2.0),
        ];
        let rational = BSplineCurve::from_nurb(
            NlCurve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap(),
            3,
            false,
        );
        assert!(rational.is_rational().unwrap());
        let weighted = ProceduralSweep::create(&rational, motion, &stations, 0.15).unwrap();
        assert_eq!(
            weighted.runtime_evaluation_allowance_if_proved().unwrap(),
            None
        );

        let law = RadialLaw::new(Vec3::X, vec![0.0, 0.6], vec![0.0, 0.1]).unwrap();
        let tapered = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, Some(law)).unwrap();
        let tapered = ProceduralSweep::create(&line, tapered, &stations, 0.15).unwrap();
        assert_eq!(
            tapered.runtime_evaluation_allowance_if_proved().unwrap(),
            None
        );

        let mut unstationed = plain.clone();
        unstationed.invalidate_runtime_allowance();
        unstationed.stations.clear();
        assert!(unstationed
            .runtime_evaluation_allowance_if_proved()
            .is_err());
    }

    #[test]
    fn point_checks_reuse_the_uniform_station_proof() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = (0..=560).map(|i| f64::from(i) * 0.1).collect::<Vec<_>>();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
        let proved = definition.compute_runtime_evaluation_allowance().unwrap();
        let first = Point2::new(0.37, 0.0);
        let point = definition.evaluate_definition(first, 0, 0).unwrap()[0][0];
        definition.verify_runtime_point(first, point).unwrap();
        // The first point check proves the uniform station cap once and keeps it.
        assert_eq!(definition.runtime_allowance.get().copied(), Some(proved));
        for i in 0..16 {
            let uv = Point2::new(0.37, f64::from(i) * 0.1);
            let point = definition.evaluate_definition(uv, 0, 0).unwrap()[0][0];
            definition.verify_runtime_point(uv, point).unwrap();
            assert!(definition
                .verify_runtime_point(uv, point + Vec3::X)
                .is_err());
        }
        assert_eq!(definition.runtime_allowance.get().copied(), Some(proved));

        let original = definition.runtime_evaluation_allowance().unwrap();
        let mut changed = definition.clone();
        assert_eq!(changed.runtime_allowance.get().copied(), Some(original));
        assert_eq!(changed.runtime_evaluation_allowance().unwrap(), original);
        changed.reparametrize_map(true, 1e8, -2.0);
        assert!(changed.runtime_allowance.get().is_none());
        let mapped = changed.runtime_evaluation_allowance().unwrap();
        assert!(mapped > original);
        assert_eq!(
            mapped,
            changed.compute_runtime_evaluation_allowance().unwrap()
        );
        changed.swap_map();
        assert!(changed.runtime_allowance.get().is_none());
        assert_eq!(changed.runtime_evaluation_allowance().unwrap(), mapped);

        let mut moved = definition.clone();
        moved
            .transform(
                &crate::geom::axis2_placement::Axis2Placement::IDENTITY,
                Some(Vec3::new(2.0, 2.0, 2.0)),
            )
            .unwrap();
        assert!(moved.runtime_allowance.get().is_none());
        let scaled = moved.runtime_evaluation_allowance().unwrap();
        assert!(scaled > original);
        assert_eq!(
            scaled,
            moved.compute_runtime_evaluation_allowance().unwrap()
        );
        assert_eq!(definition.runtime_evaluation_allowance().unwrap(), original);
    }

    /// The kept section and station-span constants are the values a fresh derivation
    /// gives, bit for bit, so evaluations with and without them agree exactly; an
    /// edit of the definition drops them with the allowance.
    #[test]
    fn kept_checked_values_are_the_fresh_ones() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.3, 0.7, 1.2, 1.25];
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.3).unwrap();
        let points = [
            Point2::new(0.37, 0.0),
            Point2::new(0.5, 0.3),
            Point2::new(0.91, 0.64),
            Point2::new(0.12, 1.249),
        ];
        let bits = |d: &ProceduralSweep| -> Vec<u64> {
            points
                .iter()
                .flat_map(|&uv| d.evaluate_definition(uv, 2, 2).unwrap().concat())
                .flat_map(|v| [v.x, v.y, v.z])
                .map(f64::to_bits)
                .collect()
        };
        let kept = bits(&definition);
        let spans = definition.checked_cache.spans.get().unwrap();
        assert_eq!(spans.len(), stations.len() - 1);
        for (index, pair) in stations.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let span = definition.span_turn(index, a, b).unwrap();
            assert_eq!(
                spans[index].get().copied().map(|s| s.tau_box),
                Some(span.tau_box)
            );
            assert_eq!(span.tau_box, span_tau_box(a, b));
            assert_eq!(
                span.tau.map(f64::to_bits),
                Some(span_tau(a, b).unwrap().to_bits())
            );
        }
        assert_eq!(
            definition.cached_screw_offset_section().unwrap().cloned(),
            definition.derive_screw_offset_section().unwrap()
        );
        // the same evaluations again from the kept values, and on a copy without them
        assert_eq!(bits(&definition), kept);
        let mut fresh = definition.clone();
        fresh.invalidate_runtime_allowance();
        assert!(fresh.checked_cache.spans.get().is_none());
        assert!(fresh.checked_cache.section.get().is_none());
        assert_eq!(bits(&fresh), kept);
        // an edit drops them
        let mut moved = definition.clone();
        moved.swap_map();
        assert!(moved.checked_cache.spans.get().is_none());
        assert!(moved.checked_cache.section.get().is_none());
    }

    /// The kept map powers are [`powi`]'s, bit for bit, on both scales and every order,
    /// refusals included; a map edited after they were formed gets its own powers.
    #[test]
    fn kept_map_powers_are_the_fresh_ones() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.3, 0.7, 1.2, 1.25];
        let mut definition = ProceduralSweep::create(&profile, motion, &stations, 0.3).unwrap();
        let same = |d: &ProceduralSweep| {
            for axis in 0..2 {
                for n in 0..=5 {
                    let kept = d.checked_map_power(axis, n).map(f64::to_bits);
                    let fresh = powi(d.map.b[axis], n).map(f64::to_bits);
                    assert_eq!(kept.ok(), fresh.ok(), "axis {axis}, n {n}");
                }
            }
        };
        same(&definition);
        assert!(definition.checked_map_power(2, 0).is_err());
        definition.reparametrize_map(true, 1e8, -2.0);
        same(&definition);
        // a scale changed in place, past the editors, is not met by the kept powers
        let kept = definition.checked_map_power(1, 3).unwrap();
        definition.map.b[1] *= 3.0;
        assert_ne!(definition.checked_map_power(1, 3).unwrap(), kept);
        same(&definition);
    }

    /// The kept turns and points are what the checks give afresh, bit for bit, at stations
    /// (where the side chooses the span), inside spans and past the ends; a point kept as
    /// passed does not pass another candidate at the same parameters; copies share what is kept
    /// until one is edited.
    #[test]
    fn kept_turns_and_points_are_the_fresh_ones() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = [0.0, 0.3, 0.7, 1.2, 1.25];
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.3).unwrap();
        let bits = |(turn, rotation): (Turn, Rotation)| {
            [
                turn.theta,
                turn.rate,
                turn.tau,
                rotation.sine,
                rotation.cosine,
            ]
            .map(f64::to_bits)
        };
        for t in [-1.0, 0.0, 0.17, 0.3, 0.5, 0.7, 1.2, 1.22, 1.25, 2.0] {
            for ending in [false, true] {
                let turn = definition.checked_turn_at_side(t, ending).unwrap();
                let fresh = bits((turn, Rotation::new(turn.theta).unwrap()));
                for _ in 0..2 {
                    let kept = definition.checked_turn_rotation(t, ending).unwrap();
                    assert_eq!(bits(kept), fresh, "t {t}, ending {ending}");
                }
            }
        }
        // at a station between spans of unequal widths the two sides differ
        let [left, right] =
            [true, false].map(|e| definition.checked_turn_rotation(0.3, e).unwrap());
        assert_ne!(bits(left), bits(right));

        let uv = Point2::new(0.37, 0.5);
        let point = definition.evaluate_definition(uv, 0, 0).unwrap()[0][0];
        definition.verify_runtime_point(uv, point).unwrap();
        definition.verify_runtime_point(uv, point).unwrap();
        assert!(definition
            .verify_runtime_point(uv, point + Vec3::X)
            .is_err());

        let mut copy = definition.clone();
        assert!(std::sync::Arc::ptr_eq(
            &copy.checked_cache,
            &definition.checked_cache
        ));
        copy.swap_map();
        assert!(!std::sync::Arc::ptr_eq(
            &copy.checked_cache,
            &definition.checked_cache
        ));
        assert!(definition.checked_cache.spans.get().is_some());
    }
}
