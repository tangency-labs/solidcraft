//! Dragging sketch geometry without jumping to another solution.
//!
//! A constrained sketch usually has several solutions for one cursor position (a two-link arm's
//! elbow up or down, a fillet inside or outside its corner). Each held solve of a drag is seeded
//! on the positions from before it: the points the solver's construction places (from distances
//! and alignments to placed points) keep the orientation they had there however far the step.
//! The rest is left to the iteration, which from far away can land on any solution, so a long
//! drag still walks from where the geometry is to where the cursor puts it in short steps, each
//! solved from the previous step's result (continuation). Continuation is what keeps lengths the
//! construction does not read (an Equal to another line, a rigid shape hanging off a held point);
//! it also lets the arc sweep guard see every step, and lets a path that crosses where the
//! geometry cannot reach stop short at the edge of that stretch and go on past it. It steps only
//! when the move is long next to the dragged geometry, so a normal mouse frame is a single solve.

use std::f64::consts::{PI, TAU};

use solidcraft_geom::Vec2;
use solidcraft_sketch::{Analysis, Budget, ConstraintKind, CurveKind, Hold, Sketch, conic_rho_from_point};

use super::{Ent, Left, counted_solve};

/// Longest step, as a fraction of the smallest driving length or radius of the dragged group.
/// Each step is seeded on its pre-step positions, so what the seed constructs keeps its branch
/// whatever the step; the steps are for what it does not (an arc's sweep guard, paths across
/// unreachable stretches). The drag tests first fail at 1 (an arc end wraps through its start):
/// this keeps a factor of four. It holds where the seed placed what it could: once a held
/// solve's decomposition runs out of budget (see [`Budget::exhausted`]), the rest of the drag
/// takes `SHORT_STEP_FRACTION` steps, and continuation keeps the branch.
const STEP_FRACTION: f64 = 0.25;
/// The steps of a drag whose seed did not place everything it could (as before seeding steps).
const SHORT_STEP_FRACTION: f64 = 0.125;
/// Shortest step (mm), so a tiny feature cannot make a drag crawl.
const MIN_STEP: f64 = 1e-3;
/// Most steps one drag is split into.
const MAX_STEPS: usize = 64;
/// After a step fails, the way to it from the last solved state is halved until what is left is
/// shorter than a step over this; at most `MAX_STEPS` such solves per drag.
const HALVINGS: usize = 16;
/// Steps of a drag that isn't live solved at once past a failed step (see [`drag_entity`]).
const AHEAD: usize = 8;
/// An arc whose sweep gets this close (radians) to 0 or a full turn has run into its start.
const SWEEP_EPS: f64 = 1e-6;
/// A step that moves some point further than this many times the sketch's size and the
/// drag's way has thrown the geometry to a far solution: it is taken as failed.
const FLUNG: f64 = 1000.0;

/// What a drag moves, from where it is (`t = 0`) to where the cursor puts it (`t = 1`).
enum Path {
    /// Points slide by `d` (a dragged point by its way to the cursor).
    Slide { pts: Vec<(usize, Vec2)>, d: Vec2 },
    /// A circle's radius or an ellipse's minor radius follows the cursor.
    Radius { c: usize, from: f64, to: f64 },
    /// An arc's ends move along their rays from the centre `o` to the cursor's radius.
    ArcRadius { o: Vec2, ends: Vec<(usize, Vec2)>, from: f64, to: f64 },
    /// A conic's `rho` follows the cursor, read off its position between the chord's midpoint
    /// and the apex (see [`solidcraft_sketch::conic_rho_from_point`]). `a`, `b` and `apex` stay
    /// put; only the curve's fullness changes, the way an ellipse's outline changes its minor
    /// radius rather than sliding the ellipse.
    ConicRho { c: usize, from: f64, to: f64 },
}

impl Path {
    fn length(&self) -> f64 {
        match self {
            Path::Slide { d, .. } => d.len(),
            Path::Radius { from, to, .. } | Path::ArcRadius { from, to, .. } | Path::ConicRho { from, to, .. } => (to - from).abs(),
        }
    }

    /// The points the drag moves.
    fn points(&self) -> impl Iterator<Item = usize> + '_ {
        let pts = match self {
            Path::Slide { pts, .. } | Path::ArcRadius { ends: pts, .. } => pts.as_slice(),
            Path::Radius { .. } | Path::ConicRho { .. } => &[],
        };
        pts.iter().map(|(q, _)| *q)
    }

    /// Put the dragged geometry where it is at `t` along the drag.
    fn apply(&self, sk: &mut Sketch, t: f64) {
        let mut set = |q: usize, at: Vec2| {
            if let Some(pt) = sk.points.get_mut(q) {
                pt.pos = at;
            }
        };
        match self {
            Path::Slide { pts, d } => {
                for &(q, at) in pts {
                    set(q, at + *d * t);
                }
            }
            Path::ArcRadius { o, ends, from, to } => {
                let r = from + (to - from) * t;
                for &(q, u) in ends {
                    set(q, *o + u * r);
                }
            }
            Path::Radius { c, from, to } => {
                if let Some(CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }) = sk.curves.get_mut(*c).map(|x| &mut x.kind) {
                    *r = from + (to - from) * t;
                }
            }
            Path::ConicRho { c, from, to } => {
                if let Some(CurveKind::Conic { rho, .. }) = sk.curves.get_mut(*c).map(|x| &mut x.kind) {
                    *rho = from + (to - from) * t;
                }
            }
        }
    }
}

/// Drag: the grabbed entity follows the cursor where possible (a point or a material point on a
/// line moves toward it; a spline slides by the cursor's move; a circle or arc changes radius about its
/// centre; a conic grabbed on its curve reads its fullness, `rho`, off the cursor) and the rest
/// of the sketch moves as little as it can, staying on the
/// solution it started on. Fixed geometry stays. A step that cannot be solved with the entity
/// held (the path crosses where the geometry cannot reach) is approached by halving the way to
/// it, and the next steps are tried from the last solved state, the last of them being the whole
/// remaining move. A point or line whose exact pin fails instead minimizes cursor distance in
/// the design's remaining freedom, retaining only corrected, feasible poses. For other entities,
/// when none past it solves, the first failed step stays placed but unsolved:
/// the edit's ordinary, non-strict solve that follows (seeded on the last solved state, see
/// [`Left::MovedFrom`]) then pulls it back (fully constrained geometry) or leaves the sketch
/// unsolved for the user to see. A step that would sweep an arc through its own start is undone
/// and the drag ends there. Returns how far it got and what it leaves for the edit's solve
/// ([`Left::Solved`]: the retained pose satisfies the design constraints, even without progress).
///
/// `with`: the rest of a selection dragged by one of its entities; all of it slides by the
/// cursor's move as one (see [`plan_group`]).
///
/// `live`: one frame of an interactive drag, whose held solves and cursor correction share
/// one frame's work budget ([`Budget::frame`]); otherwise each has a solve's own.
///
/// `solved`: the sketch as last evaluated. When it is unchanged and solved, a grabbed entity it
/// determined is not dragged.
pub(super) fn drag_entity(
    sk: &mut Sketch,
    e: Ent,
    with: &[Ent],
    from: Vec2,
    to: Vec2,
    live: bool,
    solved: Option<&solidcraft_doc::SolvedSketch>,
) -> (Dragged, Left) {
    // An unchanged, solved entity with no freedom cannot follow a drag. Holding it at a
    // different position only makes its constraints conflict, and a large pattern would spend
    // minutes trying dense solves before returning to the same geometry (#2006).
    if solved.is_some_and(|ss| {
        ss.report.ok()
            && ss.sketch == *sk
            && match e {
                Ent::Point(q) => ss.report.point_determined.get(q),
                Ent::Curve(c) => ss.report.curve_determined.get(c),
            }
            .copied()
            .unwrap_or(false)
    }) {
        return (Dragged::default(), Left::Solved);
    }
    let Some((path, hold, n, short)) = prepare(sk, e, with, from, to) else {
        return (Dragged::default(), Left::Unsolved);
    };
    // A point dragged alone that ends tangent lines: each step first tries with their other ends
    // held too, and only when that can't be solved lets them move. Unheld, the solver spread its
    // correction over every free point, so the line's far end moved (towards the dragged one)
    // where the curve it is tangent to could have taken the change, as holding that end by hand
    // showed.
    let keep = keep_line_ends(sk, e, with, &hold);
    #[cfg(test)]
    let (n, short) = (n.min(step_cap::get()), short.min(step_cap::get()));
    let moving: Vec<_> = path.points().filter_map(|q| Some((q, sk.point(q)?))).collect();
    let line_grab = if with.is_empty() { LineGrab::new(sk, e, from) } else { None };
    let start = line_grab.and_then(|g| g.point(sk)).or_else(|| if let Ent::Point(q) = e { sk.point(q) } else { None });
    let mut frame = live.then(frame_budget);
    let reach = FLUNG * (extent(sk) + path.length()).max(1.0);
    // Only success matters to these solves: the degrees of freedom of a sketch with the entity
    // held are not the sketch's. Each is seeded on the last solved state (`last`), so what the
    // constraints pin down keeps the orientation it had there. A failed or wrapping step is
    // undone (a failed solve leaves the step placed). Also returns whether the decomposition ran
    // out of budget.
    let mut solves = Solves::default();
    // Whether the sketch follows step `t` (with the tangent lines' far ends held, else free).
    let held = |sk: &mut Sketch, last: &Geometry, t: f64, budget: &mut Budget| {
        path.apply(sk, t);
        let kept = keep.as_ref().is_some_and(|keep| {
            let ok = counted_solve(sk, keep, Some(&last.pos), Analysis::StatusOnly, Some(&mut *budget)).ok() && !last.sweep_wrapped(sk);
            if !ok {
                last.restore(sk);
                path.apply(sk, t);
            }
            ok
        });
        kept || counted_solve(sk, &hold, Some(&last.pos), Analysis::StatusOnly, Some(&mut *budget)).ok()
    };
    let mut step = |sk: &mut Sketch, last: &Geometry, t: f64, solves: &mut Solves| {
        let mut own = Budget::full();
        let budget = frame.as_mut().unwrap_or(&mut own);
        let spent = budget.spent();
        let ok = held(sk, last, t, budget);
        solves.count += 1;
        solves.work += budget.spent() - spent;
        solves.cut |= budget.exhausted();
        solves.numeric |= budget.numeric_exhausted();
        let outcome = if !ok {
            Step::Failed
        } else if last.sweep_wrapped(sk) {
            Step::Wrapped
        } else if last.flung(sk, reach) {
            Step::Failed
        } else {
            return Step::Solved;
        };
        last.restore(sk);
        outcome
    };
    let mut last = Geometry::default();
    last.take(sk);
    // How far along the last solved state is; the first step since it that failed.
    let (mut good, mut failed) = (0.0, None);
    let mut spare = MAX_STEPS;
    // The steps: `k` of `dt` from `base`; once a seed ran out of budget, the rest of the way in
    // short steps.
    let (mut base, mut dt, mut k, mut t) = (0.0, 1.0 / n as f64, 0, 0.0);
    // The `k`-th step from `base`; the last ends exactly at the cursor.
    let at = |base: f64, dt: f64, k: usize| {
        let t = base + dt * k as f64;
        if 1.0 - t < 0.5 * dt { 1.0 } else { t }
    };
    let mut shortened = false;
    let mut wrapped = false;
    let mut out_of_frame = false;
    'walk: while t < 1.0 {
        // A live frame whose numerical solves have done their share takes no more steps: the
        // rest of the way waits for the next frame (`retry`), instead of seconds of held solves
        // in this one, all failing where the geometry can't follow.
        if solves.numeric {
            out_of_frame = true;
            break 'walk;
        }
        if solves.cut && !shortened {
            shortened = true;
            let rest = 1.0 - t;
            let m = steps(rest * short as f64, 1.0);
            (base, dt, k) = (t, rest / m as f64, 0);
        }
        // Past a failed step, every next one starts from the last solved state again and a failed
        // one leaves nothing for the one after. Unless the drag is live (its steps share the
        // frame's budget), the next few are solved at once, on all cores, and those that fail
        // in a row are taken as tried: the walk goes on where it would have, one by one, at the
        // first of them that doesn't fail, without the seconds of failing solves before it.
        #[cfg(test)]
        let ahead = last_drag::ahead();
        #[cfg(not(test))]
        let ahead = true;
        if ahead && !live && failed.is_some() && line_grab.is_none() {
            let ahead: Vec<f64> =
                (1..=AHEAD).map(|j| at(base, dt, k + j)).scan(false, |done, t| (!std::mem::replace(done, t >= 1.0)).then_some(t)).collect();
            let tried = solidcraft_kernel::map_on_threads(&ahead, AHEAD, |&t| {
                let mut sk = sk.clone();
                let mut budget = Budget::full();
                let ok = held(&mut sk, &last, t, &mut budget);
                (ok, budget.spent(), budget.exhausted())
            });
            let failing = tried.iter().take_while(|r| matches!(r, Some((false, _, false)))).flatten().collect::<Vec<_>>();
            if !failing.is_empty() {
                for (_, work, _) in &failing {
                    solves.count += 1;
                    solves.work += work;
                }
                k += failing.len();
                t = at(base, dt, k);
                #[cfg(test)]
                last_drag::add_skipped(failing.len());
                continue 'walk;
            }
        }
        k += 1;
        t = at(base, dt, k);
        match step(sk, &last, t, &mut solves) {
            Step::Solved => {
                (good, failed) = (t, None);
                if t < 1.0 {
                    last.take(sk);
                }
                continue;
            }
            Step::Wrapped => {
                wrapped = true;
                failed = None;
                break 'walk;
            }
            Step::Failed => {}
        }
        // Translating both ends can be impossible although a link can rotate. Do not spend
        // the frame walking more impossible pins: optimize the point actually grabbed.
        if line_grab.is_some() {
            failed = Some(t);
            break 'walk;
        }
        if failed.is_none() {
            // Halve the way to the failed step, so the state the next steps start from (and are
            // seeded on) is as close to it as the geometry follows. The shortest move is tried
            // first: geometry that cannot move at all (pinned) costs one solve, not every halving.
            let fine = dt / HALVINGS as f64;
            let mut miss = t;
            let mut first = true;
            while spare > 0 && miss - good > fine && !solves.numeric {
                spare -= 1;
                let mid = if std::mem::take(&mut first) { good + fine } else { 0.5 * (good + miss) };
                match step(sk, &last, mid, &mut solves) {
                    Step::Solved => {
                        good = mid;
                        last.take(sk);
                    }
                    Step::Wrapped => {
                        wrapped = true;
                        break 'walk;
                    }
                    Step::Failed => miss = mid,
                }
            }
            failed = Some(t);
        }
    }
    // First retain held continuation across unreachable stretches of an otherwise reachable
    // path (for example an arm dragged through the hole around its shoulder). If it cannot
    // arrive, optimize the full cursor target without relaxing the design constraints.
    if !wrapped
        && with.is_empty()
        && (matches!(e, Ent::Point(_)) || line_grab.is_some())
        && (failed.is_some() || line_grab.and_then(|g| g.point(sk)).is_some_and(|at| at.dist(to) > 1e-7))
    {
        let (points, radii) = sk.coupled(&hold.points, &hold.radii);
        let size = smallest_size(sk, &hold, Some(Group { points: &points, radii: &radii }));
        let max_step = (size * SHORT_STEP_FRACTION).min(path.length()).max(MIN_STEP);
        #[cfg(test)]
        let spent = frame.as_ref().map_or(0, Budget::spent);
        let valid = |before: &Sketch, after: &Sketch| {
            let mut geometry = Geometry::default();
            geometry.take(before);
            !geometry.sweep_wrapped(after) && !geometry.flung(after, reach) && solidcraft_sketch::fillets_kept(before, after)
        };
        let followed = match (e, line_grab) {
            (Ent::Point(q), _) => solidcraft_sketch::move_point_towards(sk, q, to, max_step, frame.as_mut(), valid),
            (_, Some(g)) => solidcraft_sketch::move_line_towards(sk, g.curve, g.local, to, max_step, frame.as_mut(), valid),
            _ => false,
        };
        #[cfg(test)]
        if let Some(budget) = &frame {
            solves.work += budget.spent() - spent;
            solves.cut |= budget.exhausted();
        }
        if followed {
            #[cfg(test)]
            last_drag::set(solves);
            let at = line_grab.and_then(|g| g.point(sk)).or_else(|| if let Ent::Point(q) = e { sk.point(q) } else { None });
            let reached = start.zip(at).map_or(0.0, |(start, at)| {
                let d = to - start;
                if at.dist(to) < 1e-6 || d.dot(d) == 0.0 { 1.0 } else { ((at - start).dot(d) / d.dot(d)).clamp(0.0, 1.0) }
            });
            let spent = out_of_frame || frame.as_ref().is_some_and(Budget::exhausted);
            let retry = spent && start.zip(at).is_some_and(|(a, b)| a.dist(b) > 1e-9);
            return (Dragged { reached, moving, line_grab, retry }, Left::Solved);
        }
    }
    #[cfg(test)]
    last_drag::set(solves);
    let left = match failed {
        // The failed step is left placed for the edit's solve, seeded on the last solved state.
        Some(t) => {
            path.apply(sk, t);
            Left::MovedFrom(last.pos)
        }
        None if good > 0.0 => Left::Solved,
        None => Left::Unsolved,
    };
    // The next frame goes on only from progress: a grab that can't move asks for no more.
    (Dragged { reached: good, moving, line_grab, retry: out_of_frame && good > 0.0 }, left)
}

/// The held solves of one drag: how many, their shared work (including cursor correction),
/// and whether it ran out of budget.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Solves {
    pub count: usize,
    pub work: usize,
    pub cut: bool,
    /// The frame's numerical solves have done their share ([`Budget::numeric_exhausted`]).
    pub numeric: bool,
}

/// One live frame's decomposition budget (smaller in tests that set one).
fn frame_budget() -> Budget {
    #[cfg(test)]
    if let Some(work) = last_drag::frame_work() {
        return Budget::new(work);
    }
    #[cfg(test)]
    if let Some(work) = last_drag::frame_numeric() {
        return Budget::frame().with_numeric(work);
    }
    Budget::frame()
}

/// What the last drag on this thread did, and a frame budget for tests to set.
#[cfg(test)]
pub(super) mod last_drag {
    use std::cell::Cell;

    use super::Solves;

    thread_local! {
        static LAST: Cell<Solves> = const { Cell::new(Solves { count: 0, work: 0, cut: false, numeric: false }) };
        static FRAME: Cell<Option<usize>> = const { Cell::new(None) };
        static NUMERIC: Cell<Option<usize>> = const { Cell::new(None) };
        static AHEAD: Cell<bool> = const { Cell::new(true) };
        static SKIPPED: Cell<usize> = const { Cell::new(0) };
    }

    pub fn set(s: Solves) {
        LAST.with(|c| c.set(s));
    }

    /// The held solves of the last drag.
    pub fn get() -> Solves {
        LAST.with(Cell::get)
    }

    pub fn frame_work() -> Option<usize> {
        FRAME.with(Cell::get)
    }

    /// Restores the frame budget when dropped, also when the test panics.
    struct Restore(Option<usize>);

    impl Drop for Restore {
        fn drop(&mut self) {
            FRAME.with(|c| c.set(self.0));
        }
    }

    /// Run `f` with live frames given `work` units of decomposition budget.
    pub fn with_frame_work<R>(work: usize, f: impl FnOnce() -> R) -> R {
        let _restore = Restore(FRAME.with(|c| c.replace(Some(work))));
        f()
    }

    pub fn frame_numeric() -> Option<usize> {
        NUMERIC.with(Cell::get)
    }

    /// Restores the numerical allowance when dropped, also when the test panics.
    struct RestoreNumeric(Option<usize>);

    impl Drop for RestoreNumeric {
        fn drop(&mut self) {
            NUMERIC.with(|c| c.set(self.0));
        }
    }

    /// Run `f` with live frames given `work` units for their numerical solves.
    pub fn with_frame_numeric<R>(work: usize, f: impl FnOnce() -> R) -> R {
        let _restore = RestoreNumeric(NUMERIC.with(|c| c.replace(Some(work))));
        f()
    }

    pub fn ahead() -> bool {
        AHEAD.with(Cell::get)
    }

    /// Failed steps taken as tried from the steps solved at once, since the last call.
    pub fn skipped() -> usize {
        SKIPPED.with(|c| c.replace(0))
    }

    pub fn add_skipped(n: usize) {
        SKIPPED.with(|c| c.set(c.get() + n));
    }

    /// Restores solving steps ahead when dropped, also when the test panics.
    struct RestoreAhead(bool);

    impl Drop for RestoreAhead {
        fn drop(&mut self) {
            AHEAD.with(|c| c.set(self.0));
        }
    }

    /// Run `f` with every step of a drag solved one by one, as before steps were solved ahead.
    pub fn one_by_one<R>(f: impl FnOnce() -> R) -> R {
        let _restore = RestoreAhead(AHEAD.with(|c| c.replace(false)));
        f()
    }
}

/// How one held solve of a drag went.
enum Step {
    Solved,
    Failed,
    /// Solved, but it swept an arc through its own start.
    Wrapped,
}

/// How far a drag got.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Dragged {
    /// The part of the way from where it was grabbed to the cursor the geometry followed (0..1).
    pub reached: f64,
    /// The points the drag moves (not the fixed ones it holds), where they were before it.
    pub moving: Vec<(usize, Vec2)>,
    pub line_grab: Option<LineGrab>,
    /// Cursor work stopped at its budget after making progress; another frame can continue.
    pub retry: bool,
}

impl Dragged {
    pub(super) fn at(&self, sk: &Sketch, from: Vec2) -> Vec2 {
        if let Some(at) = self.line_grab.and_then(|g| g.point(sk)) {
            return at;
        }
        let moves: Vec<_> = self.moving.iter().filter_map(|&(q, was)| Some(sk.point(q)? - was)).collect();
        from + moves.iter().fold(Vec2::ZERO, |a, m| a + *m) * (1.0 / moves.len().max(1) as f64)
    }
}

/// The grabbed position in a line's frame, retained through rotation and constraint correction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LineGrab {
    curve: usize,
    a: usize,
    b: usize,
    local: Vec2,
}

impl LineGrab {
    fn new(sk: &Sketch, e: Ent, from: Vec2) -> Option<Self> {
        let Ent::Curve(curve) = e else { return None };
        let CurveKind::Line { a, b } = sk.curves.get(curve)?.kind else { return None };
        let pa = sk.point(a)?;
        let d = sk.point(b)? - pa;
        let len2 = d.dot(d);
        if !(len2 > 1e-18) {
            return None;
        }
        Some(Self { curve, a, b, local: Vec2::new((from - pa).dot(d) / len2, d.cross(from - pa) / len2) })
    }

    fn point(self, sk: &Sketch) -> Option<Vec2> {
        let a = sk.point(self.a)?;
        let d = sk.point(self.b)? - a;
        Some(a + d * self.local.x + Vec2::new(-d.y, d.x) * self.local.y)
    }
}

/// The geometry a step changes (point positions; curves, for radii and the arc ends a solve
/// may swap), kept to undo a step. Its buffers are reused from step to step.
#[derive(Default)]
struct Geometry {
    pos: Vec<Vec2>,
    curves: Vec<(CurveKind, bool)>,
}

impl Geometry {
    fn take(&mut self, sk: &Sketch) {
        self.pos.clear();
        self.pos.extend(sk.points.iter().map(|p| p.pos));
        self.curves.clear();
        self.curves.extend(sk.curves.iter().map(|c| (c.kind.clone(), c.reversed)));
    }

    fn restore(&self, sk: &mut Sketch) {
        for (p, at) in sk.points.iter_mut().zip(&self.pos) {
            p.pos = *at;
        }
        for (c, (kind, reversed)) in sk.curves.iter_mut().zip(&self.curves) {
            c.kind = kind.clone();
            c.reversed = *reversed;
        }
    }

    /// Did the step throw some point further than `reach` (or to no number at all)?
    fn flung(&self, sk: &Sketch, reach: f64) -> bool {
        sk.points.iter().zip(&self.pos).any(|(p, was)| !(p.pos.dist(*was) <= reach))
    }

    /// Did the step drive some arc's end through its start: the sweep jumped through 0 / a full
    /// turn, or ran from clear of them onto one? An arc already that close may still move.
    /// Measured on the arc's ends as they were before the step (a solve may swap them).
    fn sweep_wrapped(&self, sk: &Sketch) -> bool {
        let was = |q: usize| self.pos.get(q).copied();
        let now = |q: usize| sk.point(q);
        let clear = |s: f64| (SWEEP_EPS..=TAU - SWEEP_EPS).contains(&s);
        self.curves.iter().any(|(kind, _)| {
            let CurveKind::Arc { c, a, b } = *kind else { return false };
            let (Some(s0), Some(s1)) = (sweep(was(c), was(a), was(b)), sweep(now(c), now(a), now(b))) else { return false };
            (s1 - s0).abs() > PI || (clear(s0) && !clear(s1))
        })
    }
}

/// The held solves dragging `e` from `from` to `to` takes (0 when it cannot move): the plan
/// `drag_entity` runs, for tests.
#[cfg(test)]
pub(super) fn drag_steps(sk: &Sketch, e: Ent, from: Vec2, to: Vec2) -> usize {
    prepare(sk, e, &[], from, to).map_or(0, |(_, _, n, _)| n)
}

/// A drag's path, what its solves hold and its number of steps (and of short steps, see
/// `SHORT_STEP_FRACTION`), or `None` if it cannot move.
/// `hold` plus the far ends of the tangent lines a lone dragged point ends (not locked ones, nor the
/// point itself), or `None` when there are none.
fn keep_line_ends(sk: &Sketch, e: Ent, with: &[Ent], hold: &Hold) -> Option<Hold> {
    let Ent::Point(q) = e else { return None };
    if !with.is_empty() {
        return None;
    }
    let locked = sk.locked_points();
    let mut keep = hold.clone();
    // Only lines tangent to a curve: there the far end is what the solver chose to move, where a
    // free curve could take the change. Other drags keep their one solve a frame.
    let tangent = |line: usize| sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Tangent { a, b } if a == line || b == line));
    for (i, c) in sk.curves.iter().enumerate() {
        if let CurveKind::Line { a, b } = c.kind {
            if !tangent(i) {
                continue;
            }
            let far = if a == q {
                b
            } else if b == q {
                a
            } else {
                continue;
            };
            if far != q && !locked.get(far).copied().unwrap_or(true) && !keep.points.contains(&far) {
                keep.points.push(far);
            }
        }
    }
    (keep.points.len() > hold.points.len()).then_some(keep)
}

fn prepare(sk: &Sketch, e: Ent, with: &[Ent], from: Vec2, to: Vec2) -> Option<(Path, Hold, usize, usize)> {
    if !(from.is_finite() && to.is_finite()) {
        return None;
    }
    let (path, hold) = if with.is_empty() { plan(sk, e, from, to)? } else { plan_group(sk, e, with, to - from)? };
    let n = step_count(sk, &hold, path.length(), STEP_FRACTION);
    let short = step_count(sk, &hold, path.length(), SHORT_STEP_FRACTION);
    Some((path, hold, n, short))
}

/// A cap on the steps `drag_entity` takes on this thread, for tests that measure what fewer
/// steps keep (the planned count, `drag_steps`, is not capped).
#[cfg(test)]
pub(super) mod step_cap {
    use std::cell::Cell;

    thread_local! {
        static CAP: Cell<usize> = const { Cell::new(super::MAX_STEPS) };
    }

    pub fn get() -> usize {
        CAP.with(Cell::get)
    }

    /// Restores the cap when dropped, also when the test panics.
    struct Restore(usize);

    impl Drop for Restore {
        fn drop(&mut self) {
            CAP.with(|c| c.set(self.0));
        }
    }

    /// Run `f` with drags split into at most `cap` steps (at least one).
    pub fn with<R>(cap: usize, f: impl FnOnce() -> R) -> R {
        let _restore = Restore(CAP.with(|c| c.replace(cap.max(1))));
        f()
    }
}

/// Steps of at most `step` covering `length`: at least one, at most `MAX_STEPS`.
fn steps(length: f64, step: f64) -> usize {
    let n = (length / step).ceil();
    if n.is_finite() && n >= 1.0 { (n as usize).min(MAX_STEPS) } else { 1 }
}

/// The dragged geometry's path and what the solves hold, or `None` if it cannot move.
fn plan(sk: &Sketch, e: Ent, from: Vec2, to: Vec2) -> Option<(Path, Hold)> {
    let mut hold = Hold::default();
    let path = match e {
        Ent::Point(q) => {
            if sk.point_locked(q) {
                return None;
            }
            hold.points.push(q);
            let at = sk.point(q)?;
            Path::Slide { pts: vec![(q, at)], d: to - at }
        }
        Ent::Curve(c) => {
            if sk.curve_locked(c) {
                return None;
            }
            let kind = sk.curves.get(c).map(|x| x.kind.clone())?;
            let locked = sk.locked_points();
            let free = |q: &usize| !locked.get(*q).copied().unwrap_or(true);
            // A cursor on the centre leaves the radius as it is.
            let radius_to = |o: Vec2, r: f64| Some(o.dist(to)).filter(|x| *x > 1e-9).unwrap_or(r);
            match kind {
                CurveKind::Circle { c: cc, r } => {
                    let to_r = sk.point(cc).map_or(r, |o| radius_to(o, r));
                    hold.points.push(cc);
                    hold.radii.push(c);
                    Path::Radius { c, from: r, to: to_r }
                }
                CurveKind::Arc { c: cc, a, b } => {
                    let o = sk.point(cc)?;
                    let r = sk.radius(c).unwrap_or(0.0);
                    hold.points.extend([cc, a, b]);
                    let ends = [a, b].into_iter().filter(free).filter_map(|q| Some((q, (sk.point(q)? - o).normalized()?))).collect();
                    Path::ArcRadius { o, ends, from: r, to: radius_to(o, r) }
                }
                CurveKind::Ellipse { c: cc, m, r } => {
                    // Grabbed on the outline: the minor radius follows (the major stays).
                    let (o, pm) = (sk.point(cc)?, sk.point(m)?);
                    let u = (pm - o).normalized().unwrap_or(Vec2::X);
                    let minor = u.cross(to - o).abs();
                    hold.points.extend([cc, m]);
                    hold.radii.push(c);
                    Path::Radius { c, from: r, to: if minor > 1e-9 { minor } else { r } }
                }
                CurveKind::Conic { a, b, apex, rho } => {
                    // Grabbed on the curve itself: its fullness (`rho`) follows the drag, the
                    // way an ellipse's outline changes its minor radius rather than sliding the
                    // ellipse. `a`, `b` and the apex (the tangent intersection, dragged
                    // separately as an ordinary point) stay where they are.
                    let (pa, px, pb) = (sk.point(a)?, sk.point(apex)?, sk.point(b)?);
                    hold.points.extend([a, b, apex]);
                    let to_rho = conic_rho_from_point(pa, px, pb, to);
                    Path::ConicRho { c, from: rho, to: to_rho }
                }
                k => {
                    // Lines and splines slide with the cursor.
                    let mut ids = k.point_ids();
                    ids.sort_unstable();
                    ids.dedup();
                    let pts = ids.iter().filter(|q| free(q)).filter_map(|&q| Some((q, sk.point(q)?))).collect();
                    hold.points.extend(ids);
                    Path::Slide { pts, d: to - from }
                }
            }
        }
    };
    Some((path, hold))
}

/// A selection dragged by one of its entities (`e`, the rest `with`): every free point of all of
/// them slides by `d` and the solves hold them all (circles and ellipses keep their radii), so
/// the selection moves as one instead of its free ends trailing behind the grabbed entity.
fn plan_group(sk: &Sketch, e: Ent, with: &[Ent], d: Vec2) -> Option<(Path, Hold)> {
    let mut hold = Hold::default();
    for e in std::iter::once(&e).chain(with) {
        match *e {
            Ent::Point(q) => hold.points.push(q),
            Ent::Curve(c) => {
                let Some(kind) = sk.curves.get(c).map(|x| &x.kind) else { continue };
                hold.points.extend(kind.point_ids());
                if matches!(kind, CurveKind::Circle { .. } | CurveKind::Ellipse { .. }) {
                    hold.radii.push(c);
                }
            }
        }
    }
    hold.points.sort_unstable();
    hold.points.dedup();
    hold.radii.sort_unstable();
    hold.radii.dedup();
    let locked = sk.locked_points();
    let pts: Vec<(usize, Vec2)> =
        hold.points.iter().filter(|q| !locked.get(**q).copied().unwrap_or(true)).filter_map(|&q| Some((q, sk.point(q)?))).collect();
    if pts.is_empty() {
        return None;
    }
    Some((Path::Slide { pts, d }, hold))
}

/// Steps for a move of `length`: enough that none is longer than `fraction` of the smallest
/// size of the geometry the solve moves with what it holds (at least `MIN_STEP`), at most
/// `MAX_STEPS`. A move short next to every size in the sketch (a normal mouse frame) is one
/// step, found without grouping the sketch.
fn step_count(sk: &Sketch, hold: &Hold, length: f64, fraction: f64) -> usize {
    let step = |size: f64| (size * fraction).max(MIN_STEP);
    if length <= step(smallest_size(sk, hold, None)) {
        return 1;
    }
    let (points, radii) = sk.coupled(&hold.points, &hold.radii);
    steps(length, step(smallest_size(sk, hold, Some(Group { points: &points, radii: &radii }))))
}

/// What a solve moves together: per point and per curve radius.
#[derive(Clone, Copy)]
struct Group<'a> {
    points: &'a [bool],
    radii: &'a [bool],
}

impl Group<'_> {
    /// Does an entity with these points and curves belong to the group?
    fn has(&self, points: &[usize], curves: &[usize]) -> bool {
        let any = |flags: &[bool], ids: &[usize]| ids.iter().any(|i| flags.get(*i).copied().unwrap_or(false));
        any(self.points, points) || any(self.radii, curves)
    }
}

/// The size of the sketch: the diagonal of its points' box and its circles' diameters.
fn extent(sk: &Sketch) -> f64 {
    let (lo, hi) = sk
        .points
        .iter()
        .map(|p| p.pos)
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| {
            (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))
        });
    let radius = sk
        .curves
        .iter()
        .filter_map(|c| match c.kind {
            CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } => Some(r.abs()),
            _ => None,
        })
        .filter(|r| r.is_finite())
        .fold(0.0, f64::max);
    let diagonal = if lo.x <= hi.x { lo.dist(hi) } else { 0.0 };
    diagonal + 2.0 * radius
}

/// The smallest size that can make a constrained shape take another solution (of the geometry
/// in `group`, or of the whole sketch), or infinity when there is none: every length the
/// constraints hold. That is driving length dimensions (offsets along an axis or from a line
/// too: the solver holds their size, either side), radii and the current lengths of lines held
/// equal to another (a link's length can come from an Equal as well as a dimension). A line that
/// only keeps a direction (a rectangle's side) holds no length: it may pass through zero and turn
/// over, so it does not shrink the steps. Nor does a distance between two held points (the
/// dragged line's own length): held geometry moves as one. A circle's radius counts when the
/// solve moves the radius itself (not merely its centre).
fn smallest_size(sk: &Sketch, hold: &Hold, group: Option<Group>) -> f64 {
    // Point lists are built only to test a size smaller than the smallest so far against the
    // group; without a group nothing is built.
    let in_group = |points: &dyn Fn() -> Vec<usize>, curves: &[usize]| group.is_none_or(|g| g.has(&points(), curves));
    // A radius is the circle's own variable, or (an arc's) the distance between its points.
    let radius_in_group = |c: usize| match sk.curves.get(c).map(|x| &x.kind) {
        Some(CurveKind::Circle { .. } | CurveKind::Ellipse { .. }) => in_group(&Vec::new, &[c]),
        Some(k) => in_group(&|| k.point_ids(), &[]),
        None => false,
    };
    let line_length = |l: usize| super::line_pts(sk, l).map(|(a, b)| a.dist(b));
    let points = |k: &ConstraintKind| sk.constraint_points(k);
    let mut min = f64::INFINITY;
    let mut take = |size: f64, counts: &dyn Fn() -> bool| {
        let size = size.abs();
        if size.is_finite() && size > 1e-9 && size < min && counts() {
            min = size;
        }
    };
    for k in sk.constraints.iter().filter(|k| !k.driven).map(|k| &k.kind) {
        match *k {
            ConstraintKind::Equal { a, b } => {
                if let Some(v) = line_length(a).into_iter().chain(line_length(b)).reduce(f64::min) {
                    take(v, &|| in_group(&|| points(k), &[]));
                }
            }
            ConstraintKind::Radius { c, value } => take(value, &|| radius_in_group(c)),
            ConstraintKind::Diameter { c, value } => take(value / 2.0, &|| radius_in_group(c)),
            _ => {
                if let Some(v) = driving_length(k) {
                    // Held points move as one: a length between them never changes.
                    take(v, &|| {
                        let pts = points(k);
                        !pts.iter().all(|p| hold.points.contains(p)) && in_group(&|| pts.clone(), &k.indices().1)
                    });
                }
            }
        }
    }
    for c in 0..sk.curves.len() {
        if let Some(r) = sk.radius(c) {
            take(r, &|| radius_in_group(c));
        }
    }
    min
}

/// The length a driving dimension sets (a linear diameter's radius), if it sets one.
fn driving_length(k: &ConstraintKind) -> Option<f64> {
    if k.is_angle() {
        return None;
    }
    let v = k.value()?;
    Some(if matches!(k, ConstraintKind::LinearDiameter { .. }) { v / 2.0 } else { v })
}

/// Counter-clockwise sweep (radians, `[0, 2π)`) from `a` to `b` about `c`.
fn sweep(c: Option<Vec2>, a: Option<Vec2>, b: Option<Vec2>) -> Option<f64> {
    let (o, pa, pb) = (c?, a?, b?);
    Some(((pb - o).angle() - (pa - o).angle()).rem_euclid(TAU))
}
