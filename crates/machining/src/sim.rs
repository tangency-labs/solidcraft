//! Runs a program's moves through the tool sweeps into a stock. S1a: flat end mills on a 3-axis
//! tool; rapids cut as well as feeds. Anything else is reported as a diagnostic and skipped. A move
//! is cut in steps (a line in pieces, an arc a quarter turn at a time), so a caller can stop between
//! them and see the cut so far.

use std::ops::Range;

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::SimError;
use crate::dexel::Stock;
use crate::sweep::{ArcFixedAxis, FlatTool, LinearFixedAxis, MAX_ARC_TURNS};
use crate::toolpath::{Diag, MoveKind, NO_TOOL, Program, Shape};

const MAX_DIAGS: usize = 1000;
/// An arc is cut a quarter turn per step: a step is then a fraction of a second even at fine spacing,
/// so a caller that checks its budget between steps stops in time.
const STEP_SWEEP: f64 = std::f64::consts::FRAC_PI_2;
/// A straight move longer than this is cut in pieces no longer than it, so a run can stop part-way
/// along a long feed. A shorter move stays one step.
const LINE_STEP_MM: f64 = 2.0;
/// The most pieces a straight move is cut in. A longer line gets longer pieces, so a hostile length
/// costs a bounded number of steps.
const MAX_LINE_STEPS: usize = 1024;

fn finite(p: &[f64; 3]) -> bool {
    p.iter().all(|x| x.is_finite())
}

/// One piece of a move's sweep: a line, or one revolution of an arc.
enum Step {
    Line(LinearFixedAxis),
    Arc(ArcFixedAxis),
}

/// A move being cut: its steps, and how many are done. The steps go into the stock as they are cut,
/// so a part-cut move shows the cut so far. A move of more than one step keeps a journal in the stock
/// while it is part-cut, which `revert` uses to take the steps back out.
pub struct Cut {
    index: usize,
    source: u32,
    tool: u32,
    tag: u32,
    steps: Vec<Step>,
    done: usize,
}

impl Cut {
    /// The move's index in the program.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Steps in the move: one per arc quarter turn or line piece, or one for a short line.
    pub fn steps(&self) -> usize {
        self.steps.len()
    }

    /// Steps done so far.
    pub fn done(&self) -> usize {
        self.done
    }

    /// Takes the steps done so far back out of `stock`, which is then as it was before the move. The
    /// chunks they changed are marked dirty (see `Stock::undo_journal`).
    pub fn revert(&mut self, stock: &mut Stock) {
        if self.steps.len() > 1 {
            stock.undo_journal();
        }
        self.done = 0;
    }
}

/// What one [`Cutter::step`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The move has steps left.
    More,
    /// The move is cut into the stock.
    Cut,
    /// The move needs more segments on a ray than the stock keeps: it is reported and not cut.
    Skipped,
}

/// Cuts a program a few moves at a time, keeping what it reported so far: a run can be split (a
/// seek replays from the start, a playback goes move by move) and each cause is still reported once.
#[derive(Clone, Debug, Default)]
pub struct Cutter {
    reported: Vec<(u32, &'static str)>,
    diags: Vec<Diag>,
}

impl Cutter {
    pub fn new() -> Cutter {
        Cutter::default()
    }

    /// What could not be simulated so far, one line per cause.
    pub fn diags(&self) -> &[Diag] {
        &self.diags
    }

    pub fn into_diags(self) -> Vec<Diag> {
        self.diags
    }

    /// Records `diag` once per tool and cause.
    fn report(&mut self, tool: u32, key: &'static str, diag: Diag) {
        if !self.reported.contains(&(tool, key)) && self.diags.len() < MAX_DIAGS {
            self.reported.push((tool, key));
            self.diags.push(diag);
        }
    }

    /// Starts move `index` of `program`. `None` when there is nothing to cut: the first move (it has
    /// no start point), a jump, or a move that cannot be simulated (a diagnostic says why). Only a
    /// move index too large for a tag is an error.
    pub fn start(&mut self, program: &Program, index: usize) -> Result<Option<Cut>, SimError> {
        match plan(program, index) {
            Ok(cut) => Ok(Some(cut)),
            Err(Stop::Quiet) => Ok(None),
            Err(Stop::Report { tool, key, diag }) => {
                self.report(tool, key, diag);
                Ok(None)
            }
            Err(Stop::Fail(e)) => Err(e),
        }
    }

    /// Cuts the next step of `cut` into `stock`. A move of more than one step is journaled in the stock
    /// from its first step, so a failing move can be taken back whole. A step over the segment cap skips
    /// the whole move, reported. Any other failure is an error naming the move; the stock keeps the
    /// moves before it.
    pub fn step(&mut self, cut: &mut Cut, stock: &mut Stock) -> Result<Progress, SimError> {
        let Some(step) = cut.steps.get(cut.done) else { return Ok(Progress::Cut) };
        let multi = cut.steps.len() > 1;
        if multi && cut.done == 0 {
            stock.begin_journal();
        }
        match apply_step(step, cut.tag, stock) {
            Ok(()) => {
                cut.done += 1;
                if cut.done < cut.steps.len() {
                    return Ok(Progress::More);
                }
                if multi {
                    stock.commit_journal();
                }
                Ok(Progress::Cut)
            }
            Err(SimError::SegmentCap { max }) => {
                cut.revert(stock);
                let msg = format!("the move at line {} needs more than {max} segments on a ray; not cut", cut.source);
                self.report(cut.tool, "segments", Diag::error(cut.source, 0, msg));
                Ok(Progress::Skipped)
            }
            Err(e) => {
                cut.revert(stock);
                Err(SimError::AtMove { index: cut.index, line: cut.source, cause: e.to_string() })
            }
        }
    }

    /// Cuts the moves in `moves` of `program` into `stock`, move `i` tagged `i + 1`. Move `i` starts
    /// from the tip of move `i - 1`, so a range may start anywhere; a range past the last move stops
    /// at it. The first move and every `Jump` only set where the tool is.
    pub fn cut(&mut self, program: &Program, stock: &mut Stock, moves: Range<usize>) -> Result<(), SimError> {
        for i in moves {
            if i >= program.moves.len() {
                break;
            }
            let Some(mut cut) = self.start(program, i)? else { continue };
            loop {
                if self.step(&mut cut, stock)? != Progress::More {
                    break;
                }
            }
        }
        Ok(())
    }
}

/// Why a move is not cut: quietly (no start point, a jump), with a diagnostic for `Cutter::report`, or
/// with an error.
enum Stop {
    Quiet,
    Report { tool: u32, key: &'static str, diag: Diag },
    Fail(SimError),
}

/// The cut of move `index`, or why it is not cut. Pure: it records nothing, so `step_count` can ask what
/// a move takes without side effects, and `start` reports the refusal.
fn plan(program: &Program, index: usize) -> Result<Cut, Stop> {
    let Some(m) = program.moves.get(index) else { return Err(Stop::Quiet) };
    let Some(start) = index.checked_sub(1).and_then(|p| program.moves.get(p)).map(|p| p.tip) else { return Err(Stop::Quiet) };
    if m.kind == MoveKind::Jump {
        return Err(Stop::Quiet);
    }
    let tag = u32::try_from(index + 1).map_err(|_| Stop::Fail(SimError::TooManyMoves { max: u32::MAX as usize }))?;
    let refuse = |key: &'static str, diag: Diag| Stop::Report { tool: m.tool, key, diag };
    if m.tool == NO_TOOL {
        let diag = Diag::warning(m.source, 0, format!("no tool is selected before line {}; the move is not cut", m.source));
        return Err(refuse("no_tool", diag));
    }
    if !finite(&start) || !finite(&m.tip) {
        return Err(refuse("finite", Diag::error(m.source, 0, format!("non-finite position at line {} skipped", m.source))));
    }
    if let MoveKind::Arc { centre, normal, .. } = &m.kind
        && (!finite(centre) || !finite(normal))
    {
        return Err(refuse("finite", Diag::error(m.source, 0, format!("non-finite arc at line {} skipped", m.source))));
    }
    let Some(tool) = program.tools.iter().find(|t| t.id == m.tool) else {
        return Err(refuse("missing", Diag::error(m.source, 0, format!("tool {} is not defined", m.tool))));
    };
    if tool.shape != Shape::Flat {
        return Err(refuse("shape", Diag::error(m.source, 0, format!("tool {} shape is not simulated yet", m.tool))));
    }
    if m.axis != [0.0, 0.0, 1.0] {
        return Err(refuse("axis", Diag::error(m.source, 0, format!("tilted tool axis is not simulated yet (tool {})", m.tool))));
    }
    let Some(flat) = FlatTool::new(tool.diameter / 2.0, tool.flute_length, Vec3::Z) else {
        return Err(refuse("size", Diag::error(m.source, 0, format!("tool {} has a bad diameter or flute length", m.tool))));
    };
    let steps = match &m.kind {
        MoveKind::Rapid | MoveKind::Feed => line_steps(Vec3::from(start), Vec3::from(m.tip), flat),
        MoveKind::Arc { centre, normal, ccw, turns } => {
            if *turns > MAX_ARC_TURNS {
                let msg = format!("arc at line {} has more than {MAX_ARC_TURNS} turns; not simulated", m.source);
                return Err(refuse("turns", Diag::error(m.source, 0, msg)));
            }
            let arc = ArcFixedAxis::new(Vec3::from(start), Vec3::from(m.tip), Vec3::from(*centre), Vec3::from(*normal), *ccw, *turns, flat);
            match arc {
                Some(arc) => arc.pieces(STEP_SWEEP).into_iter().map(Step::Arc).collect(),
                None => return Err(refuse("arc", Diag::error(m.source, 0, format!("degenerate arc at line {} skipped", m.source)))),
            }
        }
        MoveKind::Jump => return Err(Stop::Quiet),
    };
    Ok(Cut { index, source: m.source, tool: m.tool, tag, steps, done: 0 })
}

/// The steps move `index` takes when cut: what `Cutter::start` would give, with no diagnostics recorded.
/// 0 when the move is not cut at all.
pub fn step_count(program: &Program, index: usize) -> usize {
    plan(program, index).map_or(0, |cut| cut.steps())
}

/// A straight move cut in pieces of at most `LINE_STEP_MM` (and at most `MAX_LINE_STEPS` of them), in
/// order. Their union is the move, so the stock is the same as for one piece.
fn line_steps(from: Vec3, to: Vec3, tool: FlatTool) -> Vec<Step> {
    let n = (((to - from).len() / LINE_STEP_MM).ceil() as usize).clamp(1, MAX_LINE_STEPS);
    let at = |k: usize| if k >= n { to } else { from + (to - from) * (k as f64 / n as f64) };
    (0..n).map(|k| Step::Line(LinearFixedAxis { from: at(k), to: at(k + 1), tool })).collect()
}

fn apply_step(step: &Step, tag: u32, stock: &mut Stock) -> Result<(), SimError> {
    match step {
        Step::Line(sweep) => stock.apply(sweep, tag),
        Step::Arc(sweep) => stock.apply(sweep, tag),
    }
}

/// Cuts every move into `stock` (see [`Cutter::cut`]). Returns what could not be simulated, one line
/// per cause.
pub fn simulate(program: &Program, stock: &mut Stock) -> Result<Vec<Diag>, SimError> {
    let mut cutter = Cutter::new();
    cutter.cut(program, stock, 0..program.moves.len())?;
    Ok(cutter.into_diags())
}
