//! Calibration: which of a device's raw axes is which, and how far each one goes. The user says
//! what kind of device it is ([`Kind`]), holds it still, then moves it along each axis both ways; [`Recording::solve`] turns what the
//! device sent into a [`Calibration`] that maps any reading onto 3Dconnexion's axes (X right,
//! Y towards the user, Z down, the rotations about them right-handed), ±[`FULL_SCALE`] at full
//! deflection. Everything after that, the driver mapping and the user's per-axis choices, then
//! treats every device the same.

use crate::FULL_SCALE;

/// One motion the user is asked for: a canonical axis, one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// 0..3 = X, Y, Z; 3..6 = rotation about them.
    pub axis: usize,
    pub positive: bool,
}

/// What kind of device is being calibrated. It decides which motions are asked for and how a step
/// is recorded; the user picks it, and [`Wizard::mismatch`] says when the device doesn't behave
/// like the kind picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Kind {
    /// A cap on springs that comes back to the middle when let go: a reading says how far it is
    /// held out, and stays while it is held.
    #[default]
    ReturnToCenter,
    /// A ball, often with a scroll ring, that stays where it is left: a reading says how fast it
    /// moves, and goes back to rest as soon as it stops.
    Trackball,
}

impl Kind {
    /// Whether the steps ask about canonical axis `axis`. A trackball has nothing to tilt: rolling
    /// it is asked as sliding (rolled right is slid right, rolled toward you is pulled toward you),
    /// its scroll ring as Z and spinning it as the twist.
    pub fn asks(self, axis: usize) -> bool {
        match self {
            Kind::ReturnToCenter => axis < 6,
            Kind::Trackball => matches!(axis, 0 | 1 | 2 | 5),
        }
    }
}

/// [`AxisCalibration::source`] for a canonical axis the device doesn't have: it always reads 0.
/// A fixed width, so a calibration saved by a 64-bit build reads the same on wasm32 and back.
pub const NO_SOURCE: usize = u16::MAX as usize;

/// Reads a source; anything from [`NO_SOURCE`] up, as `usize::MAX` saved by earlier builds is, means none.
#[cfg(feature = "serde")]
fn source_that_reads<'de, D: serde::Deserializer<'de>>(saved: D) -> Result<usize, D::Error> {
    let saved = <u64 as serde::Deserialize>::deserialize(saved)?;
    Ok(usize::try_from(saved).map_or(NO_SOURCE, |source| source.min(NO_SOURCE)))
}

/// The motions, in the order they are asked for. The positive direction of each is what both
/// 3Dconnexion devices we measured report as positive: slide right, pull towards you, press down,
/// tilt the top towards you (the near edge down), tilt left, twist clockwise seen from above.
pub const STEPS: [Step; 12] = {
    let mut steps = [Step { axis: 0, positive: true }; 12];
    let mut i = 0;
    while i < 12 {
        steps[i] = Step { axis: i / 2, positive: i % 2 == 0 };
        i += 1;
    }
    steps
};

/// Where one canonical axis comes from, and how to scale it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AxisCalibration {
    /// The raw axis it is read from, or [`NO_SOURCE`].
    #[cfg_attr(feature = "serde", serde(deserialize_with = "source_that_reads"))]
    pub source: usize,
    /// The raw axis runs the other way.
    pub reversed: bool,
    /// The raw value with the cap at rest.
    pub rest: i16,
    /// How far from `rest` the value wanders with nobody touching the cap; readings within it are 0.
    pub dead_zone: u16,
    /// How far below and above `rest` the value goes at full deflection; each reads as
    /// ±[`FULL_SCALE`], so a device that goes further one way than the other still reaches full
    /// scale both ways.
    pub full: [u16; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Calibration {
    /// The user's dead zone on top of each axis's measured one, as a share of full deflection
    /// (up to [`MAX_DEAD_ZONE_SHARE`]): readings within it are 0, and the rest of the travel
    /// still reaches full scale. Widening it settles a device that drifts at rest, or whose rest
    /// calibrated off. Calibrations saved before it existed read as 0.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dead_zone_share: f32,
    /// What kind of device it calibrates. Calibrations saved before it existed are of caps.
    #[cfg_attr(feature = "serde", serde(default))]
    pub kind: Kind,
    pub axes: [AxisCalibration; 6],
}

/// How much of an axis's travel past its dead zone the response takes to come up to full rate,
/// so the view eases into motion at the dead zone's edge rather than jumping to speed. Kept
/// small: a steady reading close to the dead zone is dropped by [`Settle`] instead.
pub const FEATHER_SHARE: f64 = 0.05;

/// How far past the dead zone, as a share of full scale, a reading that has stopped changing
/// reads as no motion ([`Settle`]).
pub const SETTLE_SHARE: f64 = 0.2;
/// How long a reading has to stay put before [`Settle`] drops it.
pub const SETTLE_AFTER_SECONDS: f64 = 0.25;
/// How far a reading may wander, in calibrated units, and still count as not changing.
pub const SETTLE_WANDER: i16 = 3;

/// Drops readings that have stopped changing close to the dead zone: a hand leaning on the cap
/// pushes it a little and holds it there, which shouldn't creep the view, while a push meant to
/// move it keeps changing or goes further. Past [`SETTLE_SHARE`] a steady reading is a held push
/// and stays. Takes the calibrated readings ([`Calibration::apply`]) of a [`Kind::ReturnToCenter`]
/// device once a frame: a trackball's slow roll is a steady reading that is meant, and a device
/// without a calibration is read as it reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct Settle {
    last: [i16; 6],
    /// How long each axis has stayed within [`SETTLE_WANDER`] of `last`.
    steady: [f64; 6],
}

impl Settle {
    /// This frame's reading, `seconds` after the previous one, with the settled axes at 0.
    pub fn filter(&mut self, reading: [i16; 6], seconds: f64) -> [i16; 6] {
        let seconds = if seconds.is_finite() { seconds.clamp(0.0, 1.0) } else { 0.0 };
        let band = (SETTLE_SHARE * f64::from(FULL_SCALE)).round() as i32;
        std::array::from_fn(|axis| {
            let value = reading[axis];
            if (i32::from(value) - i32::from(self.last[axis])).abs() <= i32::from(SETTLE_WANDER) {
                self.steady[axis] += seconds;
            } else {
                (self.last[axis], self.steady[axis]) = (value, 0.0);
            }
            if i32::from(value).abs() <= band && self.steady[axis] >= SETTLE_AFTER_SECONDS { 0 } else { value }
        })
    }
}

/// The widest dead zone a calibration takes, as a share of full deflection.
pub const MAX_DEAD_ZONE_SHARE: f32 = 0.5;

impl Calibration {
    /// A reading in the device's raw axes, on the canonical ones. Past full deflection the value
    /// keeps growing, up to the i16 range; a source the device doesn't have reads 0. Just past the
    /// dead zone a cap's response feathers in ([`FEATHER_SHARE`]) instead of starting at full rate;
    /// a trackball's reads its speed straight, so a slow roll stays slow and in proportion.
    pub fn apply(&self, raw: [i16; 6]) -> [i16; 6] {
        let share = if self.dead_zone_share.is_finite() { self.dead_zone_share.clamp(0.0, MAX_DEAD_ZONE_SHARE) } else { 0.0 };
        let shared = (f64::from(share) * f64::from(FULL_SCALE)).round() as i32;
        self.axes.map(|axis| {
            let Some(&value) = raw.get(axis.source) else { return 0 };
            let offset = i32::from(value) - i32::from(axis.rest);
            let dead = i32::from(axis.dead_zone);
            let full = axis.full[usize::from(offset > 0)];
            let span = (i32::from(full) - dead).max(1);
            let past = offset.abs() - dead;
            if past <= 0 {
                return 0;
            }
            // In i64: a short calibrated range and an extreme reading go far past i32 here.
            let full_scale = i64::from(FULL_SCALE);
            let scaled = i64::from(past) * full_scale / i64::from(span);
            // The user's dead zone, in the same units; the travel past it stretches back to full scale.
            if scaled <= i64::from(shared) {
                return 0;
            }
            let scaled = (scaled - i64::from(shared)) * full_scale / (full_scale - i64::from(shared));
            // Feathered: over the first `knee` past the dead zone the response eases in (a
            // parabola from no slope to full slope), then carries on in a straight line, rescaled
            // so full deflection is still full scale. Even, so both pieces meet on a whole unit.
            let knee = if self.kind == Kind::Trackball { 0 } else { 2 * (FEATHER_SHARE * f64::from(FULL_SCALE) / 2.0).round() as i64 };
            let eased = if scaled < knee { scaled * scaled / (2 * knee) } else { scaled - knee / 2 };
            // Never 0 past the dead zone, so its edge stays where the motion starts.
            let scaled = (eased * full_scale / (full_scale - knee / 2)).clamp(1, i64::from(i16::MAX));
            let signed = if (offset < 0) != axis.reversed { -scaled } else { scaled };
            i16::try_from(signed).unwrap_or(0)
        })
    }

    /// How far each canonical axis is through its travel before any dead zone: ±1 at full
    /// deflection either way, further past it. Where a reading sits on a picture of the axis.
    pub fn travel(&self, raw: [i16; 6]) -> [f32; 6] {
        self.axes.map(|axis| {
            let Some(&value) = raw.get(axis.source) else { return 0.0 };
            let offset = f32::from(value) - f32::from(axis.rest);
            let share = offset / f32::from(axis.full[usize::from(offset > 0.0)]).max(1.0);
            if axis.reversed { -share } else { share }
        })
    }

    /// How much of each canonical axis's travel reads as no motion, `[negative, positive]` as
    /// shares of it: the measured dead zone with the user's on top, as [`Calibration::apply`] has it.
    pub fn dead_zone_shares(&self) -> [[f32; 2]; 6] {
        let user = if self.dead_zone_share.is_finite() { self.dead_zone_share.clamp(0.0, MAX_DEAD_ZONE_SHARE) } else { 0.0 };
        self.axes.map(|axis| {
            let [below, above] = axis.full.map(|full| (f32::from(axis.dead_zone) / f32::from(full).max(1.0)).min(1.0));
            let sides = if axis.reversed { [above, below] } else { [below, above] };
            sides.map(|measured| measured + (1.0 - measured) * user)
        })
    }

    /// Each raw axis's value at rest, which calibrates to no motion: what an app reads a raw axis
    /// as while it has nothing fresh from it. A raw axis no canonical one reads rests at 0.
    pub fn raw_rest(&self) -> [i16; 6] {
        let mut rest = [0; 6];
        for axis in &self.axes {
            if let Some(raw) = rest.get_mut(axis.source) {
                *raw = axis.rest;
            }
        }
        rest
    }

    /// Whether it can be used as it is: each canonical axis the device has reads its own raw axis,
    /// and goes further both ways at full deflection than it wanders at rest, and the device has
    /// at least one. Every calibration [`Recording::solve`] gives is; one from a hand-edited
    /// settings file may not be, and could turn any touch of the cap into full speed. Its dead
    /// zone has to be a share from 0 to [`MAX_DEAD_ZONE_SHARE`].
    pub fn is_usable(&self) -> bool {
        let mut used = [false; 6];
        (0.0..=MAX_DEAD_ZONE_SHARE).contains(&self.dead_zone_share)
            && self.axes.iter().any(|axis| axis.source != NO_SOURCE)
            && self.axes.iter().all(|axis| {
                axis.source == NO_SOURCE
                    || axis.full.iter().all(|&full| full > axis.dead_zone)
                        && used.get_mut(axis.source).is_some_and(|used| !std::mem::replace(used, true))
            })
    }
}

/// Why a recording didn't give a calibration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Nothing was recorded with the cap at rest.
    NoRest,
    /// Nothing was recorded for this step.
    NotRecorded(Step),
    /// Moving the cap along this canonical axis barely moved its raw axis one way: less than twice
    /// what it wanders at rest, or less than [`MIN_SHARE`] of how far any axis went.
    NoResponse(usize),
}

#[derive(Clone, Copy, Debug)]
struct Samples {
    sum: [f64; 6],
    count: u32,
    min: [i16; 6],
    max: [i16; 6],
}

impl Default for Samples {
    fn default() -> Self {
        Self { sum: [0.0; 6], count: 0, min: [i16::MAX; 6], max: [i16::MIN; 6] }
    }
}

impl Samples {
    fn add(&mut self, raw: [i16; 6]) {
        for (((sum, min), max), value) in self.sum.iter_mut().zip(&mut self.min).zip(&mut self.max).zip(raw) {
            *sum += f64::from(value);
            *min = (*min).min(value);
            *max = (*max).max(value);
        }
        self.count += 1;
    }

    fn mean(&self) -> Option<[f64; 6]> {
        (self.count > 0).then(|| self.sum.map(|sum| sum / f64::from(self.count)))
    }

    /// Where raw axis `r` rests (its mean, rounded) and how far its readings went from there;
    /// `None` without readings.
    fn rest_and_wander(&self, r: usize) -> Option<(f64, f64)> {
        let rest = self.mean()?.get(r)?.round();
        let (min, max) = (f64::from(*self.min.get(r)?), f64::from(*self.max.get(r)?));
        Some((rest, (max - rest).max(rest - min).max(0.0)))
    }

    /// How far apart raw axis `r`'s lowest and highest readings were; 0 without readings.
    fn spread(&self, r: usize) -> f64 {
        match (self.min.get(r), self.max.get(r)) {
            (Some(&min), Some(&max)) => (f64::from(max) - f64::from(min)).max(0.0),
            _ => 0.0,
        }
    }
}

/// What the device sent during calibration: readings at rest, with a hand resting on the cap,
/// during each of [`STEPS`], and where the cap came back to after each one.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    rest: Samples,
    /// Readings with a hand resting on the cap, not pushing it: what the dead zone has to hide.
    touch: Vec<[i16; 6]>,
    steps: [Samples; 12],
    /// The readings the cap settled on after step `n` was let go: all of a steady run.
    settled: [Samples; 12],
    /// The canonical axes the device doesn't have: not asked for, and calibrated to [`NO_SOURCE`].
    absent: [bool; 6],
    /// What kind of device it is, which the calibration carries.
    pub kind: Kind,
}

impl Recording {
    /// One reading with nobody touching the cap.
    pub fn rest(&mut self, raw: [i16; 6]) {
        self.rest.add(raw);
    }

    /// One reading with a hand resting on the cap.
    pub fn touch(&mut self, raw: [i16; 6]) {
        // A recording is a few seconds of frames; the cap keeps a wrong caller from growing it.
        if self.touch.len() < MAX_TOUCH_READINGS {
            self.touch.push(raw);
        }
    }

    /// One reading during step `index` of [`STEPS`]; any other index is ignored.
    pub fn step(&mut self, index: usize, raw: [i16; 6]) {
        if let Some(samples) = self.steps.get_mut(index) {
            samples.add(raw);
        }
    }

    /// One reading of where the cap settled after step `index` was let go; any other index is ignored.
    pub fn settled(&mut self, index: usize, raw: [i16; 6]) {
        if let Some(settled) = self.settled.get_mut(index) {
            settled.add(raw);
        }
    }

    /// How far raw axis `source` went from `rest` with a hand on the cap: the [`TOUCH_PERCENTILE`]th
    /// of its distances, so the jolt of a few frames doesn't count; 0 without readings.
    fn touch_wander(&self, source: usize, rest: f64) -> f64 {
        let mut away: Vec<f64> = self.touch.iter().filter_map(|raw| raw.get(source)).map(|&value| (f64::from(value) - rest).abs()).collect();
        away.sort_by(f64::total_cmp);
        let index = (away.len() as f64 * TOUCH_PERCENTILE).ceil() as usize;
        index.checked_sub(1).and_then(|index| away.get(index)).copied().unwrap_or(0.0)
    }

    /// Notes that the device doesn't have canonical axis `axis` (a trackball can't tilt, and not
    /// every one has a scroll ring), and forgets anything recorded for it; any other axis is ignored.
    pub fn absent(&mut self, axis: usize) {
        if let Some(absent) = self.absent.get_mut(axis) {
            *absent = true;
            self.clear_step(2 * axis);
            self.clear_step(2 * axis + 1);
        }
    }

    /// Whether the device has canonical axis `axis`, as far as the recording knows.
    pub fn has_axis(&self, axis: usize) -> bool {
        self.absent.get(axis).is_some_and(|absent| !absent)
    }

    /// Whether step `index` has a reading yet.
    pub fn has_step(&self, index: usize) -> bool {
        self.steps.get(index).is_some_and(|samples| samples.count > 0)
    }

    /// Forgets what was recorded for step `index`.
    fn clear_step(&mut self, index: usize) {
        if let Some(samples) = self.steps.get_mut(index) {
            *samples = Samples::default();
        }
        if let Some(settled) = self.settled.get_mut(index) {
            *settled = Samples::default();
        }
    }

    /// The calibration the recording gives. Hands are never exact: sliding the cap tilts it a
    /// little and tilting it slides it, sometimes further than the axis being asked for. So the
    /// raw axis for each canonical one isn't the one that moved most in its step. It is the
    /// one-to-one assignment that best explains all six axes together, scored by how far each
    /// raw axis moved between the step's two directions, as a share of the furthest it moved in
    /// any step: an axis with a wider range doesn't win through cross-talk alone. A raw axis that
    /// only wandered scores low, since its share is of at least four times its wander at rest.
    ///
    /// A device that doesn't come back to the same place after each release (a drifting stick, or
    /// spacenavd keeping a relative-axis device's last small report, #2778) rests, here, at the
    /// middle of everything it read at rest and after each release, and its dead zone covers all of
    /// it, so where it stops doesn't creep.
    ///
    /// A canonical axis the device doesn't have ([`Recording::absent`]) needs no steps, takes no
    /// raw axis from one it has, and reads nothing.
    pub fn solve(&self) -> Result<Calibration, Problem> {
        let rest = self.rest.mean().ok_or(Problem::NoRest)?;
        if self.absent.iter().all(|&absent| absent) {
            return Err(Problem::NotRecorded(STEPS[0]));
        }
        let mut means = [[0.0; 6]; 12];
        for ((mean, samples), step) in means.iter_mut().zip(&self.steps).zip(STEPS) {
            if self.has_axis(step.axis) {
                *mean = samples.mean().ok_or(Problem::NotRecorded(step))?;
            }
        }
        // travel[c][r]: how far raw axis r moved from canonical axis c's negative step to its positive one.
        let travel: [[f64; 6]; 6] = std::array::from_fn(|c| std::array::from_fn(|r| means[2 * c][r] - means[2 * c + 1][r]));
        let widest: [f64; 6] = std::array::from_fn(|r| travel.iter().map(|row| row[r].abs()).fold(4.0 * self.rest.spread(r), f64::max).max(1.0));
        let shares: [[f64; 6]; 6] = std::array::from_fn(|c| std::array::from_fn(|r| travel[c][r].abs() / widest[r]));
        let sources = best_assignment(&shares);
        let mut axes = [AxisCalibration { source: 0, reversed: false, rest: 0, dead_zone: 0, full: [0; 2] }; 6];
        for (c, (axis, source)) in axes.iter_mut().zip(sources).enumerate() {
            if !self.has_axis(c) {
                *axis = AxisCalibration { source: NO_SOURCE, reversed: false, rest: 0, dead_zone: 0, full: [1; 2] };
                continue;
            }
            let (rest_value, wander) = self.rest.rest_and_wander(source).unwrap_or((rest[source].round(), 0.0));
            let (rest_value, wander) = self.settled_rest(source, rest_value, wander);
            let (positive, negative) = (&self.steps[2 * c], &self.steps[2 * c + 1]);
            let reversed = travel[c][source] < 0.0;
            // The steps that push the raw axis above and below rest, and how far each got.
            let (above, below) = if reversed { (negative, positive) } else { (positive, negative) };
            let full = [rest_value - f64::from(below.min[source]), f64::from(above.max[source]) - rest_value];
            if full.iter().any(|&full| full < 2.0 * wander || full < 1.0) {
                return Err(Problem::NoResponse(c));
            }
            // A hand resting on the cap reads as no motion too, up to `MAX_TOUCH_SHARE` of the
            // shorter direction, so a hand that pushed doesn't hide the axis.
            let touched = self.touch_wander(source, rest_value);
            let wander = wander.max(touched.min(MAX_TOUCH_SHARE * full[0].min(full[1])));
            *axis = AxisCalibration {
                source,
                reversed,
                rest: rest_value.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16,
                dead_zone: wander.min(f64::from(u16::MAX)) as u16,
                full: full.map(|full| full.min(f64::from(u16::MAX)) as u16),
            };
        }
        // A trackball reads exactly 0 at rest, so it has no resting hand to turn away, and its
        // scroll ring clicks one unit at a time where the ball reports tens.
        let present = |axis: &&AxisCalibration| axis.source != NO_SOURCE;
        let most = axes.iter().filter(present).flat_map(|axis| axis.full).max().unwrap_or(0);
        if self.kind == Kind::ReturnToCenter
            && let Some(c) =
                axes.iter().position(|axis| axis.source != NO_SOURCE && axis.full.iter().any(|&full| f64::from(full) < MIN_SHARE * f64::from(most)))
        {
            return Err(Problem::NoResponse(c));
        }
        Ok(Calibration { dead_zone_share: 0.0, kind: self.kind, axes })
    }
}

impl Recording {
    /// Raw axis `r`'s rest and dead zone, given the rest stage's: unchanged while every reading
    /// after a release stayed within the wander; otherwise the middle of everything read at rest
    /// and after the releases, with a dead zone reaching every one of those readings.
    fn settled_rest(&self, r: usize, rest: f64, wander: f64) -> (f64, f64) {
        let (mut low, mut high) = (rest - wander, rest + wander);
        for settled in self.settled.iter().filter(|settled| settled.count > 0) {
            if let (Some(&min), Some(&max)) = (settled.min.get(r), settled.max.get(r)) {
                (low, high) = (low.min(f64::from(min)), high.max(f64::from(max)));
            }
        }
        if low >= rest - wander && high <= rest + wander {
            return (rest, wander);
        }
        let middle = ((low + high) / 2.0).round();
        (middle, (high - middle).max(middle - low))
    }
}

/// The raw axis for each canonical one, each raw axis used once, with the highest total share.
fn best_assignment(shares: &[[f64; 6]; 6]) -> [usize; 6] {
    fn search(shares: &[[f64; 6]; 6], canonical: usize, used: [bool; 6], current: &mut [usize; 6], total: f64, best: &mut ([usize; 6], f64)) {
        if canonical == 6 {
            if total > best.1 {
                *best = (*current, total);
            }
            return;
        }
        for raw in 0..6 {
            if !used[raw] {
                let mut now_used = used;
                now_used[raw] = true;
                current[canonical] = raw;
                search(shares, canonical + 1, now_used, current, total + shares[canonical][raw], best);
            }
        }
    }
    let mut best = ([0, 1, 2, 3, 4, 5], f64::NEG_INFINITY);
    search(shares, 0, [false; 6], &mut [0; 6], 0.0, &mut best);
    best.0
}

/// The least a direction may go, as a share of how far the furthest axis went, before it counts
/// as not having responded (on a [`Kind::ReturnToCenter`] device). It turns away a step that recorded a resting hand rather than a push,
/// which would otherwise scale a few units of noise up to full speed, and still lets a homemade
/// device go ten times further one way than the other.
pub const MIN_SHARE: f64 = 1.0 / 12.0;

/// Seconds of readings with the cap at rest before the first step.
pub const REST_SECONDS: f64 = 1.5;
/// Seconds of readings with a hand resting on the cap, after the rest.
pub const TOUCH_SECONDS: f64 = 2.5;
/// Seconds at the start of the touch stage that aren't recorded: a hand landing on the cap jolts it.
pub const TOUCH_LANDING_SECONDS: f64 = 0.5;
/// How much of the touch readings the dead zone has to hide, as a share of them; the rest is a jolt.
pub const TOUCH_PERCENTILE: f64 = 0.9;
/// The most touch readings a recording keeps.
const MAX_TOUCH_READINGS: usize = 10_000;
/// The most of an axis's travel a resting hand may take as dead zone.
pub const MAX_TOUCH_SHARE: f64 = 0.25;
/// Seconds the cap has to be held out in a step's direction before letting go finishes it.
pub const HOLD_SECONDS: f64 = 0.4;
/// Seconds the reading has to stay steady, back close to rest, to count as let go even outside the
/// rest wander: the cap stopped short of rest, as a drifting stick does, or spacenavd with a
/// relative-axis device (#2778). Back close to rest means the axis that went furthest this step is
/// at least [`RETURNED_SHARE`] of the way back, and some axis that moved about as far (at least
/// half as far) is within [`SETTLED_SHARE`] of its own peak: cross-talk can move another raw axis
/// further than the one being asked for, and which raw axis that is isn't known yet. Steady means
/// each raw axis's readings span no more than twice its rest wander, at least 4 units (±2) and
/// [`STEADY_SHARE`] of the furthest that axis has gone in any step: an axis that read flat at rest
/// may still dither once it's off-centre.
pub const SETTLE_SECONDS: f64 = 0.5;

/// How long without a fresh report before a steady reading may be the place a trackball stopped: reports
/// that stop coming mean the value is a kept one. Shorter than [`SETTLE_SECONDS`], longer than the gap
/// between a device's reports.
const REPORT_GAP_SECONDS: f64 = 0.25;
/// How far from rest, as a share of its peak this step, an axis that moved has to be to count as let go.
pub const SETTLED_SHARE: f64 = 0.25;
/// How far from rest, as a share of its peak, the axis that went furthest may still be when the
/// step counts as let go. A hand still holding the push keeps it out.
pub const RETURNED_SHARE: f64 = 0.5;
/// The least spread steady readings may have, as a share of the furthest the axis has gone.
pub const STEADY_SHARE: f64 = 1.0 / 50.0;

/// How long a trackball's reading has to stay exactly the same, far out, to look like a held cap.
pub const HELD_SECONDS: f64 = 1.5;

/// How many times in one step a cap that is let go too soon makes [`Wizard::mismatch`] suggest a
/// trackball: a ball's reading drops back to rest between strokes, so it can't be held out.
pub const RESTARTS_FOR_TRACKBALL: u32 = 3;

/// Where the guided calibration is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Which kind of device it is ([`Wizard::choose`]); nothing is recorded yet.
    Choose,
    /// Hands off the cap.
    Rest,
    /// A hand resting on the cap without pushing it, which sets the dead zone. Caps only: a
    /// trackball doesn't move under a resting hand.
    Touch,
    /// Step `n` of [`STEPS`].
    Step(usize),
    /// Every step is recorded.
    Done,
}

/// The guided calibration: it records the cap at rest, then each of [`STEPS`] in turn, and moves
/// on by itself, so the user's hand stays on the cap. A step is done once the cap has been held
/// out for [`HOLD_SECONDS`] in one go and let go. Before calibration nothing says how far a raw
/// axis goes, so "held out" only means some raw axis is at least twice as far from rest as it
/// wandered at rest, each axis measured against its own wander: the least [`Recording::solve`]
/// accepts. So whatever the solver would accept can be recorded, a noisy axis doesn't hold up a
/// quiet one, and the solver judges whether each direction went far enough. "Let go" means every
/// raw axis is back within its rest wander, or the reading has gone steady close to rest
/// ([`SETTLE_SECONDS`], [`SETTLED_SHARE`]).
///
/// A trackball can't be held out: its reading is a speed, back at rest whenever the ball stops,
/// between strokes and between the clicks of a scroll ring. So for a [`Kind::Trackball`] a step
/// adds up the time the reading is away from rest, pauses and all, and is done once that reaches
/// [`HOLD_SECONDS`] and the ball stops. Only the motions a trackball has are asked for, and any
/// step can be skipped ([`Wizard::skip`]) for a motion the device doesn't have.
#[derive(Clone, Debug)]
pub struct Wizard {
    stage: Stage,
    kind: Kind,
    recording: Recording,
    /// Seconds spent in the current stage: at rest, or held out.
    seconds: f64,
    /// The dead zone the user set while calibrating, which the result carries.
    dead_zone_share: f32,
    /// The readings since the reading went steady, and for how long.
    run: Samples,
    steady: f64,
    /// How far each raw axis went from rest in this step, in raw units.
    peaks: [f64; 6],
    /// The furthest each raw axis went from rest in any step so far.
    reach: [f64; 6],
    /// The readings the previous step settled on: while a reading stays within their spread, nobody has touched the cap.
    settled: Option<Samples>,
    /// How often this step was let go too soon.
    restarts: u32,
    /// What the device has done that the kind picked doesn't: a cap let go too soon again and
    /// again ([`RESTARTS_FOR_TRACKBALL`]), or a trackball's reading held steady far out.
    dropped: bool,
    held: bool,
    /// The last reading, and how long it has been exactly that.
    last: Option<[i16; 6]>,
    same: f64,
    /// Seconds since a fresh report was last fed (see [`Self::feed_reading`]).
    since_fresh: f64,
}

impl Default for Wizard {
    fn default() -> Self {
        Self {
            stage: Stage::Rest,
            kind: Kind::default(),
            recording: Recording::default(),
            seconds: 0.0,
            dead_zone_share: 0.0,
            run: Samples::default(),
            steady: 0.0,
            peaks: [0.0; 6],
            reach: [0.0; 6],
            settled: None,
            restarts: 0,
            dropped: false,
            held: false,
            last: None,
            same: 0.0,
            since_fresh: f64::INFINITY,
        }
    }
}

impl Wizard {
    /// A calibration of a `kind` of device, starting with it at rest.
    pub fn new(kind: Kind) -> Self {
        let mut wizard = Self { kind, ..Self::default() };
        wizard.recording.kind = kind;
        for axis in (0..6).filter(|&axis| !kind.asks(axis)) {
            wizard.recording.absent(axis);
        }
        wizard
    }

    /// A calibration that first asks which kind of device it is.
    pub fn choosing() -> Self {
        Self { stage: Stage::Choose, ..Self::default() }
    }

    /// Answers [`Stage::Choose`] (or switches kind and starts over, as [`Wizard::mismatch`]
    /// suggests); the dead zone set so far stays.
    pub fn choose(&mut self, kind: Kind) {
        let dead_zone_share = self.dead_zone_share;
        *self = Self { dead_zone_share, ..Self::new(kind) };
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Which of the steps asked for this device the current one is, from 1, and how many there are:
    /// 0 before the first, all of them once done.
    pub fn counter(&self) -> (usize, usize) {
        let asked = |index: &usize| STEPS.get(*index).is_some_and(|step| self.recording.has_axis(step.axis));
        let total = (0..STEPS.len()).filter(asked).count();
        match self.stage {
            Stage::Choose | Stage::Rest | Stage::Touch => (0, total),
            Stage::Step(index) => ((0..=index).filter(asked).count(), total),
            Stage::Done => (total, total),
        }
    }

    /// Whether the current step can be skipped: the device would still have a motion left.
    pub fn can_skip(&self) -> bool {
        matches!(self.stage, Stage::Step(_)) && (0..6).filter(|&axis| self.recording.has_axis(axis)).count() > 1
    }

    /// Skips the current step's motion, both ways: the device doesn't have it.
    pub fn skip(&mut self) {
        if let (true, Stage::Step(index)) = (self.can_skip(), self.stage)
            && let Some(step) = STEPS.get(index)
        {
            self.recording.absent(step.axis);
            let next = self.next_step(index + 1);
            self.enter(next);
            self.settled = None;
        }
    }

    /// The kind of device this one behaves like, when that isn't the kind picked.
    pub fn mismatch(&self) -> Option<Kind> {
        match self.kind {
            Kind::ReturnToCenter if self.dropped => Some(Kind::Trackball),
            Kind::Trackball if self.held => Some(Kind::ReturnToCenter),
            _ => None,
        }
    }

    /// How far through the current stage, 0..=1: the time at rest, or held out.
    pub fn progress(&self) -> f64 {
        match self.stage {
            Stage::Choose => 0.0,
            Stage::Rest => (self.seconds / REST_SECONDS).min(1.0),
            Stage::Touch => (self.seconds / TOUCH_SECONDS).min(1.0),
            Stage::Step(_) => (self.seconds / HOLD_SECONDS).min(1.0),
            Stage::Done => 1.0,
        }
    }

    /// One reading, `seconds` after the previous one, which may be a value kept since the last report
    /// (spacenavd sends only on change). See [`Self::feed_reading`].
    pub fn feed(&mut self, raw: [i16; 6], seconds: f64) {
        self.feed_reading(raw, seconds, false);
    }

    /// One reading, `seconds` after the previous one. `fresh` says a report or record arrived since the
    /// previous reading, even if its value is the same (a ball at one speed sends it over and over, and a
    /// daemon's scaling can make two records read alike); a last value kept with nothing arriving is not
    /// fresh. A trackball whose reports keep arriving (the latest less than a short gap ago) is still
    /// rolling, so a steady reading is never taken for where it came to rest, whatever fraction of its
    /// peak it is. Once nothing has arrived for the gap, a plateau below half the peak for the settle
    /// time is taken as where the ball stopped.
    pub fn feed_reading(&mut self, raw: [i16; 6], seconds: f64, fresh: bool) {
        self.since_fresh = if fresh { 0.0 } else { self.since_fresh + seconds.clamp(0.0, 0.1) };
        // A stalled frame shouldn't finish a stage on its own.
        let seconds = if seconds.is_finite() { seconds.clamp(0.0, 0.1) } else { 0.0 };
        // Steady while every axis's readings since the run began stay within its noise.
        let mut run = self.run;
        run.add(raw);
        if (0..6).all(|r| run.spread(r) <= self.tolerance(r)) {
            self.steady += seconds;
        } else {
            (run, self.steady) = (Samples::default(), 0.0);
            run.add(raw);
        }
        self.run = run;
        if self.last == Some(raw) {
            self.same += seconds;
        } else {
            (self.last, self.same) = (Some(raw), 0.0);
        }
        match self.stage {
            Stage::Choose => {}
            Stage::Rest => {
                self.recording.rest(raw);
                self.seconds += seconds;
                if self.seconds >= REST_SECONDS {
                    let next = match self.kind {
                        Kind::ReturnToCenter => Stage::Touch,
                        Kind::Trackball => self.next_step(0),
                    };
                    self.enter(next);
                }
            }
            Stage::Touch => {
                if self.seconds >= TOUCH_LANDING_SECONDS {
                    self.recording.touch(raw);
                }
                self.seconds += seconds;
                if self.seconds >= TOUCH_SECONDS {
                    let first = self.next_step(0);
                    self.enter(first);
                }
            }
            Stage::Step(index) => {
                // Still where the last step settled: the cap hasn't been touched yet. Deliberately
                // skips the peak and hold bookkeeping; a device that rests exactly there reads as
                // untouched until it moves, which is what it is.
                // A trackball's has to be exactly the same: a scroll ring clicks by less than a
                // cap's noise.
                let untouched = match self.kind {
                    Kind::ReturnToCenter => self.settled.is_some_and(|settled| self.near(settled, raw)),
                    Kind::Trackball => self.settled.is_some_and(|settled| settled.count > 0 && settled.min == raw && settled.max == raw),
                };
                if untouched {
                    return;
                }
                self.settled = None;
                let away = self.away(raw);
                let offsets = self.offsets(raw);
                for ((peak, reach), offset) in self.peaks.iter_mut().zip(&mut self.reach).zip(offsets) {
                    *peak = peak.max(offset);
                    *reach = reach.max(offset);
                }
                let furthest = self.peaks.iter().copied().fold(0.0, f64::max);
                if self.kind == Kind::Trackball {
                    self.roll(index, raw, away, offsets, furthest, seconds);
                    return;
                }
                let peaks = self.peaks;
                // Close to rest is the cap settling, never held; it has let go once it's steady there.
                // Every axis that moved about as far as the furthest has to be back by
                // `RETURNED_SHARE` of its own peak too, or a lesser axis still held out would be
                // missed while a larger cross-talk axis relaxes.
                let settling = (0..6).any(|r| peaks[r] == furthest && offsets[r] <= RETURNED_SHARE * furthest)
                    && (0..6).any(|r| peaks[r] >= 0.5 * furthest && offsets[r] <= SETTLED_SHARE * peaks[r])
                    && (0..6).all(|r| peaks[r] < 0.5 * furthest || offsets[r] <= RETURNED_SHARE * peaks[r]);
                let stopped = settling && (self.steady >= SETTLE_SECONDS);
                if away <= 0.5 || stopped {
                    // Where it settled: every reading of the steady run, or just this one.
                    let mut settled = Samples::default();
                    settled.add(raw);
                    if stopped {
                        settled = self.run;
                    }
                    if self.seconds >= HOLD_SECONDS {
                        // Every reading of the steady run, so the dead zone covers all of them.
                        if stopped {
                            self.recording.settled[index] = self.run;
                        } else {
                            self.recording.settled(index, raw);
                        }
                        let next = self.next_step(index + 1);
                        self.enter(next);
                        self.settled = Some(settled);
                    } else if self.seconds > 0.0 {
                        // Let go too soon: the hold starts over, and only a whole one is kept.
                        self.recording.clear_step(index);
                        self.enter(Stage::Step(index));
                        self.settled = Some(settled);
                        self.restarts += 1;
                        self.dropped |= self.restarts >= RESTARTS_FOR_TRACKBALL;
                    }
                } else if away >= 1.0 && !settling {
                    self.recording.step(index, raw);
                    self.seconds += seconds;
                }
            }
            Stage::Done => {}
        }
    }

    /// Goes back one step and records it again; from the first step, back to rest, and from rest
    /// back to choosing the kind. A skipped motion stays skipped.
    pub fn back(&mut self) {
        let current = match self.stage {
            Stage::Choose => return,
            Stage::Rest => {
                let dead_zone_share = self.dead_zone_share;
                *self = Self { dead_zone_share, ..Self::choosing() };
                return;
            }
            Stage::Touch => 0,
            Stage::Step(index) => index,
            Stage::Done => STEPS.len(),
        };
        let asked = |index: &usize| STEPS.get(*index).is_some_and(|step| self.recording.has_axis(step.axis));
        match (0..current).rev().find(asked) {
            Some(previous) => {
                self.recording.clear_step(current);
                self.recording.clear_step(previous);
                self.enter(Stage::Step(previous));
            }
            None => {
                let (kind, dead_zone_share, absent) = (self.kind, self.dead_zone_share, self.recording.absent);
                *self = Self { dead_zone_share, ..Self::new(kind) };
                for axis in (0..6).filter(|&axis| absent[axis]) {
                    self.recording.absent(axis);
                }
            }
        }
    }

    /// Once every step is recorded, the calibration they give, with the dead zone set here.
    pub fn result(&self) -> Option<Result<Calibration, Problem>> {
        (self.stage == Stage::Done)
            .then(|| self.recording.solve().map(|calibration| Calibration { dead_zone_share: self.dead_zone_share, ..calibration }))
    }

    pub fn dead_zone_share(&self) -> f32 {
        self.dead_zone_share
    }

    /// Sets the dead zone the result carries, a share from 0 to [`MAX_DEAD_ZONE_SHARE`]; anything
    /// else is held to that range, and a value that isn't a number reads as 0.
    pub fn set_dead_zone_share(&mut self, share: f32) {
        self.dead_zone_share = if share.is_finite() { share.clamp(0.0, MAX_DEAD_ZONE_SHARE) } else { 0.0 };
    }

    fn enter(&mut self, stage: Stage) {
        if stage != self.stage {
            self.restarts = 0;
        }
        self.stage = stage;
        self.seconds = 0.0;
        self.peaks = [0.0; 6];
        // A new stage's steadiness starts with its own readings.
        (self.run, self.steady) = (Samples::default(), 0.0);
    }

    /// The first step from `index` on that asks about a motion the device has, or done.
    fn next_step(&self, index: usize) -> Stage {
        (index..STEPS.len()).find(|&i| STEPS.get(i).is_some_and(|step| self.recording.has_axis(step.axis))).map_or(Stage::Done, Stage::Step)
    }

    /// One reading of a trackball's step: the time away from rest adds up, pauses and all, and the
    /// step is done once it reaches [`HOLD_SECONDS`] and the ball has stopped. Stopped means back
    /// at rest, or exactly the same reading for [`SETTLE_SECONDS`] after slowing to under half the
    /// step's peak: spacenavd keeps a relative device's last report (#2778), which isn't recorded.
    /// Exactly, not within the noise a cap's steadiness allows: a scroll ring's clicks are smaller
    /// than that. The same reading for [`HELD_SECONDS`] without slowing down is a cap being held,
    /// which [`Wizard::mismatch`] reports; the step stays open (a skip or a switch of kind moves on).
    fn roll(&mut self, index: usize, raw: [i16; 6], away: f64, offsets: [f64; 6], furthest: f64, seconds: f64) {
        let stuck = self.same >= SETTLE_SECONDS;
        let high = away >= 1.0 && offsets.iter().copied().fold(0.0, f64::max) >= 0.5 * furthest;
        self.held |= high && self.same >= HELD_SECONDS;
        // A still-high reading is never a place the ball came to rest: a quantized ball can send the
        // same speed while it rolls. Held steady far out only makes `mismatch` suggest a cap.
        let stopped = away <= 0.5 || (stuck && !high && self.since_fresh >= REPORT_GAP_SECONDS);
        if away >= 1.0 && !stuck {
            self.recording.step(index, raw);
            self.seconds += seconds;
        } else if self.seconds >= HOLD_SECONDS && stopped {
            // Stopped exactly here, so this one reading is where it settled.
            let mut settled = Samples::default();
            settled.add(raw);
            self.recording.settled[index] = settled;
            let next = self.next_step(index + 1);
            self.enter(next);
            self.settled = Some(settled);
        }
    }

    /// How far each raw axis is from rest, in raw units.
    fn offsets(&self, raw: [i16; 6]) -> [f64; 6] {
        std::array::from_fn(|r| (f64::from(raw[r]) - self.recording.rest.rest_and_wander(r).map_or(0.0, |(rest, _)| rest)).abs())
    }

    /// How far apart raw axis `r`'s readings may be and still be one position: its noise's whole
    /// spread (twice its rest wander), at least 4 units (±2) and [`STEADY_SHARE`] of the furthest it
    /// has gone in any step.
    fn tolerance(&self, r: usize) -> f64 {
        let wander = self.recording.rest.rest_and_wander(r).map_or(0.0, |(_, wander)| wander);
        (2.0 * wander).max(4.0).max(STEADY_SHARE * self.reach.get(r).copied().unwrap_or(0.0))
    }

    /// Whether `raw` is still where `settled` was: adding it keeps every axis within [`Self::tolerance`],
    /// the same band the steadiness check accepts.
    fn near(&self, mut settled: Samples, raw: [i16; 6]) -> bool {
        settled.add(raw);
        (0..6).all(|r| settled.spread(r) <= self.tolerance(r))
    }

    /// How far the reading is from rest, on the raw axis furthest out, as a share of what counts as
    /// held out for that axis (twice its rest wander, at least one unit): 1 or more is held out,
    /// half or less is back within its wander.
    fn away(&self, raw: [i16; 6]) -> f64 {
        raw.iter()
            .enumerate()
            .map(|(r, &value)| {
                let (rest, wander) = self.recording.rest.rest_and_wander(r).unwrap_or((0.0, 0.0));
                (f64::from(value) - rest).abs() / (2.0 * wander).max(1.0)
            })
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mean of each axis (X Y Z Rx Ry Rz) during each step, in `STEPS` order, as measured
    /// with our own SpaceMouse Pro (046d:c62b) following capture prompts. Note the slide right
    /// row: Ry moved further than X.
    const PRO: [[i16; 6]; 12] = [
        [250, -10, 0, 0, -336, 0],     // slide right
        [-277, 55, 86, -36, -196, 0],  // slide left
        [0, 324, 82, -26, -3, -28],    // pull towards you
        [-2, -340, 219, -26, -20, 0],  // push away
        [-1, -3, 344, 0, -37, 0],      // press down
        [41, -24, -341, -11, -10, 32], // lift up
        [0, 193, 0, 333, -26, 0],      // tilt the top towards you
        [0, -339, 163, -183, 0, 0],    // tilt the top away
        [61, 0, 1, -12, 328, 0],       // tilt left
        [9, 80, 1, 0, -330, -6],       // tilt right
        [0, -1, 78, 0, 0, 341],        // twist clockwise
        [0, -6, 0, 0, -4, -337],       // twist counterclockwise
    ];

    /// The same with our own SpaceMouse Wireless (256f:c63a), all six axes in one report.
    const WIRELESS: [[i16; 6]; 12] = [
        [325, 92, 72, 0, 44, -6],
        [-336, 38, 104, 19, -66, 5],
        [-5, 339, 73, -71, 0, 0],
        [-18, -323, 144, -2, 1, 0],
        [0, 1, 341, -2, 2, 0],
        [0, 1, -320, 1, -43, 0],
        [0, -12, 3, 330, 4, -24],
        [0, 51, 17, -319, -20, 0],
        [-23, 0, 0, -1, 333, 0],
        [7, 41, 23, 15, -163, -13],
        [-13, 4, 44, -12, -12, 326],
        [0, -1, -1, 1, -9, -313],
    ];

    fn record(steps: [[i16; 6]; 12], rest: [[i16; 6]; 2]) -> Recording {
        let mut recording = Recording::default();
        for reading in rest {
            recording.rest(reading);
        }
        for (index, reading) in steps.into_iter().enumerate() {
            recording.step(index, reading);
        }
        recording
    }

    const STILL: [[i16; 6]; 2] = [[-3, 2, 0, 1, -2, 0], [3, -2, 0, -1, 2, 0]];

    #[test]
    fn both_3dconnexion_devices_calibrate_to_their_own_axes() {
        for steps in [PRO, WIRELESS] {
            let calibration = record(steps, STILL).solve().unwrap();
            for (c, axis) in calibration.axes.iter().enumerate() {
                assert_eq!((axis.source, axis.reversed), (c, false), "axis {c}");
                assert!(axis.full.iter().all(|full| (150..=350).contains(full)), "axis {c}: {axis:?}");
            }
        }
    }

    #[test]
    fn a_homemade_device_with_shuffled_reversed_wider_axes_is_found() {
        // A device reporting ±32767, its axes in another order (raw r carries canonical
        // ORDER[r]), the odd ones running the other way, at rest around 100.
        const ORDER: [usize; 6] = [4, 0, 5, 2, 1, 3];
        let wire = |canonical: [i16; 6]| -> [i16; 6] {
            std::array::from_fn(|r| {
                let v = i32::from(canonical[ORDER[r]]) * 32767 / 350;
                i16::try_from(if r % 2 == 1 { -v } else { v } + 100).unwrap()
            })
        };
        let calibration = record(PRO.map(wire), STILL.map(wire)).solve().unwrap();
        for (c, axis) in calibration.axes.iter().enumerate() {
            let r = ORDER.iter().position(|&o| o == c).unwrap();
            assert_eq!((axis.source, axis.reversed), (r, r % 2 == 1), "axis {c}");
            assert_eq!(axis.rest, 100);
        }
        // Each step now reads on its own axis, the right way, at full scale: each direction is
        // scaled on its own, so the Pro's shorter tilt away gets there too.
        for (step, reading) in STEPS.iter().zip(PRO.map(wire)) {
            let value = calibration.apply(reading)[step.axis];
            assert_eq!(value, if step.positive { FULL_SCALE } else { -FULL_SCALE }, "{step:?}");
        }
    }

    #[test]
    fn readings_within_the_rest_wander_are_zero_and_full_deflection_is_full_scale() {
        let axis = AxisCalibration { source: 1, reversed: true, rest: 10, dead_zone: 5, full: [105; 2] };
        let calibration = Calibration { dead_zone_share: 0.0, kind: Kind::default(), axes: [axis; 6] };
        assert_eq!(calibration.apply([0, 14, 0, 0, 0, 0]), [0; 6]);
        assert_eq!(calibration.apply([0, 115, 0, 0, 0, 0]), [-FULL_SCALE; 6]);
        assert_eq!(calibration.apply([0, -95, 0, 0, 0, 0]), [FULL_SCALE; 6]);
        // Past full deflection the value keeps going, up to the i16 range.
        assert_eq!(calibration.apply([0, i16::MIN, 0, 0, 0, 0]), [i16::MAX; 6]);
    }

    #[test]
    fn a_device_that_goes_further_one_way_reaches_full_scale_both_ways() {
        let axis = AxisCalibration { source: 0, reversed: false, rest: 0, dead_zone: 0, full: [200, 500] };
        let calibration = Calibration { dead_zone_share: 0.0, kind: Kind::default(), axes: [axis; 6] };
        assert_eq!(calibration.apply([500, 0, 0, 0, 0, 0]), [FULL_SCALE; 6]);
        assert_eq!(calibration.apply([-200, 0, 0, 0, 0, 0]), [-FULL_SCALE; 6]);
        // Halfway reads a little under half: the first stretch past rest feathers in.
        assert_eq!(calibration.apply([-100, 0, 0, 0, 0, 0]), [-170; 6]);
    }

    #[test]
    fn the_users_dead_zone_comes_off_the_bottom_and_full_deflection_stays_full_scale() {
        let axis = AxisCalibration { source: 0, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] };
        let calibration = Calibration { dead_zone_share: 0.2, kind: Kind::default(), axes: [axis; 6] };
        assert_eq!(calibration.apply([70, 0, 0, 0, 0, 0])[0], 0, "a fifth of the travel is inside it");
        assert_eq!(calibration.apply([210, 0, 0, 0, 0, 0])[0], 170, "60% of the travel is halfway past it, feathered");
        assert_eq!(calibration.apply([-350, 0, 0, 0, 0, 0])[0], -FULL_SCALE);
        // Out of range or not a number: held to the range, or none.
        assert_eq!(Calibration { dead_zone_share: 9.0, ..calibration }.apply([170, 0, 0, 0, 0, 0])[0], 0);
        assert_eq!(Calibration { dead_zone_share: f32::NAN, ..calibration }.apply([70, 0, 0, 0, 0, 0])[0], 62);
        let solved = record(PRO, STILL).solve().unwrap();
        assert!(Calibration { dead_zone_share: 0.5, ..solved }.is_usable());
        assert!(!Calibration { dead_zone_share: 0.6, ..solved }.is_usable());
        assert!(!Calibration { dead_zone_share: f32::NAN, ..solved }.is_usable());
        assert!(!Calibration { dead_zone_share: -0.1, ..solved }.is_usable());
    }

    #[test]
    fn extreme_readings_on_a_short_range_saturate_with_the_right_sign() {
        // Six distinct sources, each with a range of one unit: any real reading is far past it.
        let axes: [AxisCalibration; 6] = std::array::from_fn(|c| AxisCalibration {
            source: (c + 2) % 6,
            reversed: c % 2 == 1,
            rest: if c < 3 { 0 } else { i16::MIN },
            dead_zone: 0,
            full: [1, 1],
        });
        for dead_zone_share in [0.0, MAX_DEAD_ZONE_SHARE] {
            let calibration = Calibration { dead_zone_share, kind: Kind::default(), axes };
            assert!(calibration.is_usable());
            let read = |raw: [i16; 6]| calibration.apply(raw);
            for (raw, sign) in [(i16::MAX, 1), (i16::MIN, -1)] {
                let out = read([raw; 6]);
                for (c, axis) in calibration.axes.iter().enumerate() {
                    let offset = i32::from(raw) - i32::from(axis.rest);
                    let expected = if offset == 0 {
                        0
                    } else {
                        let positive = (offset > 0) != axis.reversed;
                        if positive { i16::MAX } else { -i16::MAX }
                    };
                    assert_eq!(out[c], expected, "share {dead_zone_share}, raw {raw} (sign {sign}), axis {c}: {axis:?}");
                }
            }
        }
    }

    #[test]
    fn the_dead_zone_set_while_calibrating_goes_with_the_result() {
        let mut wizard = Wizard::default();
        wizard.set_dead_zone_share(0.15);
        rest_then(&mut wizard);
        for reading in PRO {
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, STILL[0], 0.1);
        }
        assert_eq!(wizard.result().unwrap().unwrap().dead_zone_share, 0.15);
        wizard.set_dead_zone_share(f32::NAN);
        assert_eq!(wizard.dead_zone_share(), 0.0);
        wizard.set_dead_zone_share(2.0);
        assert_eq!(wizard.dead_zone_share(), MAX_DEAD_ZONE_SHARE);
    }

    #[test]
    fn a_device_at_its_raw_rest_reads_no_motion() {
        let axis = |source, rest| AxisCalibration { source, reversed: source % 2 == 1, rest, dead_zone: 0, full: [350; 2] };
        let calibration = Calibration {
            dead_zone_share: 0.0,
            kind: Kind::default(),
            axes: [axis(3, 100), axis(0, -40), axis(1, 7), axis(2, 0), axis(5, 1000), axis(4, -1)],
        };
        assert_eq!(calibration.raw_rest(), [-40, 7, 0, 100, -1, 1000]);
        assert_eq!(calibration.apply(calibration.raw_rest()), [0; 6]);
    }

    #[test]
    fn a_source_the_device_doesnt_have_reads_zero() {
        let axis = AxisCalibration { source: 9, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] };
        assert_eq!(Calibration { dead_zone_share: 0.0, kind: Kind::default(), axes: [axis; 6] }.apply([350; 6]), [0; 6]);
    }

    #[test]
    fn a_missing_step_or_rest_is_named() {
        let mut recording = record(PRO, STILL);
        recording.steps[7] = Samples::default();
        assert_eq!(recording.solve(), Err(Problem::NotRecorded(STEPS[7])));
        assert!(!recording.has_step(7) && recording.has_step(6));
        assert_eq!(Recording::default().solve(), Err(Problem::NoRest));
    }

    #[test]
    fn an_axis_that_barely_moves_is_named() {
        let mut steps = PRO;
        // Twisting did nothing beyond what the cap wanders at rest.
        steps[10] = [0, 0, 0, 0, 0, 20];
        steps[11] = [0, 0, 0, 0, 0, -20];
        assert_eq!(record(steps, STILL).solve(), Err(Problem::NoResponse(5)));
    }

    /// Feeds `reading` for `seconds`, a frame every 20 ms.
    fn hold(wizard: &mut Wizard, reading: [i16; 6], seconds: f64) {
        for _ in 0..(seconds / 0.02).round() as usize {
            wizard.feed(reading, 0.02);
        }
    }

    /// As `hold`, each reading flagged fresh or not.
    fn hold_fresh(wizard: &mut Wizard, reading: [i16; 6], seconds: f64, fresh: bool) {
        for _ in 0..(seconds / 0.02).round() as usize {
            wizard.feed_reading(reading, 0.02, fresh);
        }
    }

    /// The rest stage and, for a cap, the touch stage, the cap only jittering.
    fn rest_then(wizard: &mut Wizard) {
        for _ in 0..105 {
            hold(wizard, STILL[0], 0.02);
            hold(wizard, STILL[1], 0.02);
        }
    }

    #[test]
    fn the_wizard_moves_on_by_itself_and_ends_with_the_calibration() {
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for (index, reading) in PRO.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            hold(&mut wizard, reading, 0.5);
            assert_eq!(wizard.progress(), 1.0);
            hold(&mut wizard, STILL[0], 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Done);
        let calibration = wizard.result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed));
    }

    #[test]
    fn a_quick_twitch_or_a_hold_without_letting_go_doesnt_finish_a_step() {
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        hold(&mut wizard, PRO[0], 0.2);
        hold(&mut wizard, STILL[0], 0.1);
        assert_eq!(wizard.stage(), Stage::Step(0));
        hold(&mut wizard, PRO[0], 2.0);
        assert_eq!(wizard.stage(), Stage::Step(0));
        // A stalled frame counts as a short one.
        let mut stalled = Wizard::default();
        rest_then(&mut stalled);
        stalled.feed(PRO[0], 5.0);
        assert!(stalled.progress() < 1.0);
    }

    #[test]
    fn back_records_the_previous_step_again() {
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for reading in &PRO[..2] {
            hold(&mut wizard, *reading, 0.5);
            hold(&mut wizard, STILL[0], 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Step(2));
        wizard.back();
        assert_eq!(wizard.stage(), Stage::Step(1));
        assert!(wizard.recording.has_step(0) && !wizard.recording.has_step(1));
        wizard.back();
        wizard.back();
        assert_eq!(wizard.stage(), Stage::Rest);
        assert!(wizard.result().is_none());
    }

    #[test]
    fn a_device_whose_rotations_go_a_fifth_as_far_as_its_slides_finishes_and_calibrates() {
        let weak = |reading: [i16; 6]| -> [i16; 6] { std::array::from_fn(|r| if r < 3 { reading[r] } else { reading[r] / 5 }) };
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for (index, reading) in PRO.map(weak).into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, STILL[0], 0.1);
        }
        let calibration = wizard.result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed), "{calibration:?}");
    }

    #[test]
    fn two_short_nudges_dont_add_up_to_a_hold() {
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for _ in 0..2 {
            hold(&mut wizard, PRO[0], 0.24);
            hold(&mut wizard, STILL[0], 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Step(0));
        assert_eq!(wizard.progress(), 0.0);
        assert!(!wizard.recording.has_step(0));
    }

    #[test]
    fn a_device_that_goes_ten_times_further_one_way_finishes_and_calibrates() {
        // Slide right goes to +500, slide left only to -50; the other axes as the Pro.
        let mut steps = PRO;
        steps[0][0] = 500;
        steps[1][0] = -50;
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for (index, reading) in steps.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, STILL[0], 0.1);
        }
        let calibration = wizard.result().unwrap().unwrap();
        assert_eq!((calibration.axes[0].source, calibration.axes[0].full), (0, [50, 500]));
        assert_eq!(calibration.apply([-50, 0, 0, 0, 0, 0])[0], -FULL_SCALE);
    }

    /// Runs the whole wizard: `rest` alternating for the rest stage, then each step held and let
    /// go back to `rest[0]`.
    fn walk(rest: [[i16; 6]; 2], steps: [[i16; 6]; 12]) -> Wizard {
        let mut wizard = Wizard::default();
        for _ in 0..105 {
            hold(&mut wizard, rest[0], 0.02);
            hold(&mut wizard, rest[1], 0.02);
        }
        for (index, reading) in steps.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index), "{rest:?}");
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, rest[0], 0.1);
        }
        wizard
    }

    /// Where a relative-axis device read through spacenavd stops after each release: its last
    /// small report, never zero (#2778). Ours, in the range that issue measured.
    const STUCK: [[i16; 6]; 13] = [
        [11, -34, -40, -5, -5, 15],
        [0, 0, -60, 0, 0, 0],
        [-8, 21, 13, 4, -9, -3],
        [25, -6, 0, -12, 7, 2],
        [-30, 12, 9, 0, 18, -7],
        [4, 38, -15, 6, -2, 0],
        [0, -17, 33, -20, 1, 9],
        [9, 3, -4, 28, -11, 0],
        [-2, -9, 6, -3, 24, -12],
        [14, 0, -11, 7, -30, 5],
        [-6, 10, 2, 0, 3, 31],
        [3, -4, 0, 2, -6, -27],
        [0, 0, 0, 0, 0, 0],
    ];

    #[test]
    fn a_device_that_stops_short_of_rest_after_each_release_finishes_and_doesnt_creep() {
        // Nothing arrives at rest, so the reading stays exactly where the last report left it:
        // first wherever the cap was last let go, then after each step a different place.
        let mut wizard = Wizard::default();
        hold(&mut wizard, STUCK[0], REST_SECONDS + TOUCH_SECONDS + 0.1);
        for (index, reading) in PRO.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            // While the cap hasn't been touched, the last step's stuck reading isn't a hold.
            hold(&mut wizard, STUCK[index], 1.0);
            assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(index), 0.0));
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, STUCK[index + 1], SETTLE_SECONDS + 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Done);
        let calibration = wizard.result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed), "{calibration:?}");
        for stuck in STUCK {
            assert_eq!(calibration.apply(stuck), [0; 6], "{stuck:?} {calibration:?}");
        }
        // The cap still reaches full speed, and a push past the stuck range still moves.
        assert!(calibration.apply(PRO[0])[0] >= FULL_SCALE - 1, "{calibration:?}");
        assert!(calibration.apply([120, 0, 0, 0, 0, 0])[0] > 0);
    }

    #[test]
    fn a_steady_hold_that_repeats_one_reading_is_still_a_hold() {
        // spacenavd sends only on change, so a cap held still at full deflection repeats one
        // reading for far longer than SETTLE_SECONDS; it is held, not let go.
        let mut wizard = Wizard::default();
        hold(&mut wizard, STUCK[0], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, PRO[0], 1.0);
        assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(0), 1.0));
        hold(&mut wizard, STUCK[1], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
    }

    /// `reading` and `reading + jitter` in turn, for `seconds`, a frame every 20 ms.
    fn jitter(wizard: &mut Wizard, reading: [i16; 6], jitter: [i16; 6], seconds: f64) {
        let shaken: [i16; 6] = std::array::from_fn(|r| reading[r] + jitter[r]);
        for frame in 0..(seconds / 0.02).round() as usize {
            wizard.feed(if frame % 2 == 0 { reading } else { shaken }, 0.02);
        }
    }

    #[test]
    fn a_drifting_device_that_settles_off_centre_after_each_release_finishes_and_doesnt_creep() {
        // It keeps reporting, noisily, but after each release settles somewhere off its rest:
        // a worn stick, not only spacenavd. The noise stays within its rest wander (`STILL`).
        const NOISE: [i16; 6] = [3, -2, 0, 1, 2, 0];
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        for (index, reading) in PRO.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            if index > 0 {
                // Sitting where the last step settled isn't a hold.
                jitter(&mut wizard, STUCK[index], NOISE, 1.0);
                assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(index), 0.0));
            }
            hold(&mut wizard, reading, 0.5);
            jitter(&mut wizard, STUCK[index + 1], NOISE, SETTLE_SECONDS + 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Done);
        let calibration = wizard.result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed), "{calibration:?}");
        for stuck in &STUCK[1..] {
            let shaken: [i16; 6] = std::array::from_fn(|r| stuck[r] + NOISE[r]);
            assert_eq!((calibration.apply(*stuck), calibration.apply(shaken)), ([0; 6], [0; 6]), "{stuck:?} {calibration:?}");
        }
        assert!(calibration.apply(PRO[0])[0] >= FULL_SCALE - 1, "{calibration:?}");
    }

    #[test]
    fn both_ends_of_an_odd_width_settled_run_are_untouched_and_one_unit_beyond_is_movement() {
        // Silent at rest (no wander) and a 250 push on X: tolerance is max(4, 250 / 50) = 5, odd.
        // The release dithers over 10..15, as wide as that, so its middle rounds to 13 and 10 is
        // 3 from it, more than half of 5.
        let wizard = || {
            let mut wizard = Wizard::default();
            hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
            hold(&mut wizard, [250, 0, 0, 0, 0, 0], 0.5);
            cycle(&mut wizard, &[[10, 0, 0, 0, 0, 0], [15, 0, 0, 0, 0, 0]], SETTLE_SECONDS + 0.1);
            assert_eq!(wizard.stage(), Stage::Step(1));
            wizard
        };
        for (x, touched) in [(10, false), (15, false), (9, true), (16, true)] {
            let mut wizard = wizard();
            assert_eq!(wizard.tolerance(0), 5.0);
            wizard.feed([x, 0, 0, 0, 0, 0], 0.02);
            assert_eq!(wizard.progress() > 0.0, touched, "{x}");
        }
    }

    #[test]
    fn a_reading_still_moving_close_to_rest_doesnt_end_a_step() {
        // Past the peak and close to rest, but moving more than the noise: still on the way back.
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        hold(&mut wizard, PRO[0], 0.5);
        jitter(&mut wizard, PRO[0].map(|value| value / 5), [20, 0, 0, 0, 0, 0], 1.0);
        assert_eq!(wizard.stage(), Stage::Step(0));
    }

    /// `readings` in turn, from the first, for `seconds`, a frame every 20 ms.
    fn cycle(wizard: &mut Wizard, readings: &[[i16; 6]], seconds: f64) {
        for frame in 0..(seconds / 0.02).round() as usize {
            wizard.feed(readings[frame % readings.len()], 0.02);
        }
    }

    #[test]
    fn an_axis_flat_at_rest_that_dithers_once_off_centre_still_settles() {
        // Silent at rest, so no wander was measured, but ±2 around where each release stops.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        let dither = |at: [i16; 6]| [at.map(|value| value - 2), at, at.map(|value| value + 2)];
        for (index, reading) in PRO.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            if index > 0 {
                // Dithering where the last step settled doesn't fill this one.
                cycle(&mut wizard, &dither(STUCK[index]), 1.0);
                assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(index), 0.0));
            }
            hold(&mut wizard, reading, 0.5);
            cycle(&mut wizard, &dither(STUCK[index + 1]), SETTLE_SECONDS + 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Done);
        let calibration = wizard.result().unwrap().unwrap();
        for at in &STUCK[1..] {
            for still in dither(*at) {
                assert_eq!(calibration.apply(still), [0; 6], "{still:?} {calibration:?}");
            }
        }
    }

    #[test]
    fn noise_that_starts_at_one_edge_of_its_spread_is_steady_and_settles_at_its_middle() {
        // X wanders -3..+3 at rest; after the release it cycles -3, 0, +3 around +40.
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        hold(&mut wizard, PRO[0], 0.5);
        cycle(&mut wizard, &[[37, 0, 0, 0, 0, 0], [40, 0, 0, 0, 0, 0], [43, 0, 0, 0, 0, 0]], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
        assert_eq!(wizard.recording.settled[0].mean().map(|mean| mean[0].round()), Some(40.0));
    }

    #[test]
    fn the_four_unit_floor_alone_lets_a_two_unit_dither_settle_on_a_short_range() {
        // Flat at rest, and the biggest push only reaches 100, so the reach share is 2 and only
        // the floor (4, i.e. ±2) lets this ±2 dither count as steady.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        cycle(&mut wizard, &[[8, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [12, 0, 0, 0, 0, 0]], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
    }

    #[test]
    fn dither_the_steadiness_check_rejects_isnt_skipped_as_untouched_either() {
        // Step 0 settles on a ±2 dither around 10. Sitting there is untouched; ±3 around it is wider
        // than the steadiness check calls one position, so it is movement and starts a hold.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        cycle(&mut wizard, &[[8, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [12, 0, 0, 0, 0, 0]], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
        cycle(&mut wizard, &[[8, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [12, 0, 0, 0, 0, 0]], 0.2);
        assert_eq!(wizard.progress(), 0.0);
        cycle(&mut wizard, &[[7, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [13, 0, 0, 0, 0, 0]], 0.2);
        assert!(wizard.progress() > 0.0);
    }

    #[test]
    fn a_push_after_a_single_reading_settle_starts_a_hold() {
        // One reading back at rest, not a steady run, is all the settle stores; a full push after
        // it is still movement.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        wizard.feed([0; 6], 0.02);
        assert_eq!(wizard.stage(), Stage::Step(1));
        assert_eq!(wizard.settled.map(|settled| settled.count), Some(1));
        hold(&mut wizard, [-100, 0, 0, 0, 0, 0], 0.2);
        assert_eq!((wizard.stage(), wizard.progress() > 0.0), (Stage::Step(1), true));
    }

    #[test]
    fn a_narrow_settled_run_skips_a_whole_tolerance_either_side_and_the_band_stays_anchored() {
        // Flat at rest and a 100 push: tolerance is exactly 4. The release settles flat at 10, so a
        // reading 4 either side (6, 14) is untouched, repeated or not; one more unit is movement.
        let wizard = || {
            let mut wizard = Wizard::default();
            hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
            hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
            hold(&mut wizard, [10, 0, 0, 0, 0, 0], SETTLE_SECONDS + 0.1);
            assert_eq!(wizard.stage(), Stage::Step(1));
            assert_eq!(wizard.tolerance(0), 4.0);
            wizard
        };
        let mut edges = wizard();
        cycle(&mut edges, &[[6, 0, 0, 0, 0, 0], [14, 0, 0, 0, 0, 0]], 0.2);
        hold(&mut edges, [14, 0, 0, 0, 0, 0], 0.2);
        assert_eq!(edges.progress(), 0.0);
        // Skipped readings don't widen the band: after all those 14s, 15 is still movement.
        edges.feed([15, 0, 0, 0, 0, 0], 0.02);
        assert!(edges.progress() > 0.0);
        for x in [5, 15] {
            let mut wizard = wizard();
            wizard.feed([x, 0, 0, 0, 0, 0], 0.02);
            assert!(wizard.progress() > 0.0, "{x}");
        }
    }

    #[test]
    fn a_step_settles_on_its_own_axis_when_another_with_a_wider_range_stays_out() {
        // The step's own axis only goes to 100; another axis stays 30 out after the release,
        // more than a quarter of 100, which mustn't hold the step up.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        hold(&mut wizard, [5, 30, 0, 0, 0, 0], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
    }

    #[test]
    fn a_step_stays_held_when_the_lesser_axis_is_held_while_the_larger_one_relaxes() {
        for held in [[100, 5, 0, 0, 0, 0], [100, 60, 0, 0, 0, 0], [100, 200, 0, 0, 0, 0]] {
            let mut wizard = Wizard::default();
            hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
            hold(&mut wizard, [100, 200, 0, 0, 0, 0], 0.5);
            hold(&mut wizard, held, SETTLE_SECONDS + 0.1);
            assert_eq!(wizard.stage(), Stage::Step(0), "{held:?}");
        }
        // A plain hold of a single axis.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(0));
    }

    #[test]
    fn a_step_settles_when_cross_talk_moved_another_axis_further_than_its_own() {
        // Nula's case: the push moves raw 1 twice as far as raw 0, which may be the one asked
        // for. Raw 0 comes back to 5 % and raw 1 stays at 30 %: let go, both steady.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 200, 0, 0, 0, 0], 0.5);
        hold(&mut wizard, [5, 60, 0, 0, 0, 0], SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
        // Still holding the push, with only the lesser axis back, isn't let go.
        let mut holding = Wizard::default();
        hold(&mut holding, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut holding, [100, 200, 0, 0, 0, 0], 0.5);
        hold(&mut holding, [5, 200, 0, 0, 0, 0], 1.0);
        assert_eq!(holding.stage(), Stage::Step(0));
    }

    #[test]
    fn a_lopsided_dither_that_settled_a_step_isnt_movement_in_the_next() {
        // Mostly 10 with an occasional 14: spread 4, so steady, but the mean sits near 11 and the 14
        // is the far edge. Sitting there stays untouched; 15 is wider than the steadiness check allows.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 0, 0, 0, 0, 0], 0.5);
        let lopsided = [[10, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [10, 0, 0, 0, 0, 0], [14, 0, 0, 0, 0, 0]];
        cycle(&mut wizard, &lopsided, SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
        cycle(&mut wizard, &lopsided, 1.0);
        assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(1), 0.0));
        hold(&mut wizard, [16, 0, 0, 0, 0, 0], 0.2);
        assert!(wizard.progress() > 0.0);
    }

    #[test]
    fn cross_talk_that_leaves_the_bigger_axis_well_out_keeps_the_step_open() {
        // The documented limit (Grok's case): raw 0 moved less than half as far as raw 1, so it
        // never qualifies, and raw 1 at 90 is above a quarter of its 300 peak.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        hold(&mut wizard, [100, 300, 0, 0, 0, 0], 0.5);
        hold(&mut wizard, [5, 90, 0, 0, 0, 0], SETTLE_SECONDS + 1.0);
        assert_eq!(wizard.stage(), Stage::Step(0));
    }

    #[test]
    fn a_hand_held_still_close_to_rest_after_the_push_counts_as_let_go() {
        // The known limit: held exactly still at a fifth of the way out, on any device, ends the
        // step there, and that axis's dead zone widens to cover it.
        let low = PRO[0].map(|value| value / 5);
        let mut wizard = Wizard::default();
        rest_then(&mut wizard);
        hold(&mut wizard, PRO[0], 0.5);
        hold(&mut wizard, low, SETTLE_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(1));
        assert_eq!((wizard.recording.settled[0].min, wizard.recording.settled[0].max), (low, low));
        // Coming back to its true rest afterwards doesn't undo that: the next step is waiting.
        hold(&mut wizard, STILL[0], 0.5);
        assert_eq!((wizard.stage(), wizard.progress()), (Stage::Step(1), 0.0));
        // No frame of where it settled went into the step as held.
        assert_eq!((wizard.recording.steps[0].min[0], wizard.recording.steps[0].max[4]), (PRO[0][0], PRO[0][4]));
    }

    #[test]
    fn every_reading_of_a_release_that_was_let_go_falls_in_the_dead_zone() {
        // Nula's case: X wanders -4..+4 at rest. Half the releases dither over +8..+16, the other
        // half over -16..-8. Keeping one reading of each, padded by the wander, can miss the edges.
        let wander = [[-4, 0, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0]];
        let x = |value: i16| [value, 0, 0, 0, 0, 0];
        let (above, below) = ([x(16), x(12), x(8)], [x(-8), x(-12), x(-16)]);
        let mut wizard = Wizard::default();
        for _ in 0..105 {
            hold(&mut wizard, wander[0], 0.02);
            hold(&mut wizard, wander[1], 0.02);
        }
        for (index, reading) in PRO.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            hold(&mut wizard, reading, 0.5);
            cycle(&mut wizard, if index % 2 == 0 { &above } else { &below }, SETTLE_SECONDS + 0.1);
        }
        let calibration = wizard.result().unwrap().unwrap();
        for still in above.iter().chain(&below).chain(&wander) {
            assert_eq!(calibration.apply(*still), [0; 6], "{still:?} {calibration:?}");
        }
    }

    #[test]
    fn two_steps_in_a_row_that_stop_at_the_same_place_both_finish() {
        let mut wizard = Wizard::default();
        hold(&mut wizard, STUCK[0], REST_SECONDS + TOUCH_SECONDS + 0.1);
        for reading in &PRO[..2] {
            hold(&mut wizard, *reading, 0.5);
            hold(&mut wizard, STUCK[1], SETTLE_SECONDS + 0.1);
        }
        assert_eq!(wizard.stage(), Stage::Step(2));
    }

    #[test]
    fn recentring_on_where_the_cap_settled_keeps_the_whole_rest_wander_inside_the_dead_zone() {
        // X wanders -4..+4 at rest, and every release settles at +12.
        let wander = [[-4, 0, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0]];
        let mut recording = record(PRO, wander);
        for index in 0..12 {
            recording.settled(index, [12, 0, 0, 0, 0, 0]);
        }
        let calibration = recording.solve().unwrap();
        for still in [wander[0], wander[1], [12, 0, 0, 0, 0, 0]] {
            assert_eq!(calibration.apply(still), [0; 6], "{still:?} {calibration:?}");
        }
    }

    #[test]
    fn the_dead_zone_shares_are_where_apply_starts_moving() {
        // A lopsided, reversed axis with the user's dead zone on top.
        let mut calibration = walk(STILL, PRO).result().unwrap().unwrap();
        calibration.axes[0] = AxisCalibration { source: 0, reversed: true, rest: 10, dead_zone: 20, full: [100, 400] };
        calibration.dead_zone_share = 0.1;
        let [negative, positive] = calibration.dead_zone_shares()[0];
        // Canonical negative is raw above rest here: 400 of travel, 20 measured.
        assert!((negative - (0.05 + 0.95 * 0.1)).abs() < 1e-6 && (positive - (0.2 + 0.8 * 0.1)).abs() < 1e-6);
        let at = |raw: i16| [raw, 0, 0, 0, 0, 0];
        let inside = |share: f32, full: f32| (share * full).floor() as i16 - 1;
        assert_eq!(calibration.apply(at(10 + inside(negative, 400.0)))[0], 0);
        assert!(calibration.apply(at(10 + inside(negative, 400.0) + 3))[0] < 0);
        assert_eq!(calibration.apply(at(10 - inside(positive, 100.0)))[0], 0);
        assert!(calibration.apply(at(10 - inside(positive, 100.0) - 3))[0] > 0);
        // Full deflection is ±1 of travel, the reversed way round.
        assert_eq!(calibration.travel(at(410))[0], -1.0);
        assert_eq!(calibration.travel(at(-90))[0], 1.0);
    }

    #[test]
    fn a_device_that_comes_back_to_rest_keeps_its_measured_rest_and_dead_zone() {
        let mut settled = record(PRO, STILL);
        let plain = settled.solve().unwrap();
        for index in 0..12 {
            settled.settled(index, STILL[index % 2]);
        }
        assert_eq!(settled.solve().unwrap(), plain);
    }

    #[test]
    fn a_noisy_axis_at_rest_holds_up_neither_itself_nor_a_quiet_one() {
        // X wanders ±4 at rest, and goes +500 one way but only -50 the other: the solver accepts
        // that, so the wizard has to be able to record it.
        let mut steps = PRO;
        steps[0][0] = 500;
        steps[1][0] = -50;
        let noisy_x = [[-4, 0, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0]];
        let calibration = walk(noisy_x, steps).result().unwrap().unwrap();
        assert_eq!((calibration.axes[0].source, calibration.axes[0].dead_zone, calibration.axes[0].full), (0, 4, [50, 500]));
        // Rz wanders ±40 at rest: every other axis still records against its own quiet rest.
        let noisy_rz = [[0, 0, 0, 0, 0, -40], [0, 0, 0, 0, 0, 40]];
        let calibration = walk(noisy_rz, PRO).result().unwrap().unwrap();
        assert!(calibration.axes.iter().enumerate().all(|(c, axis)| axis.source == c && !axis.reversed), "{calibration:?}");
    }

    #[test]
    fn a_step_that_caught_a_resting_hand_instead_of_a_push_is_named() {
        // A quiet device (nothing at rest), and slide right recorded only a hand resting on the
        // cap: scaled up, a few units would be full speed.
        let mut steps = PRO;
        steps[0] = [6, 4, 0, 0, 3, 0];
        assert_eq!(record(steps, [[0; 6]; 2]).solve(), Err(Problem::NoResponse(0)));
    }

    #[test]
    fn a_hand_edited_calibration_that_could_spin_the_view_isnt_usable() {
        let solved = record(PRO, STILL).solve().unwrap();
        assert!(solved.is_usable());
        let mut bad = solved;
        bad.axes[2].source = 6;
        assert!(!bad.is_usable(), "a source the device doesn't have");
        let mut bad = solved;
        bad.axes[2].full[0] = bad.axes[2].dead_zone;
        assert!(!bad.is_usable(), "any touch past the dead zone would be full speed");
        let mut bad = solved;
        bad.axes[2].source = bad.axes[3].source;
        assert!(!bad.is_usable(), "two canonical axes from one raw axis");
    }

    #[test]
    fn readings_for_a_step_that_doesnt_exist_are_ignored() {
        let mut recording = record(PRO, STILL);
        recording.step(12, [i16::MAX; 6]);
        assert_eq!(recording.solve(), record(PRO, STILL).solve());
    }

    /// A trackball as raw axes: rolled right on raw 0, rolled toward you on raw 1, its scroll ring
    /// on raw 2 in single clicks, spun on raw 5; raw 3 and 4 never move.
    const BALL: [(usize, [i16; 6]); 8] = [
        (0, [40, 3, 0, 0, 0, 0]),
        (1, [-40, -2, 0, 0, 0, 0]),
        (2, [2, 40, 0, 0, 0, 0]),
        (3, [-3, -40, 0, 0, 0, 0]),
        (4, [0, 0, 1, 0, 0, 0]),
        (5, [0, 0, -1, 0, 0, 0]),
        (10, [0, 0, 0, 0, 0, 30]),
        (11, [0, 0, 0, 0, 0, -30]),
    ];

    /// Strokes of `reading` with pauses between, as a hand rolls a ball or clicks a scroll ring,
    /// until the step moves on.
    fn strokes(wizard: &mut Wizard, reading: [i16; 6], index: usize) {
        for _ in 0..20 {
            if wizard.stage() != Stage::Step(index) {
                return;
            }
            hold(wizard, reading, 0.14);
            hold(wizard, [0; 6], 0.1);
        }
    }

    #[test]
    fn a_trackball_calibrates_from_strokes_with_pauses_and_has_no_tilt() {
        let mut wizard = Wizard::choosing();
        hold(&mut wizard, [5; 6], 1.0);
        assert_eq!(wizard.stage(), Stage::Choose, "nothing is recorded before the kind is picked");
        wizard.choose(Kind::Trackball);
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        assert_eq!(wizard.counter(), (1, 8));
        for (n, (index, reading)) in BALL.into_iter().enumerate() {
            assert_eq!(wizard.stage(), Stage::Step(index));
            assert_eq!(wizard.counter(), (n + 1, 8));
            strokes(&mut wizard, reading, index);
        }
        assert_eq!(wizard.stage(), Stage::Done);
        assert_eq!(wizard.mismatch(), None);
        let calibration = wizard.result().unwrap().unwrap();
        assert_eq!(calibration.kind, Kind::Trackball);
        for (axis, source) in [(0, 0), (1, 1), (2, 2), (5, 5)] {
            assert_eq!((calibration.axes[axis].source, calibration.axes[axis].reversed), (source, false), "axis {axis}");
        }
        assert_eq!((calibration.axes[3].source, calibration.axes[4].source), (NO_SOURCE, NO_SOURCE));
        assert!(calibration.is_usable());
        // The raw axes it has no use for read nothing; the ones it has read at full scale.
        assert_eq!(calibration.apply([0, 0, 0, 99, 99, 0]), [0; 6]);
        assert_eq!(calibration.apply([40, 0, 0, 0, 0, 0])[0], FULL_SCALE);
    }

    /// A ball that slows to a lower speed and keeps sending it is still rolling: its fresh reports are not
    /// where it came to rest. (Without freshness, a daemon's kept value, the same plateau does end the step.)
    #[test]
    fn a_decelerating_trackball_that_keeps_sending_is_not_taken_for_rest() {
        let run = |fresh: bool| {
            let mut wizard = Wizard::new(Kind::Trackball);
            hold_fresh(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1, fresh);
            assert_eq!(wizard.stage(), Stage::Step(0));
            hold_fresh(&mut wizard, [40, 3, 0, 0, 0, 0], 0.6, fresh);
            hold_fresh(&mut wizard, [10, 0, 0, 0, 0, 0], 0.7, fresh);
            wizard
        };
        // Reports still arriving at that speed: the ball is rolling, the step is not done.
        let mut wizard = run(true);
        assert_eq!(wizard.stage(), Stage::Step(0), "a fresh plateau is motion");
        // It then stops (a zero report) and the strokes go on: the first axis is not recorded as 10 at rest.
        hold_fresh(&mut wizard, [0; 6], 0.2, true);
        assert_eq!(wizard.stage(), Stage::Step(1));
        for (index, reading) in BALL.into_iter().skip(1) {
            strokes(&mut wizard, reading, index);
        }
        let calibration = wizard.result().unwrap().expect("no NoResponse for the first axis");
        assert!(calibration.axes[0].rest == 0 && calibration.axes[0].dead_zone < 5, "{:?}", calibration.axes[0]);
        // The same readings as a kept value (no freshness) keep the old reading of a plateau.
        assert_eq!(run(false).stage(), Stage::Step(1), "a kept value that stops changing is where it stopped");
    }

    /// Reports that arrive only on some frames (a device slower than the display) still count as arriving,
    /// and silence for longer than the gap does not.
    #[test]
    fn reports_on_some_frames_keep_a_plateau_rolling_and_silence_ends_it() {
        let mut wizard = Wizard::new(Kind::Trackball);
        hold_fresh(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1, true);
        hold_fresh(&mut wizard, [40, 3, 0, 0, 0, 0], 0.6, true);
        // A report every third frame (60 ms), the same speed.
        for frame in 0..40 {
            wizard.feed_reading([10, 0, 0, 0, 0, 0], 0.02, frame % 3 == 0);
        }
        assert_eq!(wizard.stage(), Stage::Step(0), "reports still arriving");
        // Nothing arrives any more: the last value is kept, and the plateau is where it stopped.
        hold_fresh(&mut wizard, [10, 0, 0, 0, 0, 0], 0.8, false);
        assert_eq!(wizard.stage(), Stage::Step(1));
    }

    #[test]
    fn a_steady_trackball_speed_is_not_taken_for_rest() {
        let mut wizard = Wizard::new(Kind::Trackball);
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(0));
        // A quantized ball rolling at one speed for longer than a cap is ever held.
        hold(&mut wizard, [40, 3, 0, 0, 0, 0], HELD_SECONDS + 0.1);
        assert_eq!(wizard.stage(), Stage::Step(0), "the step is not done by a reading that stays high");
        // Letting it stop finishes the step, and the rest of the strokes go as usual.
        hold(&mut wizard, [0; 6], 0.2);
        assert_eq!(wizard.stage(), Stage::Step(1));
        for (index, reading) in BALL.into_iter().skip(1) {
            strokes(&mut wizard, reading, index);
        }
        let calibration = wizard.result().unwrap().expect("no NoResponse for the first axis");
        assert!(calibration.axes[0].dead_zone < 20 && calibration.axes[0].rest == 0, "{:?}", calibration.axes[0]);
        assert!(calibration.is_usable());
    }

    #[test]
    fn a_motion_the_device_lacks_can_be_skipped_but_not_the_last_one() {
        let mut wizard = Wizard::new(Kind::Trackball);
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        for (index, reading) in BALL.into_iter().take(4) {
            strokes(&mut wizard, reading, index);
        }
        // No scroll ring: skipping the press step skips the lift step too.
        assert_eq!(wizard.stage(), Stage::Step(4));
        wizard.skip();
        assert_eq!((wizard.stage(), wizard.counter()), (Stage::Step(10), (5, 6)));
        for (index, reading) in BALL.into_iter().skip(6) {
            strokes(&mut wizard, reading, index);
        }
        let calibration = wizard.result().unwrap().unwrap();
        assert_eq!(calibration.axes[2].source, NO_SOURCE);
        assert!(calibration.is_usable());
        // Going back over a skipped motion doesn't ask for it again.
        wizard.back();
        assert_eq!(wizard.stage(), Stage::Step(11));
        wizard.back();
        wizard.back();
        assert_eq!(wizard.stage(), Stage::Step(3));
        // A device has to have something.
        let mut wizard = Wizard::new(Kind::Trackball);
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        for _ in 0..3 {
            assert!(wizard.can_skip());
            wizard.skip();
        }
        assert_eq!(wizard.stage(), Stage::Step(10));
        assert!(!wizard.can_skip());
        wizard.skip();
        assert_eq!(wizard.stage(), Stage::Step(10));
        let none = Calibration { dead_zone_share: 0.0, kind: Kind::Trackball, axes: [calibration.axes[3]; 6] };
        assert!(!none.is_usable());
    }

    #[test]
    fn a_device_that_doesnt_behave_like_the_kind_picked_is_named() {
        // A trackball calibrated as a cap: every stroke ends too soon.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + TOUCH_SECONDS + 0.1);
        for _ in 0..RESTARTS_FOR_TRACKBALL {
            assert_eq!(wizard.mismatch(), None);
            hold(&mut wizard, [40, 0, 0, 0, 0, 0], 0.14);
            hold(&mut wizard, [0; 6], 0.1);
        }
        assert_eq!(wizard.mismatch(), Some(Kind::Trackball));
        // A cap calibrated as a trackball: held out, steady.
        let mut wizard = Wizard::new(Kind::Trackball);
        rest_then(&mut wizard);
        hold(&mut wizard, PRO[0], HELD_SECONDS + 0.2);
        assert_eq!(wizard.mismatch(), Some(Kind::ReturnToCenter));
        // Switching starts over as the other kind, keeping the dead zone.
        wizard.set_dead_zone_share(0.1);
        wizard.choose(Kind::ReturnToCenter);
        assert_eq!((wizard.stage(), wizard.kind(), wizard.dead_zone_share(), wizard.mismatch()), (Stage::Rest, Kind::ReturnToCenter, 0.1, None));
        wizard.back();
        assert_eq!(wizard.stage(), Stage::Choose);
    }

    #[test]
    fn a_resting_hand_sets_the_dead_zone_but_never_hides_the_axis() {
        // A cap that reads nothing untouched, as a SpaceMouse Pro does once its reports stop.
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + 0.02);
        assert_eq!(wizard.stage(), Stage::Touch);
        // The hand on the cap leans a little: 12 on X, 30 on Rz.
        while wizard.stage() == Stage::Touch {
            hold(&mut wizard, [12, -5, 3, 0, 4, -30], 0.02);
            hold(&mut wizard, [-8, 5, 0, 2, -4, 20], 0.02);
        }
        assert_eq!(wizard.stage(), Stage::Step(0));
        hold(&mut wizard, [0; 6], 0.1);
        for reading in PRO {
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, [0; 6], 0.1);
        }
        let calibration = wizard.result().unwrap().unwrap();
        assert_eq!(calibration.axes[0].dead_zone, 12);
        assert_eq!(calibration.axes[5].dead_zone, 30);
        // What the hand did reads as nothing; past it, the cap moves.
        assert_eq!(calibration.apply([12, -5, 3, 0, 4, -30]), [0; 6]);
        assert!(calibration.apply([40, 0, 0, 0, 0, 0])[0] > 0);
        // A hand that pushed instead takes at most a quarter of the axis.
        let mut recording = record(PRO, STILL);
        recording.touch([300, 0, 0, 0, 0, 0]);
        let pushed = recording.solve().unwrap();
        assert!(f64::from(pushed.axes[0].dead_zone) <= MAX_TOUCH_SHARE * f64::from(pushed.axes[0].full[0].min(pushed.axes[0].full[1])) + 1.0);
        assert!(pushed.is_usable());
    }

    #[test]
    fn a_hand_landing_on_the_cap_does_not_set_the_dead_zone() {
        let mut wizard = Wizard::default();
        hold(&mut wizard, [0; 6], REST_SECONDS + 0.02);
        // The landing jolts X to 90 for a quarter second, then the hand leans a little: 12.
        hold(&mut wizard, [90, 0, 0, 0, 0, 0], 0.3);
        while wizard.stage() == Stage::Touch {
            hold(&mut wizard, [12, 0, 0, 0, 0, 0], 0.02);
            hold(&mut wizard, [-8, 0, 0, 0, 0, 0], 0.02);
        }
        hold(&mut wizard, [0; 6], 0.1);
        for reading in PRO {
            hold(&mut wizard, reading, 0.5);
            hold(&mut wizard, [0; 6], 0.1);
        }
        let calibration = wizard.result().unwrap().unwrap();
        assert_eq!(calibration.axes[0].dead_zone, 12);
        // Even landing late, a few frames past the first half second, one jolt isn't the dead zone.
        let mut recording = record(PRO, STILL);
        for _ in 0..3 {
            recording.touch([90, 0, 0, 0, 0, 0]);
        }
        for _ in 0..97 {
            recording.touch([10, 0, 0, 0, 0, 0]);
        }
        assert_eq!(recording.solve().unwrap().axes[0].dead_zone, 10);
    }

    #[test]
    fn a_slow_trackball_roll_maps_straight_while_a_cap_feathers_in() {
        let axis = AxisCalibration { source: 0, reversed: false, rest: 0, dead_zone: 0, full: [350; 2] };
        let ball = Calibration { dead_zone_share: 0.0, kind: Kind::Trackball, axes: [axis; 6] };
        assert_eq!(ball.apply([9, 0, 0, 0, 0, 0])[0], 9);
        assert_eq!(ball.apply([350, 0, 0, 0, 0, 0])[0], FULL_SCALE);
        let cap = Calibration { kind: Kind::ReturnToCenter, ..ball };
        assert!(cap.apply([9, 0, 0, 0, 0, 0])[0] < 9);
    }

    #[test]
    fn the_response_feathers_in_past_the_dead_zone() {
        let axis = AxisCalibration { source: 0, reversed: false, rest: 0, dead_zone: 20, full: [370; 2] };
        let calibration = Calibration { dead_zone_share: 0.0, kind: Kind::default(), axes: [axis; 6] };
        let read = |value: i16| calibration.apply([value, 0, 0, 0, 0, 0])[0];
        // The dead zone's edge is still where motion starts, and it starts small.
        assert_eq!((read(20), read(21)), (0, 1));
        // Easing in: under the straight line it used to follow, which had 30 past the edge at 30.
        assert!(read(50) < 30 && read(50) > 0);
        // Never steps back or jumps, and still reaches full scale.
        let response: Vec<i16> = (0..=370).map(read).collect();
        assert!(response.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] <= 2), "{response:?}");
        assert_eq!(read(370), FULL_SCALE);
        assert_eq!(read(-370), -FULL_SCALE);
    }

    #[test]
    fn a_steady_lean_near_the_dead_zone_drops_but_a_held_push_stays() {
        let mut settle = Settle::default();
        let frames = |settle: &mut Settle, reading: [i16; 6], seconds: f64| {
            let mut out = [0; 6];
            for _ in 0..(seconds / 0.02).round() as usize {
                out = settle.filter(reading, 0.02);
            }
            out
        };
        // A hand leaning: 30 on X, jittering by 2, then held there.
        assert_eq!(frames(&mut settle, [30, 0, 0, 0, 0, 0], 0.1)[0], 30, "fresh, it moves");
        frames(&mut settle, [32, 0, 0, 0, 0, 0], 0.1);
        assert_eq!(frames(&mut settle, [31, 0, 0, 0, 0, 0], SETTLE_AFTER_SECONDS)[0], 0, "steady: dropped");
        // Moving again moves at once.
        assert_eq!(settle.filter([45, 0, 0, 0, 0, 0], 0.02)[0], 45);
        // A held push past the band stays, however steady.
        assert_eq!(frames(&mut settle, [0, 0, 0, 0, 0, 200], 2.0)[5], 200);
        // Each axis on its own, and nothing odd for odd times.
        assert!(settle.filter([10, -10, 0, 0, 0, 0], f64::NAN).iter().all(|v| v.abs() <= 10));
    }
}
