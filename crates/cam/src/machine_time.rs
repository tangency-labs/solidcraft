//! A written program timed as the machine runs it, and the controller settings a post must
//! agree with to be read right.
//!
//! A controller moves no axis faster than its top rate: a rapid takes as long as its slowest
//! axis needs, every axis at its own rapid rate, and a cut written faster than an axis can go
//! (a feed above the machine's top feed, or a cut that turns a rotary faster than it can turn,
//! in inverse time, `G93`, or not) is slowed until every axis keeps within its rate, so the cut
//! takes longer than its `F` says. [`machine_time`] reads a program with [`parse5`] and times every
//! block that way against a machine's [`MachineRates`]: the cut time as written and as run, the
//! rapid time, the spindle's start-up waits, and the blocks the controller would slow, with a
//! warning for each kind. Acceleration is left out (a controller blending moves at constant
//! velocity rarely stops between them), so the times are a lower bound for short blocks. A
//! return of Z to the machine's home (`G28 … Z`, before a tool change or in a rotary's rewind)
//! is timed up to [`MachineRates::home_z_mm`] and back down from there.
//!
//! [`block_times`] times every block on its own for a view that plays the program in time,
//! and, where the rates give the axes' accelerations, with them: each block speeds up and slows
//! down as a trapezoid (or a triangle when it is too short to reach its top speed), slowing at
//! a corner to the speed both blocks share times the cosine of the turn (to a stop at a right
//! angle or more, and between a rapid and a cut), round an arc to `√(a·r)`; spindle starts and
//! tool changes wait their time. Ported from the author's own MIT-licensed G-code simulator.
//!
//! A dwell's `P` is read in the unit the post writes it in ([`DwellUnit`]), which must be the
//! unit the controller is set to read (some read `G4 P` in seconds, others in milliseconds).

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::gcode::{number, words};
use crate::gcode5::{JointWords, parse5};
use crate::kinematics::AxisLetter;
use crate::path::{ArcPlane, Move};
use crate::post::Post;
use crate::{Result, invalid};

/// Largest rate accepted (mm/min or degrees a minute).
const MAX_RATE: f64 = 1.0e6;
/// Largest travel accepted (mm).
const MAX_TRAVEL: f64 = 1.0e5;
/// Longest spindle start accepted (seconds).
const MAX_SPINDLE_START: f64 = 3600.0;
/// Most spindle speeds out of range listed in a report.
const MAX_LISTED: usize = 20;
/// Share by which a block may run longer than written before it counts as slowed.
const SLOWED: f64 = 1e-6;

/// How a post's dwell template states a dwell's length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DwellUnit {
    Seconds,
    Milliseconds,
}

impl DwellUnit {
    /// The unit `post`'s dwell template writes: `{milliseconds}` or `{seconds}`, never both, and
    /// never neither (a dwell would then be written at a fixed length, whatever was planned).
    pub fn of(post: &Post) -> Result<DwellUnit> {
        match (post.dwell.contains("{seconds}"), post.dwell.contains("{milliseconds}")) {
            (true, false) => Ok(DwellUnit::Seconds),
            (false, true) => Ok(DwellUnit::Milliseconds),
            (true, true) => invalid("the post's dwell template must give the length once: `{seconds}` or `{milliseconds}`, not both"),
            (false, false) => invalid("the post's dwell template must give the length: `{seconds}` or `{milliseconds}`"),
        }
    }

    /// A dwell's `P` (or `X`) as written, in seconds.
    pub fn seconds(self, written: f64) -> f64 {
        match self {
            DwellUnit::Seconds => written,
            DwellUnit::Milliseconds => written / 1000.0,
        }
    }
}

/// The dwells of read motion, as [`crate::parse`] and [`parse5`] read them (`P` in seconds), in
/// seconds for a post that writes them in `unit`.
pub fn dwells_in(unit: DwellUnit, moves: Vec<Move>) -> Vec<Move> {
    moves
        .into_iter()
        .map(|m| match m {
            Move::Dwell { seconds } => Move::Dwell { seconds: unit.seconds(seconds) },
            other => other,
        })
        .collect()
}

/// [`dwells_in`] for multi-axis motion.
pub fn joint_dwells_in(unit: DwellUnit, words: Vec<JointWords>) -> Vec<JointWords> {
    words
        .into_iter()
        .map(|w| match w {
            JointWords::Dwell { line, seconds } => JointWords::Dwell { line, seconds: unit.seconds(seconds) },
            other => other,
        })
        .collect()
}

/// A machine's top rates, from its controller's settings. Every linear rate is mm/min, every
/// rotary rate degrees a minute; a rate left out is not limited (and a rapid of that axis is
/// not timed).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRates {
    /// Rapid rate of X and Y (and of Z, unless `rapid_z_mm_min` is given).
    pub rapid_xyz_mm_min: f64,
    /// Rapid rate of Z, where it differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rapid_z_mm_min: Option<f64>,
    /// Rapid rate of every rotary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rapid_a_deg_min: Option<f64>,
    /// Top feed of X and Y (and of Z, unless `max_feed_z_mm_min` is given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_feed_mm_min: Option<f64>,
    /// Top feed of Z, where it differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_feed_z_mm_min: Option<f64>,
    /// Top rate of every rotary while cutting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_a_deg_min: Option<f64>,
    /// The spindle's lowest and highest speed (rpm): the controller runs any other `S` at the
    /// nearest end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_rpm: Option<[f64; 2]>,
    /// How long the controller waits for the spindle to come up to speed at each start (`M3`,
    /// `M4`), seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_start_seconds: Option<f64>,
    /// The travel of X, Y and Z (mm): no program can span more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub travel_mm: Option<[f64; 3]>,
    /// Acceleration of X and Y (and of Z, unless `accel_z_mm_s2` is given), mm/s²: given, the
    /// blocks are timed with it ([`block_times`]); left out, at their top rates throughout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel_mm_s2: Option<f64>,
    /// Acceleration of Z, where it differs, mm/s².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel_z_mm_s2: Option<f64>,
    /// Acceleration of every rotary, degrees/s².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel_a_deg_s2: Option<f64>,
    /// How long a tool change (`M6`) takes, seconds ([`block_times`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_change_seconds: Option<f64>,
    /// Where Z's home is, in work coordinates (mm): a return home (`G28` with a `Z` word, as a
    /// post writes before a tool change or a rewind) takes the tool up there and the next move
    /// brings it back down, both timed at rapid. Left out, the highest Z the program reaches,
    /// so the trip is timed at least that far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_z_mm: Option<f64>,
}

fn rate_ok(v: f64) -> bool {
    v > 0.0 && v <= MAX_RATE
}

impl MachineRates {
    pub fn validate(&self) -> Result<()> {
        let rates = [self.rapid_z_mm_min, self.rapid_a_deg_min, self.max_feed_mm_min, self.max_feed_z_mm_min, self.max_a_deg_min];
        if !rate_ok(self.rapid_xyz_mm_min) || rates.iter().flatten().any(|&r| !rate_ok(r)) {
            return invalid(format!("every machine rate must be above 0 and at most {} a minute", number(MAX_RATE, 0)));
        }
        if let Some([lo, hi]) = self.spindle_rpm
            && !(lo >= 0.0 && lo <= hi && hi <= MAX_RATE)
        {
            return invalid("the spindle range must be [lowest, highest] rpm, the lowest first, at most 1000000");
        }
        if self.spindle_start_seconds.is_some_and(|s| !(0.0..=MAX_SPINDLE_START).contains(&s)) {
            return invalid("the spindle's start must be 0 to 3600 seconds");
        }
        if self.travel_mm.is_some_and(|t| t.iter().any(|&v| !(v > 0.0 && v <= MAX_TRAVEL))) {
            return invalid("the travel of X, Y and Z must be above 0 and at most 100 m");
        }
        if [self.accel_mm_s2, self.accel_z_mm_s2, self.accel_a_deg_s2].iter().flatten().any(|&a| !rate_ok(a)) {
            return invalid(format!("every acceleration must be above 0 and at most {} a second squared", number(MAX_RATE, 0)));
        }
        if self.accel_z_mm_s2.is_some() && self.accel_mm_s2.is_none() {
            return invalid("`accel_z_mm_s2` goes with `accel_mm_s2` (the acceleration of X and Y)");
        }
        if self.tool_change_seconds.is_some_and(|s| !(0.0..=MAX_SPINDLE_START).contains(&s)) {
            return invalid("a tool change must take 0 to 3600 seconds");
        }
        if self.home_z_mm.is_some_and(|z| !(z.abs() <= MAX_TRAVEL)) {
            return invalid("Z's home must be a finite height within 100 m of work zero");
        }
        Ok(())
    }

    fn rapid_z(&self) -> f64 {
        self.rapid_z_mm_min.unwrap_or(self.rapid_xyz_mm_min)
    }

    fn feed_z(&self) -> Option<f64> {
        self.max_feed_z_mm_min.or(self.max_feed_mm_min)
    }
}

/// A program timed against a machine's rates ([`machine_time`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineTime {
    /// Time at feed as the machine runs it, every cut held to the axes' top rates, dwells
    /// included (minutes).
    pub cut_minutes: f64,
    /// Time at feed as written (the `F` words alone), dwells included (minutes).
    pub written_cut_minutes: f64,
    /// Time at rapid, each move as long as its slowest axis takes (minutes).
    pub rapid_minutes: f64,
    /// Time waiting for the spindle to start (minutes).
    pub spindle_minutes: f64,
    /// Returns of Z to its home (`G28 … Z`) timed among the rapids: tool changes and rewinds.
    #[serde(default)]
    pub home_returns: usize,
    /// Tool changes: an `M6`, or a programmed stop (`M0`) for a controller that ignores
    /// it; a change written as both counts once ([`tool_change_lines`]).
    #[serde(default)]
    pub tool_changes: usize,
    /// Time the tool changes take, `tool_change_seconds` each (minutes).
    #[serde(default)]
    pub tool_change_minutes: f64,
    /// Tagged stops for the operator (probing, the zero check, a tool check: [`stop_lines`]),
    /// not tool changes.
    #[serde(default)]
    pub stops: usize,
    /// The operator's time at them, [`STOP_MINUTES`] each (minutes).
    #[serde(default)]
    pub stop_minutes: f64,
    /// All of it (minutes).
    pub total_minutes: f64,
    /// Cutting blocks the controller runs slower than written.
    pub slowed_blocks: usize,
    /// Of those, blocks that ask a rotary for more than its top rate.
    pub rotary_over: usize,
    /// Of those, blocks that ask a linear axis for more than its top feed.
    pub feed_over: usize,
    /// The highest rotary rate a cut asks for (degrees a minute) and its line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rotary_deg_min: Option<(usize, f64)>,
    /// The highest feed of the linear axes a cut asks for (mm/min) and its line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_feed_mm_min: Option<(usize, f64)>,
    /// Spindle speeds outside the machine's range: `(line, rpm)`, the first few.
    pub spindle_out_of_range: Vec<(usize, f64)>,
    /// How far the program's positions reach along X, Y and Z (mm).
    pub span_mm: [f64; 3],
    /// What the controller will do differently from what is written.
    pub warnings: Vec<String>,
    /// Of those, where the program asks for more than the machine's spindle range or top feeds
    /// allow (a setup sheet states these first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<String>,
}

/// Where the axes are after a block (mm, radians).
struct At {
    linear: Vec3,
    rotary: Vec<f64>,
    line: usize,
}

/// The lines of `text` that return Z to its home (`G28` or `G30` with a `Z` word).
fn home_lines(text: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let ws = words(l, i + 1)?;
        let g = |v: i64| ws.iter().any(|&(c, x)| c == 'G' && (x * 10.0).round() as i64 == v);
        if (g(280) || g(300)) && ws.iter().any(|(c, _)| *c == 'Z') {
            out.push(i + 1);
        }
    }
    Ok(out)
}

/// The largest turn of any rotary between two positions, degrees.
fn turn_deg(from: &[f64], to: &[f64]) -> f64 {
    from.iter().zip(to).map(|(a, b)| (b - a).abs().to_degrees()).fold(0.0, f64::max)
}

/// Minutes the slowest axis needs to move `d` (mm) and turn `turn` (degrees) at `xy`, `z` and
/// `a` (a rate per minute; `None`: unlimited).
fn axis_minutes(d: Vec3, turn: f64, xy: Option<f64>, z: Option<f64>, a: Option<f64>) -> f64 {
    let per = |dist: f64, rate: Option<f64>| rate.map_or(0.0, |r| dist / r);
    per(d.x.abs(), xy).max(per(d.y.abs(), xy)).max(per(d.z.abs(), z)).max(per(turn, a))
}

/// For an arc in `plane` that moves `d` end to end: the rate of the axes it turns round with
/// (each in turn moves as fast as the tool does: the slower of the plane's two), how far it
/// goes along the plane's normal, and that axis's rate.
fn arc_rates(plane: ArcPlane, d: Vec3, xy: Option<f64>, z: Option<f64>) -> (Option<f64>, f64, Option<f64>) {
    let slower = match (xy, z) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    match plane {
        ArcPlane::Xy => (xy, d.z.abs(), z),
        ArcPlane::Zx => (slower, d.y.abs(), xy),
        ArcPlane::Yz => (slower, d.x.abs(), xy),
    }
}

fn peak(slot: &mut Option<(usize, f64)>, line: usize, v: f64) {
    if slot.is_none_or(|(_, p)| v > p) {
        *slot = Some((line, v));
    }
}

/// Times `text` (G-code in absolute coordinates, rotaries `letters`, dwells written in `dwell`)
/// as a machine with `rates` runs it.
pub fn machine_time(text: &str, letters: &[AxisLetter], rates: &MachineRates, dwell: DwellUnit) -> Result<MachineTime> {
    rates.validate()?;
    let written = parse5(text, letters)?;
    let homes = home_lines(text)?;
    // Where a return home takes Z: the rates', else at least the highest Z the program reaches.
    let highest = written
        .iter()
        .filter_map(|w| match w {
            JointWords::Rapid { linear, .. }
            | JointWords::Feed { linear, .. }
            | JointWords::Arc { linear, .. }
            | JointWords::PlaneArc { linear, .. } => Some(linear.z),
            JointWords::Dwell { .. } => None,
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let home_z = rates.home_z_mm.unwrap_or(highest);
    if !homes.is_empty() && highest.is_finite() && home_z < highest - 1e-9 {
        return invalid(format!("Z's home ({home_z} mm) is below the highest Z the program reaches ({highest} mm)"));
    }
    let mut t = MachineTime::default();
    let (xy, z, a) = (rates.max_feed_mm_min, rates.feed_z(), rates.max_a_deg_min);
    let mut at: Option<At> = None;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut untimed_turns = 0usize;
    let (mut first_rotary_over, mut first_feed_over) = (None, None);
    // The fastest feed among the blocks slowed for a linear axis: the feed the warning names
    // (the fastest of all may be a diagonal no single axis is too slow for).
    let mut worst_feed_over: Option<(usize, f64)> = None;
    for w in &written {
        let (line, linear, rotary) = match w {
            JointWords::Dwell { seconds, .. } => {
                let minutes = dwell.seconds(*seconds) / 60.0;
                t.cut_minutes += minutes;
                t.written_cut_minutes += minutes;
                continue;
            }
            JointWords::Rapid { line, linear, rotary, .. }
            | JointWords::Feed { line, linear, rotary, .. }
            | JointWords::Arc { line, linear, rotary, .. }
            | JointWords::PlaneArc { line, linear, rotary, .. } => (*line, *linear, rotary),
        };
        for (k, v) in [linear.x, linear.y, linear.z].into_iter().enumerate() {
            if let (Some(lo), Some(hi)) = (low.get_mut(k), high.get_mut(k)) {
                *lo = lo.min(v);
                *hi = hi.max(v);
            }
        }
        // The first block whose every axis is known only says where the axes are.
        let Some(mut from) = at.replace(At { linear, rotary: rotary.clone(), line }) else { continue };
        // A return home since the last block: up to Z's home at rapid, and on from there.
        if homes.iter().any(|&h| h > from.line && h < line) && home_z.is_finite() {
            t.home_returns += 1;
            t.rapid_minutes += (home_z - from.linear.z).max(0.0) / rates.rapid_z();
            from.linear.z = home_z;
        }
        let d = linear - from.linear;
        let turn = turn_deg(&from.rotary, rotary);
        match w {
            JointWords::Rapid { .. } => {
                if turn > 0.0 && rates.rapid_a_deg_min.is_none() {
                    untimed_turns += 1;
                }
                t.rapid_minutes += axis_minutes(d, turn, Some(rates.rapid_xyz_mm_min), Some(rates.rapid_z()), rates.rapid_a_deg_min);
            }
            JointWords::Feed { f, inverse_time, mm_per_unit, .. } => {
                let len = d.len();
                let nominal = if *inverse_time {
                    1.0 / f
                } else if len > 1e-12 {
                    len / (f * mm_per_unit)
                } else {
                    turn / f
                };
                if !(nominal.is_finite() && nominal > 0.0) {
                    continue;
                }
                // Every cut that turns a rotary asks it for a rate, whether its `F` is inverse
                // time or along the linear axes (or, for a turn alone, in degrees a minute).
                if turn > 0.0 {
                    peak(&mut t.peak_rotary_deg_min, line, turn / nominal);
                }
                if len > 1e-12 {
                    peak(&mut t.peak_feed_mm_min, line, len / nominal);
                }
                let run = nominal.max(axis_minutes(d, turn, xy, z, a));
                t.written_cut_minutes += nominal;
                t.cut_minutes += run;
                if run > nominal * (1.0 + SLOWED) {
                    t.slowed_blocks += 1;
                    if a.is_some_and(|a| turn / nominal > a * (1.0 + SLOWED)) {
                        t.rotary_over += 1;
                        first_rotary_over.get_or_insert(line);
                    }
                    if axis_minutes(d, 0.0, xy, z, None) > nominal * (1.0 + SLOWED) {
                        t.feed_over += 1;
                        first_feed_over.get_or_insert(line);
                        peak(&mut worst_feed_over, line, len / nominal);
                    }
                }
            }
            JointWords::Arc { f, mm_per_unit, .. } | JointWords::PlaneArc { f, mm_per_unit, .. } => {
                let Some(arc) = w.arc_from(from.linear) else { continue };
                let across = arc.radius() * arc.sweep().abs();
                let len = arc.length();
                let nominal = len / (f * mm_per_unit);
                if !(nominal.is_finite() && nominal > 0.0) {
                    continue;
                }
                peak(&mut t.peak_feed_mm_min, line, len / nominal);
                // Round an arc each linear axis in turn moves as fast as the tool does.
                let per = |dist: f64, rate: Option<f64>| rate.map_or(0.0, |r| dist / r);
                let (round, along, normal) = arc_rates(arc.plane, d, xy, z);
                let run = nominal.max(per(across, round)).max(per(along, normal));
                t.written_cut_minutes += nominal;
                t.cut_minutes += run;
                if run > nominal * (1.0 + SLOWED) {
                    t.slowed_blocks += 1;
                    t.feed_over += 1;
                    first_feed_over.get_or_insert(line);
                    peak(&mut worst_feed_over, line, len / nominal);
                }
            }
            JointWords::Dwell { .. } => {}
        }
    }
    if low.iter().all(|v| v.is_finite()) {
        t.span_mm = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    }

    // The spindle: its starts, and speeds outside its range.
    let mut out_of_range = 0usize;
    let mut on = false;
    let mut starts = 0usize;
    for (i, text_line) in text.lines().enumerate() {
        let n = i + 1;
        let ws = words(text_line, n)?;
        let code = |letter: char, v: i64| ws.iter().any(|&(c, x)| c == letter && (x * 10.0).round() as i64 == v * 10);
        if let (Some([lo, hi]), Some(&(_, s))) = (rates.spindle_rpm, ws.iter().find(|(c, _)| *c == 'S'))
            && !(s >= lo && s <= hi)
        {
            out_of_range += 1;
            if t.spindle_out_of_range.len() < MAX_LISTED {
                t.spindle_out_of_range.push((n, s));
            }
        }
        if code('M', 3) || code('M', 4) {
            if !on {
                starts += 1;
            }
            on = true;
        }
        if code('M', 5) || code('M', 2) || code('M', 30) {
            on = false;
        }
    }
    t.spindle_minutes = rates.spindle_start_seconds.unwrap_or(0.0) * starts as f64 / 60.0;
    t.tool_changes = tool_change_lines(text)?.len();
    t.tool_change_minutes = rates.tool_change_seconds.unwrap_or(0.0) * t.tool_changes as f64 / 60.0;
    t.stops = stop_lines(text)?.len();
    t.stop_minutes = STOP_MINUTES * t.stops as f64;
    t.total_minutes = t.cut_minutes + t.rapid_minutes + t.spindle_minutes + t.tool_change_minutes + t.stop_minutes;

    let rotary = match letters {
        [one] => one.char().to_string(),
        _ => "a rotary".into(),
    };
    let fmt = |v: f64| number(v, 0);
    if t.rotary_over > 0
        && let (Some(max), Some((l, p)), Some(first)) = (a, t.peak_rotary_deg_min, first_rotary_over)
    {
        let w = format!(
            "{} cutting blocks (the first at line {first}) ask {rotary} to turn faster than its {}°/min (up to {}°/min at line {l}): \
             the controller slows them, so they take longer than written and cut at a lower feed",
            t.rotary_over,
            fmt(max),
            fmt(p)
        );
        t.limits.push(w.clone());
        t.warnings.push(w);
    }
    if t.feed_over > 0
        && let Some(first) = first_feed_over
    {
        let peak = worst_feed_over.map_or(String::new(), |(l, p)| format!(" (up to {} mm/min at line {l})", fmt(p)));
        let w = format!(
            "{} cutting blocks (the first at line {first}) ask a linear axis for more than its top feed{peak}: the controller \
             slows them, so they take longer than written",
            t.feed_over
        );
        t.limits.push(w.clone());
        t.warnings.push(w);
    }
    if untimed_turns > 0 {
        t.warnings.push(format!("{untimed_turns} rapids turn {rotary}, which is not timed: the machine's rates give no `rapid_a_deg_min`"));
    }
    if let (Some(range), Some(&first)) = (rates.spindle_rpm, t.spindle_out_of_range.first()) {
        let w = spindle_out_of_range_warning(out_of_range, range, first);
        t.limits.push(w.clone());
        t.warnings.push(w);
    }
    if let Some(travel) = rates.travel_mm {
        for ((axis, span), limit) in ['X', 'Y', 'Z'].into_iter().zip(t.span_mm).zip(travel) {
            if span > limit + 1e-6 {
                t.warnings.push(format!(
                    "the program spans {} mm along {axis}, more than the machine's {} mm of travel",
                    number(span, 3),
                    number(limit, 3)
                ));
            }
        }
    }
    Ok(t)
}

/// The warning for `count` spindle speeds outside the machine's `range`, the first `S{s}` at
/// line `l`: the controller runs them at the nearest end of its range.
fn spindle_out_of_range_warning(count: usize, range: [f64; 2], (l, s): (usize, f64)) -> String {
    let fmt = |v: f64| number(v, 0);
    format!(
        "{count} spindle speeds are outside the machine's {}-{} rpm (S{} at line {l}): the controller runs them at the nearest end of its range",
        fmt(range[0]),
        fmt(range[1]),
        fmt(s)
    )
}

/// The warning [`machine_time`] gives for the spindle speeds (`S` words) of `text` outside the
/// machine's `range` (lowest and highest rpm), for a simulator that does not time the program;
/// `None` when every speed is in range.
pub fn spindle_range_warning(text: &str, range: [f64; 2]) -> Result<Option<String>> {
    let [lo, hi] = range;
    let mut count = 0usize;
    let mut first = None;
    for (i, text_line) in text.lines().enumerate() {
        let n = i + 1;
        if let Some(&(_, s)) = words(text_line, n)?.iter().find(|(c, _)| *c == 'S')
            && !(s >= lo && s <= hi)
        {
            count += 1;
            first = first.or(Some((n, s)));
        }
    }
    Ok(first.map(|f| spindle_out_of_range_warning(count, range, f)))
}

/// The operations whose spindle speed the machine cannot run: `range` is its lowest and highest
/// rpm, `ops` each operation's name, speed and whether its tool is HSS cutting a material that
/// burns an HSS edge run over its speed (steel and harder). A speed below the range is run at its
/// lowest and one above at its highest (the job says so as a warning); refused are only a speed
/// that is not a number and an HSS tool in such a material run more than `over` times its own
/// speed, as the planner refuses it. `None` when nothing is refused, else the refusal with its fix.
pub fn spindle_refusal<'a>(ops: impl IntoIterator<Item = (&'a str, f64, bool)>, range: [f64; 2], over: f64) -> Option<String> {
    let [lo, hi] = range;
    let refused = |s: f64, hss_burns: bool| !s.is_finite() || s <= 0.0 || (hss_burns && s < lo && lo > s * over);
    let out: Vec<(&str, f64)> = ops.into_iter().filter(|&(_, s, b)| refused(s, b)).map(|(n, s, _)| (n, s)).collect();
    let &(name, s) = out.first()?;
    if !(s.is_finite() && s > 0.0) {
        return Some(format!(
            "spindle: {} operations ask for a spindle speed the machine cannot run ({}-{} rpm); the first, `{name}`, asks for {s} rpm: give the tool a `spindle_rpm` or `surface_speed`",
            out.len(),
            number(lo, 0),
            number(hi, 0)
        ));
    }
    Some(format!(
        "spindle: {} operations run an HSS tool far over its speed ({}× or more) in a material that burns its edge; the first, `{name}`, asks for {} rpm and the spindle's slowest is {} rpm: use a carbide tool whose speed is at least {} rpm",
        out.len(),
        number(over, 1),
        number(s, 0),
        number(lo, 0),
        number((lo / over).round(), 0)
    ))
}

/// The lines (from 1) where `text` changes the tool: an `M6`, or a programmed stop (`M0`),
/// which is how a program asks for a tool on a controller that ignores `M6`. A change written as
/// both (`T2 M6` then `M0`, no move between) counts once, at its first line.
///
/// A stop tagged for probing, the zero check or a tool check ([`stop_lines`]) is not a tool change.
pub fn tool_change_lines(text: &str) -> Result<Vec<usize>> {
    let tagged: Vec<usize> = stop_lines(text)?.into_iter().map(|s| s.line).collect();
    let mut out = Vec::new();
    let mut moved = true;
    for (i, text_line) in text.lines().enumerate() {
        let n = i + 1;
        let ws = words(text_line, n)?;
        let code = |letter: char, v: i64| ws.iter().any(|&(c, x)| c == letter && (x * 10.0).round() as i64 == v * 10);
        if code('M', 0) && !code('M', 6) && tagged.binary_search(&n).is_ok() {
            continue;
        }
        if code('M', 6) || code('M', 0) {
            if moved {
                out.push(n);
            }
            moved = false;
        } else if ws.iter().any(|&(c, _)| matches!(c, 'X' | 'Y' | 'Z' | 'A' | 'B' | 'C')) {
            moved = true;
        }
    }
    Ok(out)
}

/// The operator's time at each tagged stop ([`stop_lines`]), minutes.
pub const STOP_MINUTES: f64 = 1.0;

/// The tag a programmed stop's comment starts with to say what the stop is for: `SC-STOP`.
pub const STOP_TAG: &str = "SC-STOP";

/// What a tagged programmed stop is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    /// The operator puts the probe in and probes the stock (program 0).
    Probe,
    /// The probe stops 5 mm above work zero for the operator to check it.
    ZeroCheck,
    /// The tool stops a gauge's gap above a fixed reference for the operator to check its length.
    ToolCheck,
}

impl StopKind {
    pub fn code(self) -> &'static str {
        match self {
            StopKind::Probe => "probe",
            StopKind::ZeroCheck => "zero_check",
            StopKind::ToolCheck => "tool_check",
        }
    }

    /// The word the tag writes (`PROBE`, `ZERO-CHECK`, `TOOL-CHECK`).
    pub fn tag(self) -> &'static str {
        match self {
            StopKind::Probe => "PROBE",
            StopKind::ZeroCheck => "ZERO-CHECK",
            StopKind::ToolCheck => "TOOL-CHECK",
        }
    }
}

/// A tagged programmed stop ([`stop_lines`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    /// The `M0`'s line, from 1.
    pub line: usize,
    pub kind: StopKind,
    /// The tool a tool check names (`T3`).
    pub tool: Option<u32>,
    /// The gap a tool check names (`GAP=10.`), mm.
    pub gap: Option<f64>,
    /// What the controller shows the operator (the `MSG,` comment before the stop), empty when
    /// there is none.
    pub says: String,
}

/// The text of a line's comment (`(…)` or `; …`), upper case; `None` without one.
fn comment_of(line: &str) -> Option<String> {
    let t = line.trim();
    let inner = if let Some(open) = t.find('(') {
        let rest = t.get(open + 1..)?;
        rest.split(')').next().unwrap_or(rest)
    } else {
        t.split_once(';').map(|(_, c)| c)?
    };
    Some(inner.trim().to_ascii_uppercase())
}

/// The kind, tool and gap a stop tag says (`SC-STOP TOOL-CHECK T3 GAP=10.`), read with the
/// separators a post may drop (`-`, `_`, `=`) left out; `None` for any other comment.
pub fn stop_tag(comment: &str) -> Option<(StopKind, Option<u32>, Option<f64>)> {
    let squeeze = |w: &str| w.chars().filter(|c| !matches!(c, '-' | '_' | '=')).collect::<String>().to_ascii_uppercase();
    let mut tokens = comment.split_whitespace().map(squeeze).filter(|w| !w.is_empty());
    let first = tokens.next()?;
    let tag = squeeze(STOP_TAG);
    let kind_word = if first == tag { tokens.next()? } else { first.strip_prefix(&tag)?.to_string() };
    let kind = match kind_word.as_str() {
        "PROBE" => StopKind::Probe,
        "ZEROCHECK" => StopKind::ZeroCheck,
        "TOOLCHECK" => StopKind::ToolCheck,
        _ => return None,
    };
    let (mut tool, mut gap) = (None, None);
    for w in tokens {
        if let Some(g) = w.strip_prefix("GAP") {
            gap = g.parse::<f64>().ok().filter(|g| g.is_finite());
        } else if let Some(t) = w.strip_prefix('T') {
            tool = t.parse::<u32>().ok();
        }
    }
    Some((kind, tool, gap))
}

/// The tagged programmed stops of `text`: an `M0` whose stop is said by an `SC-STOP` comment
/// ([`stop_tag`]) since the stop before it, with the `MSG,` comment shown with it. An untagged
/// `M0` is not listed (it is a tool change: [`tool_change_lines`]).
pub fn stop_lines(text: &str) -> Result<Vec<Stop>> {
    let mut out = Vec::new();
    let mut tag: Option<(StopKind, Option<u32>, Option<f64>)> = None;
    let mut says = String::new();
    for (i, text_line) in text.lines().enumerate() {
        let n = i + 1;
        if let Some(c) = comment_of(text_line) {
            if let Some(t) = stop_tag(&c) {
                tag = Some(t);
            } else if let Some(m) = c.strip_prefix("MSG") {
                says = m.trim_start_matches([',', ' ']).trim().to_string();
            }
        }
        let ws = words(text_line, n)?;
        if ws.iter().any(|&(c, x)| c == 'M' && (x * 10.0).round() == 0.0) {
            if let Some((kind, tool, gap)) = tag.take() {
                out.push(Stop { line: n, kind, tool, gap, says: std::mem::take(&mut says) });
            }
            says.clear();
        }
    }
    Ok(out)
}

/// Why a program waits between moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    SpindleStart,
    ToolChange,
}

/// A wait at a line ([`BlockTimes::waits`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wait {
    pub line: usize,
    pub seconds: f64,
    pub kind: WaitKind,
}

/// Every block of a program timed ([`block_times`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockTimes {
    /// Seconds per block [`parse5`] reads (dwells included), in its order; the first block whose
    /// every axis is known only says where they are (0).
    pub seconds: Vec<f64>,
    /// Each block's line.
    pub lines: Vec<usize>,
    /// Spindle starts and tool changes, in line order.
    pub waits: Vec<Wait>,
    /// Whether the blocks were timed with acceleration.
    pub accelerated: bool,
    /// All of it, seconds.
    pub total: f64,
}

/// Time to cover `l` starting at `v0`, cruising at up to `v`, ending at `v1`, speeding up and
/// slowing down at `a` (consistent units: mm, mm/s, mm/s²).
pub fn trapezoid(l: f64, v0: f64, v: f64, v1: f64, a: f64) -> f64 {
    if !(l > 1e-12) || !(v > 0.0) {
        return 0.0;
    }
    if !(a > 0.0 && a.is_finite()) {
        return l / v;
    }
    let (v0, v1) = (v0.clamp(0.0, v), v1.clamp(0.0, v));
    let d_acc = (v * v - v0 * v0) / (2.0 * a);
    let d_dec = (v * v - v1 * v1) / (2.0 * a);
    if d_acc + d_dec <= l {
        (v - v0) / a + (v - v1) / a + (l - d_acc - d_dec) / v
    } else {
        // A triangle: the peak speed it reaches.
        let vp = ((2.0 * a * l + v0 * v0 + v1 * v1) / 2.0).sqrt();
        (vp - v0).max(0.0) / a + (vp - v1).max(0.0) / a
    }
}

/// One block as [`block_times`] sees it: its length (mm, or degrees for a turn alone), its top
/// speed (a second), how fast it may speed up (a second squared), where it heads at its start
/// and end, and whether it is a rapid.
#[derive(Clone, Copy)]
struct Leg {
    length: f64,
    speed: f64,
    accel: f64,
    start: Vec3,
    end: Vec3,
    rapid: bool,
    /// A stop before it (a dwell, an unknown position).
    stop: bool,
}

/// The acceleration a move along unit `u` may have when no axis may have more than its own.
fn accel_along(u: Vec3, xy: f64, z: f64) -> f64 {
    let mut a = f64::INFINITY;
    for (c, lim) in [(u.x, xy), (u.y, xy), (u.z, z)] {
        if c.abs() > 1e-9 {
            a = a.min(lim / c.abs());
        }
    }
    if a.is_finite() { a } else { xy }
}

/// Times every block of `text` (absolute G-code, rotaries `letters`, dwells in `dwell`) as a
/// machine with `rates` runs it; see the module documentation. Without `accel_mm_s2` each block
/// takes what [`machine_time`] gives it (rapids at their slowest axis, cuts held to the top
/// feeds), and a turn alone without `accel_a_deg_s2` its top rate throughout.
pub fn block_times(text: &str, letters: &[AxisLetter], rates: &MachineRates, dwell: DwellUnit) -> Result<BlockTimes> {
    rates.validate()?;
    let written = parse5(text, letters)?;
    let (xy, z, a) = (rates.max_feed_mm_min, rates.feed_z(), rates.max_a_deg_min);
    let mut out = BlockTimes { accelerated: rates.accel_mm_s2.is_some() || rates.accel_a_deg_s2.is_some(), ..BlockTimes::default() };
    let mut legs: Vec<Option<Leg>> = Vec::with_capacity(written.len());
    let mut at: Option<At> = None;
    let mut stop = true;
    for w in &written {
        let (line, linear, rotary) = match w {
            JointWords::Dwell { line, seconds } => {
                out.seconds.push(dwell.seconds(*seconds).max(0.0));
                out.lines.push(*line);
                legs.push(None);
                stop = true;
                continue;
            }
            JointWords::Rapid { line, linear, rotary, .. }
            | JointWords::Feed { line, linear, rotary, .. }
            | JointWords::Arc { line, linear, rotary, .. }
            | JointWords::PlaneArc { line, linear, rotary, .. } => (*line, *linear, rotary),
        };
        out.lines.push(line);
        let Some(from) = at.replace(At { linear, rotary: rotary.clone(), line }) else {
            out.seconds.push(0.0);
            legs.push(None);
            stop = true;
            continue;
        };
        let d = linear - from.linear;
        let turn = turn_deg(&from.rotary, rotary);
        let dir = d.normalized().unwrap_or(Vec3::ZERO);
        let (minutes, length, start, end, rapid) = match w {
            JointWords::Rapid { .. } => {
                let m = axis_minutes(d, turn, Some(rates.rapid_xyz_mm_min), Some(rates.rapid_z()), rates.rapid_a_deg_min);
                (m, if d.len() > 1e-12 { d.len() } else { turn }, dir, dir, true)
            }
            JointWords::Feed { f, inverse_time, mm_per_unit, .. } => {
                let len = d.len();
                let nominal = if *inverse_time {
                    1.0 / f
                } else if len > 1e-12 {
                    len / (f * mm_per_unit)
                } else {
                    turn / f
                };
                let m = if nominal.is_finite() && nominal > 0.0 { nominal.max(axis_minutes(d, turn, xy, z, a)) } else { 0.0 };
                (m, if len > 1e-12 { len } else { turn }, dir, dir, false)
            }
            JointWords::Arc { f, mm_per_unit, .. } | JointWords::PlaneArc { f, mm_per_unit, .. } => {
                let Some(arc) = w.arc_from(from.linear) else { continue };
                let across = arc.radius() * arc.sweep().abs();
                let len = arc.length();
                let nominal = len / (f * mm_per_unit);
                let per = |dist: f64, rate: Option<f64>| rate.map_or(0.0, |r| dist / r);
                let (round, along, normal) = arc_rates(arc.plane, d, xy, z);
                let m = if nominal.is_finite() && nominal > 0.0 { nominal.max(per(across, round)).max(per(along, normal)) } else { 0.0 };
                // The tangents at the ends.
                (m, len, arc.tangent(0.0).unwrap_or(dir), arc.tangent(1.0).unwrap_or(dir), false)
            }
            JointWords::Dwell { .. } => (0.0, 0.0, dir, dir, false),
        };
        let seconds = minutes * 60.0;
        out.seconds.push(seconds);
        let accel = match (d.len() > 1e-12, rates.accel_mm_s2) {
            (true, Some(axy)) => accel_along(dir, axy, rates.accel_z_mm_s2.unwrap_or(axy)),
            (false, _) => rates.accel_a_deg_s2.unwrap_or(f64::INFINITY),
            (true, None) => f64::INFINITY,
        };
        let mut speed = if seconds > 1e-12 { length / seconds } else { 0.0 };
        if let Some(arc) = w.arc_from(from.linear) {
            let r = arc.radius();
            if r > 1e-9 && accel.is_finite() {
                speed = speed.min((accel * r).sqrt());
            }
        }
        legs.push(Some(Leg { length, speed, accel, start, end, rapid, stop }));
        stop = false;
    }
    if out.accelerated {
        // The speed at each junction, then each block a trapezoid between them.
        let mut v_in = 0.0f64;
        for k in 0..legs.len() {
            let Some(Some(leg)) = legs.get(k).copied() else {
                v_in = 0.0;
                continue;
            };
            if leg.stop {
                v_in = 0.0;
            }
            let v_out = match legs.get(k + 1).copied().flatten() {
                Some(next) if !next.stop && next.rapid == leg.rapid => leg.speed.min(next.speed) * leg.end.dot(next.start).clamp(0.0, 1.0),
                _ => 0.0,
            };
            if leg.speed > 0.0
                && leg.accel.is_finite()
                && let Some(s) = out.seconds.get_mut(k)
            {
                *s = trapezoid(leg.length, v_in.min(leg.speed), leg.speed, v_out.min(leg.speed), leg.accel);
            }
            v_in = v_out;
        }
    }
    // Spindle starts and tool changes.
    let changes = tool_change_lines(text)?;
    let mut on = false;
    for (i, text_line) in text.lines().enumerate() {
        let n = i + 1;
        let ws = words(text_line, n)?;
        let code = |letter: char, v: i64| ws.iter().any(|&(c, x)| c == letter && (x * 10.0).round() as i64 == v * 10);
        if changes.binary_search(&n).is_ok()
            && let Some(s) = rates.tool_change_seconds
        {
            out.waits.push(Wait { line: n, seconds: s, kind: WaitKind::ToolChange });
        }
        if code('M', 3) || code('M', 4) {
            if !on && let Some(s) = rates.spindle_start_seconds {
                out.waits.push(Wait { line: n, seconds: s, kind: WaitKind::SpindleStart });
            }
            on = true;
        }
        if code('M', 5) || code('M', 2) || code('M', 30) {
            on = false;
        }
    }
    out.total = out.seconds.iter().sum::<f64>() + out.waits.iter().map(|w| w.seconds).sum::<f64>();
    Ok(out)
}
