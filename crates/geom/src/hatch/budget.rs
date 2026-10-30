//! One cumulative budget for preparation, clipping and pattern expansion.

use super::Invalid;

/// Internal limits can tighten, but never enlarge, the production ceilings.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub input_pieces: usize,
    pub candidate_lines: usize,
    pub work: usize,
    pub events: usize,
    pub dash_steps: usize,
    pub marks: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { input_pieces: 8192, candidate_lines: 10000, work: 2000000, events: 65536, dash_steps: 200000, marks: 40000 }
    }
}

#[derive(Debug)]
pub(super) struct Budget {
    limits: Limits,
    used: Limits,
}

impl Default for Budget {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self { limits, used: Limits { input_pieces: 0, candidate_lines: 0, work: 0, events: 0, dash_steps: 0, marks: 0 } }
    }

    pub fn input_pieces(&mut self, n: usize) -> Result<(), Invalid> {
        charge(&mut self.used.input_pieces, n, self.limits.input_pieces.min(Limits::default().input_pieces), "input piece budget exhausted")
    }

    pub fn candidate_lines(&mut self, n: usize) -> Result<(), Invalid> {
        charge(
            &mut self.used.candidate_lines,
            n,
            self.limits.candidate_lines.min(Limits::default().candidate_lines),
            "candidate line budget exhausted",
        )
    }

    pub fn work(&mut self, n: usize) -> Result<(), Invalid> {
        charge(&mut self.used.work, n, self.limits.work.min(Limits::default().work), "geometry work budget exhausted")
    }

    pub fn events(&mut self, n: usize) -> Result<(), Invalid> {
        charge(&mut self.used.events, n, self.limits.events.min(Limits::default().events), "boundary event budget exhausted")
    }

    pub fn dash_steps(&mut self, n: usize) -> Result<(), Invalid> {
        charge(&mut self.used.dash_steps, n, self.limits.dash_steps.min(Limits::default().dash_steps), "dash step budget exhausted")
    }

    pub fn marks(&mut self, n: usize) -> Result<(), Invalid> {
        charge(&mut self.used.marks, n, self.limits.marks.min(Limits::default().marks), "output mark budget exhausted")
    }
}

fn charge(used: &mut usize, n: usize, limit: usize, message: &'static str) -> Result<(), Invalid> {
    let next = used.checked_add(n).ok_or(Invalid(message))?;
    if next > limit {
        return Err(Invalid(message));
    }
    *used = next;
    Ok(())
}
