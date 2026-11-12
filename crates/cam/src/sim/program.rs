//! Reading the program: every motion block as a step the stock is cut along, with the tool in
//! the spindle, the spindle's speed, where the material must be for a slot, and the time at feed.

use super::*;

/// Most chords of one arc.
pub(super) const MAX_BLOCK_POINTS: usize = 1_000_000;

/// Where the axes are after a block.
#[derive(Clone)]
pub(super) struct State {
    pub(super) linear: Vec3,
    pub(super) rotary: Vec<f64>,
    pub(super) line: usize,
}

/// The tool's pose with the axes at `linear` and `rotary`.
pub(super) fn pose_at(machine: &Machine, linear: Vec3, rotary: &[f64]) -> Pose {
    forward(machine, &Joints { linear, rotary: rotary.to_vec() })
}

/// What the simulation reads of a program beside [`parse5`]'s motion.
#[derive(Default)]
pub(super) struct Scan {
    /// The tool loaded at each line (`T` with `M6`), in line order.
    pub(super) changes: Vec<(usize, u32)>,
    /// The lines that lose the position (a return home or a move in machine coordinates).
    pub(super) gaps: Vec<usize>,
    /// The spindle after each line that changes it (`S`, `M3`, `M4`, `M5`, `M2`, `M30`), in line
    /// order: its speed when it turns, 0 when it is stopped. Before the first `M3`/`M4`/`M5` it is
    /// not known (a program that never says is not taken as cutting with it stopped).
    pub(super) spindle: Vec<(usize, f64)>,
}

impl Scan {
    pub(super) fn of(text: &str) -> Result<Scan> {
        let mut scan = Scan::default();
        let mut pending: Option<u32> = None;
        let (mut rpm, mut on): (f64, Option<bool>) = (0.0, None);
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let ws = words(line, n)?;
            let mut m6 = false;
            let was = (rpm, on);
            for &(c, v) in &ws {
                match c {
                    'S' if v.is_finite() && v >= 0.0 => rpm = v,
                    'M' if [3.0, 4.0].iter().any(|m| (v - m).abs() < 1e-9) => on = Some(true),
                    'M' if [5.0, 2.0, 30.0].iter().any(|m| (v - m).abs() < 1e-9) => on = Some(false),
                    'T' => {
                        if !((0.0..=1.0e6).contains(&v) && v.fract() == 0.0) {
                            return Err(CamError::Parse(n, "a tool number must be a whole number".into()));
                        }
                        pending = Some(v as u32);
                    }
                    'M' if (v - 6.0).abs() < 1e-9 => m6 = true,
                    'G' => match (v * 10.0).round() as i64 {
                        280 | 300 | 530 => scan.gaps.push(n),
                        _ => {}
                    },
                    _ => {}
                }
            }
            if (rpm, on) != was
                && let Some(turning) = on
            {
                scan.spindle.push((n, if turning { rpm } else { 0.0 }));
            }
            if m6 {
                let Some(t) = pending else { return Err(CamError::Parse(n, "a tool change (M6) without a tool number (T)".into())) };
                scan.changes.push((n, t));
            }
        }
        Ok(scan)
    }

    /// The spindle's speed at `line` (0: stopped), when the program has said.
    pub(super) fn rpm_at(&self, line: usize) -> Option<f64> {
        let k = self.spindle.partition_point(|(l, _)| *l <= line).checked_sub(1)?;
        self.spindle.get(k).map(|(_, r)| *r)
    }
}

/// The tool in the spindle at `line`: its slot in `tools`.
pub(super) fn tool_at(changes: &[(usize, u32)], tools: &[(u32, Profile)], line: usize) -> Result<usize> {
    let loaded = changes.partition_point(|(l, _)| *l <= line).checked_sub(1).and_then(|k| changes.get(k)).map(|(_, t)| *t);
    match loaded {
        Some(t) => tools
            .iter()
            .position(|(n, _)| *n == t)
            .ok_or_else(|| CamError::Parse(line, format!("the program uses tool {t}, which is not among the tools given"))),
        None => match tools {
            [_] => Ok(0),
            _ => Err(CamError::Parse(line, "a move before any tool change (T… M6), with more than one tool given".into())),
        },
    }
}

/// How the tool goes through one block.
pub(super) enum How {
    /// Straight in the work frame from the start pose to the end (down the tool axis from
    /// above, when the position was not known).
    Straight,
    /// Every axis evenly from `from` to `to`.
    Joint { from: Joints, to: Joints },
    /// Round an arc of the linear axes (rotaries held) about `center`, turning `sweep`;
    /// `work_center` is the centre in work coordinates (3-axis).
    Arc { from: Vec3, to: Vec3, center: Vec2, work_center: Vec2, sweep: f64, rotary: Vec<f64> },
    /// An arc in the XZ (`G18`) or YZ (`G19`) plane, the rotaries held, as `chords` chords on a
    /// 3-axis machine.
    Tilted { arc: PlaneArc, chords: usize, rotary: Vec<f64> },
}

/// The plane an arc is in (`G17`, `G18`, `G19`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Plane {
    Xy,
    Zx,
    Yz,
}

impl Plane {
    /// The plane's first and second axes and its normal (indices into X, Y, Z): turning from
    /// the first towards the second is counter-clockwise (`G3`) seen from the normal's +end.
    pub(super) fn axes(self) -> (usize, usize, usize) {
        match self {
            Plane::Xy => (0, 1, 2),
            Plane::Zx => (2, 0, 1),
            Plane::Yz => (1, 2, 0),
        }
    }
}

pub(super) fn comp(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

pub(super) fn with(mut v: Vec3, i: usize, value: f64) -> Vec3 {
    match i {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
    v
}

/// An arc of the linear axes in a plane of them, moving evenly along the plane's normal.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlaneArc {
    pub(super) from: Vec3,
    pub(super) to: Vec3,
    pub(super) center: Vec3,
    pub(super) plane: Plane,
    pub(super) radius: f64,
    pub(super) start: f64,
    pub(super) sweep: f64,
}

impl PlaneArc {
    /// The arc from `from` to `to` about `center` (only its in-plane coordinates count), as
    /// the controller runs it: an end at the start is a whole turn.
    pub(super) fn new(from: Vec3, to: Vec3, center: Vec3, plane: Plane, ccw: bool) -> PlaneArc {
        let (u, v, _) = plane.axes();
        let s = Vec2::new(comp(from, u) - comp(center, u), comp(from, v) - comp(center, v));
        let e = Vec2::new(comp(to, u) - comp(center, u), comp(to, v) - comp(center, v));
        let sweep = if s.dist(e) <= 1e-9 {
            if s.len() > 1e-9 { if ccw { TAU } else { -TAU } } else { 0.0 }
        } else {
            let delta = (e.angle() - s.angle()).rem_euclid(TAU);
            let delta = if delta.is_finite() { delta } else { 0.0 };
            if ccw { delta } else { delta - TAU }
        };
        PlaneArc { from, to, center, plane, radius: s.len(), start: s.angle(), sweep }
    }

    /// The point a fraction `t` of the way along.
    pub(super) fn at(&self, t: f64) -> Vec3 {
        if t >= 1.0 {
            return self.to;
        }
        let (u, v, w) = self.plane.axes();
        let p = Vec2::from_angle(self.start + self.sweep * t) * self.radius;
        let q = with(self.from, u, comp(self.center, u) + p.x);
        let q = with(q, v, comp(self.center, v) + p.y);
        with(q, w, comp(self.from, w) + (comp(self.to, w) - comp(self.from, w)) * t)
    }

    pub(super) fn length(&self) -> f64 {
        let (_, _, w) = self.plane.axes();
        (self.radius * self.sweep.abs()).hypot(comp(self.to, w) - comp(self.from, w))
    }

    /// Chords at most `e` inside the arc.
    pub(super) fn chords(&self, e: f64) -> Result<usize> {
        let e = e.min(self.radius);
        let step = if self.radius > 1e-9 { 2.0 * (1.0 - e / self.radius).clamp(-1.0, 1.0).acos() } else { TAU };
        let n = (self.sweep.abs() / step.max(1e-9)).ceil().max(1.0);
        if !(n <= MAX_BLOCK_POINTS as f64) {
            return Err(CamError::TooLarge("an arc needs too many chords to simulate".into()));
        }
        Ok(n as usize)
    }
}

/// One block as the simulation runs it.
pub(super) struct Step {
    pub(super) profile: Profile,
    pub(super) start: Pose,
    pub(super) end: Pose,
    pub(super) how: How,
    /// Tool positions along it on a 4th axis.
    pub(super) points: usize,
    /// Where the material must be for a cut at the tool's full width: under the tool and at
    /// both sides of it, halfway along.
    pub(super) probes: Option<[Vec3; 3]>,
    pub(super) line: usize,
    pub(super) rapid: bool,
    /// The tool's slot among the tools given, and its flute length.
    pub(super) slot: u8,
    pub(super) flute: f64,
    /// The spindle's speed (0: stopped), when the program has said.
    pub(super) rpm: Option<f64>,
    /// The axes at its start, when known.
    pub(super) from: Option<Joints>,
    /// The distance the tool travels (mm; with the rotaries' sweep on a 4th axis).
    pub(super) travel: f64,
    /// About how many cells it visits.
    pub(super) cost: f64,
}

impl Step {
    /// The pose a fraction `t` of the way along, as the 4th axis's tool positions are placed.
    pub(super) fn pose_frac(&self, machine: &Machine, t: f64) -> Pose {
        match &self.how {
            How::Straight => Pose { tip: self.start.tip.lerp(self.end.tip, t), axis: self.end.axis },
            How::Joint { from, to } => {
                let j = from.lerp(to, t);
                pose_at(machine, j.linear, &j.rotary)
            }
            How::Arc { from, to, center, sweep, rotary, .. } => pose_at(machine, arc_linear(*from, *to, *center, *sweep, t), rotary),
            How::Tilted { arc, rotary, .. } => pose_at(machine, arc.at(t), rotary),
        }
    }

    /// The 4th axis's tool positions: indices `first..=last` of [`Step::pose_frac`] at `k / n`.
    pub(super) fn pose_range(&self) -> (usize, usize) {
        let n = self.points.max(1);
        match self.how {
            How::Straight => (0, n),
            _ => (1, n),
        }
    }
}

/// The linear axes a fraction `t` round an arc of them from `from` to `to` about `center`.
pub(super) fn arc_linear(from: Vec3, to: Vec3, center: Vec2, sweep: f64, t: f64) -> Vec3 {
    if t >= 1.0 {
        return to;
    }
    let st = Vec2::new(from.x, from.y) - center;
    (center + Vec2::from_angle(st.angle() + sweep * t) * st.len()).extend(from.z + (to.z - from.z) * t)
}

/// Of `total` pieces of work, those done a fraction `f` of the way.
pub(super) fn done(total: usize, f: f64) -> usize {
    if f >= 1.0 { total } else { ((f.max(0.0) * total as f64).floor() as usize).min(total) }
}

/// The point a fraction `f` round a level arc (or helix) from `a` to `b` about `center`.
pub(super) fn arc_point(a: Vec3, b: Vec3, center: Vec2, sweep: f64, f: f64) -> Vec3 {
    if f <= 0.0 {
        return a;
    }
    arc_linear(a, b, center, sweep, f)
}

/// A program read for the simulation: its steps (one per motion block whose position is known),
/// the grid of the stock and what times it. See [`plan_sim`].
pub struct SimPlan {
    pub(super) machine: Machine,
    pub(super) steps: Vec<Step>,
    pub(super) grid: Grid,
    pub(super) cell: f64,
    pub(super) tol: f64,
    pub(super) initial: f64,
    pub(super) minutes: f64,
    pub(super) feed_length: f64,
    pub(super) rapid_length: f64,
    /// The tools by slot: number.
    pub(super) tools: Vec<u32>,
    /// The rows (angles) of each band, from and to (multiples of [`TILE`], the last excepted).
    pub(super) bands: Vec<(usize, usize)>,
}

/// One step of a [`SimPlan`], as a view shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct StepView {
    /// The G-code line, from 1.
    pub line: usize,
    pub rapid: bool,
    /// An arc (in any plane).
    pub arc: bool,
    pub tool: u32,
    pub slot: u8,
    /// The spindle's speed (0: stopped), when the program has said.
    pub rpm: Option<f64>,
    pub start: Pose,
    pub end: Pose,
    /// Whether the position before it was known (otherwise it comes down the tool axis from
    /// above the stock).
    pub known: bool,
    /// The distance it travels, mm.
    pub travel: f64,
}

impl SimPlan {
    /// Steps: motion blocks simulated.
    pub fn moves(&self) -> usize {
        self.steps.len()
    }

    pub fn step(&self, k: usize) -> Option<StepView> {
        let s = self.steps.get(k)?;
        Some(StepView {
            line: s.line,
            rapid: s.rapid,
            arc: matches!(s.how, How::Arc { .. } | How::Tilted { .. }),
            tool: self.tools.get(usize::from(s.slot)).copied().unwrap_or(0),
            slot: s.slot,
            rpm: s.rpm,
            start: s.start,
            end: s.end,
            known: s.from.is_some(),
            travel: s.travel,
        })
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The cell size (mm).
    pub fn cell(&self) -> f64 {
        self.cell
    }

    pub fn cells(&self) -> usize {
        self.grid.columns().saturating_mul(self.grid.rows())
    }

    pub fn tolerance(&self) -> f64 {
        self.tol
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// The number of the tool in `slot`.
    pub fn tool_number(&self, slot: u8) -> Option<u32> {
        self.tools.get(usize::from(slot)).copied()
    }

    /// Time at feed as the controller times it, dwells included (minutes).
    pub fn minutes(&self) -> f64 {
        self.minutes
    }

    /// About how many cells cutting `work` visits.
    pub(super) fn cost(&self, work: &[Work]) -> f64 {
        work.iter().map(|&(k, f0, f1)| self.steps.get(k).map_or(0.0, |s| s.cost * (f1 - f0).max(0.0))).sum()
    }

    /// About how many cells cutting step `k` visits.
    pub fn step_cost(&self, k: usize) -> f64 {
        self.steps.get(k).map_or(0.0, |s| s.cost)
    }

    /// The part's height over every cell (its radius along every ray on a 4th axis) where a
    /// ray from above (from outside) first meets it; NaN where none does. For gouges found as
    /// the stock is cut ([`SimState::set_floor`]), and for a stock an earlier setup left held up
    /// to the part ([`SimState::hold_part`]).
    pub fn part_floor(&self, part: &Surface) -> PartFloor {
        let (nu, nv) = (self.grid.columns(), self.grid.rows());
        let grid = self.grid;
        let threaded = !part.is_empty() && nu.saturating_mul(nv) > 100_000;
        let rows = each_threaded(self.bands.clone(), threaded, |(from, to)| {
            let mut out = Vec::with_capacity(nu * to.saturating_sub(from));
            let mut more = Vec::new();
            for j in from..to {
                for i in 0..nu {
                    if part.is_empty() {
                        out.push(f64::NAN);
                        continue;
                    }
                    match grid {
                        Grid::Heights { .. } => out.push(floor_at(&grid, part, i, j)),
                        Grid::Radii { x0, dx, nt, radius, .. } => {
                            let (s, c) = ((j as f64 + 0.5) * TAU / nt.max(1) as f64).sin_cos();
                            let (outer, list) = ray_stretches(part, x0 + (i as f64 + 0.5) * dx, s, c, radius + 1.0);
                            out.push(outer);
                            if let Some(list) = list {
                                more.push((j * nu + i, list));
                            }
                        }
                    }
                }
            }
            Ok((out, more))
        });
        let mut floor = PartFloor { outer: Vec::with_capacity(nu * nv), stretches: HashMap::new() };
        for (r, more) in rows.unwrap_or_default() {
            floor.outer.extend(r);
            floor.stretches.extend(more);
        }
        floor.outer.resize(nu * nv, f64::NAN);
        floor
    }
}

/// The part's height (radius) over cell `(i, j)` of `grid`, NaN when the ray misses it.
pub(super) fn floor_at(grid: &Grid, part: &Surface, i: usize, j: usize) -> f64 {
    match *grid {
        Grid::Heights { top, bottom, .. } => {
            let span = (top - bottom).max(0.0) + 2.0;
            let p = grid.point(i, j, top + 1.0);
            part.raycast(p, Vec3::new(0.0, 0.0, -span)).map_or(f64::NAN, |(t, _)| top + 1.0 - t * span)
        }
        Grid::Radii { radius, .. } => {
            let out = radius + 1.0;
            let p = grid.point(i, j, out);
            part.raycast(p, Vec3::new(p.x, 0.0, 0.0) - p).map_or(f64::NAN, |(t, _)| out * (1.0 - t))
        }
    }
}

/// The part along each cell's column of a grid (each ray from outside towards the axis, on a 4th
/// axis): where it first meets the part (NaN: never; a height, or a radius) and, for the rays
/// that pass through the part more than once (by a cross hole, a pocket in the side), every
/// stretch of the part along them, from the axis out (a stretch from `-∞` runs through the
/// axis). From [`SimPlan::part_floor`], for gouges found as the stock is cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartFloor {
    pub outer: Vec<f64>,
    pub stretches: HashMap<usize, Vec<[f64; 2]>>,
}

/// Reads `text` (G-code for `machine`, in absolute coordinates) into the steps that cut `stock`
/// with `tools` (by number): the first part of [`simulate`].
pub fn plan_sim(text: &str, machine: &Machine, tools: &[Tool], stock: &StockShape, params: &SimParams) -> Result<SimPlan> {
    let tol = params.tolerance;
    if !(1e-4..=1.0).contains(&tol) {
        return invalid("the tolerance must be from 0.0001 to 1 mm");
    }
    let kind = Kind::of(machine)?;
    match (kind, stock) {
        (Kind::Three, StockShape::Box { .. }) | (Kind::Rotary, StockShape::Cylinder { .. } | StockShape::Box { .. }) => {}
        (Kind::Three, _) => return invalid("a 3-axis machine's stock is a box"),
    }
    if tools.is_empty() {
        return invalid("the simulation needs the program's tools");
    }
    let mut profiles: Vec<(u32, Profile)> = Vec::with_capacity(tools.len());
    for t in tools {
        if profiles.iter().any(|(n, _)| *n == t.number) {
            return invalid(format!("two tools are number {}", t.number));
        }
        profiles.push((t.number, Profile::of(t)?));
    }
    let blocks = parse5(text, &machine.letters())?;
    let scan = Scan::of(text)?;
    let (grid, cell) = grid_for(stock, kind, params)?;
    let (reach, initial) = match (*stock, grid) {
        (StockShape::Box { min, max }, Grid::Radii { radius, .. }) => ((max.x - min.x).hypot(2.0 * radius), radius),
        (StockShape::Box { min, max }, _) => ((max - min).len(), max.z),
        (StockShape::Cylinder { x_min, x_max, radius }, _) => ((x_max - x_min).hypot(2.0 * radius), radius),
    };
    let lever_extra = machine.work_origin.len() + machine.rotaries.iter().map(|r| r.center.len()).sum::<f64>();
    let area = match grid {
        Grid::Heights { dx, dy, .. } => dx * dy,
        Grid::Radii { dx, nt, radius, .. } => dx * (TAU * radius / nt.max(1) as f64),
    };

    // The blocks as steps, in order.
    let mut steps: Vec<Step> = Vec::with_capacity(blocks.len());
    let (mut minutes, mut feed_length, mut rapid_length) = (0.0, 0.0, 0.0);
    let mut stamps = 0usize;
    let mut state: Option<State> = None;
    for b in &blocks {
        let (line, linear, rotary, rapid) = match b {
            JointWords::Rapid { line, linear, rotary } => (*line, *linear, rotary.clone(), true),
            JointWords::Feed { line, linear, rotary, .. }
            | JointWords::Arc { line, linear, rotary, .. }
            | JointWords::PlaneArc { line, linear, rotary, .. } => (*line, *linear, rotary.clone(), false),
            JointWords::Dwell { seconds, .. } => {
                minutes += seconds / 60.0;
                continue;
            }
        };
        let slot = tool_at(&scan.changes, &profiles, line)?;
        let Some(&(_, profile)) = profiles.get(slot) else { return Err(CamError::Parse(line, "a tool that is not among the tools given".into())) };
        let flute = tools.get(slot).map_or(0.0, |t| t.flute_length);
        // The position before this block, unless the program lost it since the last one.
        let gaps = &scan.gaps;
        let lost = |since: usize| gaps.partition_point(|g| *g <= since) < gaps.partition_point(|g| *g <= line);
        let from = state.as_ref().filter(|s| !lost(s.line)).cloned();
        let to = State { linear, rotary, line };
        let end = pose_at(machine, to.linear, &to.rotary);
        if !(end.tip.is_finite() && end.axis.is_finite()) {
            return Err(CamError::Parse(line, "a position that is not a finite number".into()));
        }
        // The first pose: the block's start, or straight down the tool axis from above the stock.
        let start = match &from {
            Some(f) => pose_at(machine, f.linear, &f.rotary),
            None => Pose { tip: end.tip + end.axis * (end.tip.len() + reach + 1.0), axis: end.axis },
        };
        let length = start.tip.dist(end.tip);
        match b {
            JointWords::Rapid { .. } => rapid_length += length,
            JointWords::Feed { f, inverse_time, mm_per_unit, .. } => {
                let lin = from.as_ref().map_or(0.0, |s| s.linear.dist(to.linear));
                let turn = from.as_ref().map_or(0.0, |s| s.rotary.iter().zip(&to.rotary).map(|(a, b)| (b - a).abs()).fold(0.0, f64::max));
                minutes += if *inverse_time {
                    1.0 / f
                } else if lin > 1e-9 {
                    lin / (f * mm_per_unit)
                } else {
                    turn.to_degrees() / f
                };
                feed_length += length;
            }
            JointWords::Arc { .. } | JointWords::PlaneArc { .. } => feed_length += length,
            JointWords::Dwell { .. } => {}
        }
        let (how, travel) = match (b, &from) {
            // An arc in the XZ or YZ plane.
            (JointWords::PlaneArc { center, plane, ccw, f, mm_per_unit, .. }, Some(f0)) => {
                let plane = match plane {
                    ArcPlane::Xy => Plane::Xy,
                    ArcPlane::Zx => Plane::Zx,
                    ArcPlane::Yz => Plane::Yz,
                };
                let arc = PlaneArc::new(f0.linear, to.linear, *center, plane, *ccw);
                let travel = arc.length();
                minutes += travel / (f * mm_per_unit);
                let chords = if kind == Kind::Three { arc.chords(tol * 0.25)? } else { 0 };
                (How::Tilted { arc, chords, rotary: to.rotary.clone() }, travel)
            }
            (JointWords::Arc { center, ccw, f, mm_per_unit, .. }, Some(f0)) => {
                let st = Vec2::new(f0.linear.x, f0.linear.y) - *center;
                let sweep = arc_turn(f0.linear, to.linear, *center, *ccw);
                let travel = (st.len() * sweep).hypot(to.linear.z - f0.linear.z);
                minutes += travel / (f * mm_per_unit);
                let offset = end.tip - to.linear;
                let how = How::Arc {
                    from: f0.linear,
                    to: to.linear,
                    center: *center,
                    work_center: *center + Vec2::new(offset.x, offset.y),
                    sweep,
                    rotary: to.rotary.clone(),
                };
                (how, travel)
            }
            (JointWords::Arc { .. } | JointWords::PlaneArc { .. }, None) => {
                return Err(CamError::Parse(line, "an arc right after the position was lost".into()));
            }
            (_, Some(f0)) => {
                let (a, c) = (Joints { linear: f0.linear, rotary: f0.rotary.clone() }, Joints { linear: to.linear, rotary: to.rotary.clone() });
                let lin = a.linear.dist(c.linear);
                let lever = start.tip.len().max(end.tip.len()) + lin + lever_extra + profile.radius;
                let travel = length.max(lin) + a.rotary_change(&c) * lever;
                (How::Joint { from: a, to: c }, travel)
            }
            (_, None) => (How::Straight, length),
        };
        // Tool positions along the block: only the 4th axis's model places the tool at points
        // (a heightfield sweeps each block exactly), so only it is limited by them.
        let points = if kind == Kind::Rotary {
            let n = points(travel / profile.step(tol))?;
            stamps = stamps.saturating_add(n);
            if stamps > MAX_STAMPS {
                return Err(CamError::TooLarge("the program needs too many tool positions to simulate: give a larger tolerance".into()));
            }
            n
        } else {
            0
        };
        // Where the material must be for a cut at the tool's full width.
        let probes = (!rapid && matches!(how, How::Joint { .. })).then(|| {
            let mid = Pose { tip: start.tip.lerp(end.tip, 0.5), axis: (start.axis + end.axis).normalized().unwrap_or(end.axis) };
            let along = end.tip - start.tip;
            let along = along - mid.axis * along.dot(mid.axis);
            along.normalized().map(|t| {
                let side = mid.axis.cross(t);
                let up = tol.max(0.05 * profile.radius);
                let w = 0.9 * profile.radius;
                let rim = mid.tip + mid.axis * (profile.height(w) + up);
                [mid.tip + mid.axis * up, rim + side * w, rim - side * w]
            })
        });
        let d = 2.0 * profile.radius;
        let cost = if kind == Kind::Rotary { points as f64 * (d * d / area + 1.0) } else { (travel.min(reach * 4.0) + d) * d / area + 1.0 };
        let from_joints = from.as_ref().map(|f| Joints { linear: f.linear, rotary: f.rotary.clone() });
        steps.push(Step {
            profile,
            start,
            end,
            how,
            points,
            probes: probes.flatten(),
            line,
            rapid,
            slot: u8::try_from(slot).unwrap_or(UNCUT - 1).min(UNCUT - 1),
            flute,
            rpm: scan.rpm_at(line),
            from: from_joints,
            travel,
            cost,
        });
        state = Some(to);
    }
    // Bands of whole tiles, so that the tiles of the bands are the tiles of the grid.
    let lines = grid.lines();
    let count = band_count(lines);
    let tiles = lines.div_ceil(TILE).max(1);
    let bands = (0..count).map(|b| ((b * tiles / count * TILE).min(lines), ((b + 1) * tiles / count * TILE).min(lines))).collect();
    Ok(SimPlan {
        machine: machine.clone(),
        steps,
        grid,
        cell,
        tol,
        initial,
        minutes,
        feed_length,
        rapid_length,
        tools: profiles.iter().map(|(n, _)| *n).collect(),
        bands,
    })
}
