//! The undo history (#6996): the steps to undo and to redo, and every rule for changing them, in
//! one place. Its lists are private, so only these calls write a history:
//!
//! - [`History::push`] records a step: at most [`MAX_UNDO`] are kept, the oldest dropping first,
//!   and a new step clears the redo list;
//! - [`History::take_undo`] and [`History::undone`] move a step to the redo list (`edit.undo`), and
//!   [`History::take_redo`] and [`History::redone`] move it back (`edit.redo`);
//! - a gesture (a drag, a typed edit) takes a [`Mark`] before it starts and folds the steps it
//!   pushed into one with [`History::fold_since`], or drops them with [`History::drop_since`];
//! - [`History::append`] takes the steps a computed copy of the session recorded, and
//!   [`History::clear`] starts afresh.
//!
//! A run that should leave no step takes the whole history and puts it back
//! (`std::mem::take(&mut session.history)`), so what is put back is always a whole history.
//!
//! Each step has a number, counted from the history's start, so a mark finds the steps pushed
//! after it whatever the list's length: a full history keeps its length at the cap while it drops
//! old steps, and a fold by length would then fold nothing. The few old steps pushed out last are
//! kept with the number of the step that pushed each out, so a fold or a drop gives back what the
//! steps it removes pushed out: a gesture on a full history costs one old step, however many
//! frames it took.

use crate::Snapshot;

/// The most undo steps kept.
pub const MAX_UNDO: usize = 200;
/// The most pushed-out steps kept to give back: a gesture folds every frame, a step at a time.
const MAX_PUSHED_OUT: usize = 16;

/// The undo and redo steps, oldest first, each with its number.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    undo_numbers: Vec<u64>,
    redo: Vec<Snapshot>,
    redo_numbers: Vec<u64>,
    /// The next step's number.
    next: u64,
    /// The number of the step [`Self::take_undo`] or [`Self::take_redo`] took, for the state it
    /// leaves ([`Self::undone`], [`Self::redone`]).
    moving: Option<u64>,
    /// The last steps the cap pushed out, oldest first: each with its number and the number of
    /// the step that pushed it out.
    pushed_out: Vec<PushedOut>,
}

#[derive(Clone, Debug)]
struct PushedOut {
    step: Snapshot,
    number: u64,
    by: u64,
}

/// Where a gesture began: the steps pushed after it are the gesture's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark(u64);

impl History {
    /// Records `step`: the oldest step goes past [`MAX_UNDO`], and the redo list is cleared.
    pub fn push(&mut self, step: Snapshot) {
        let number = self.number();
        self.undo.push(step);
        self.undo_numbers.push(number);
        self.cap(number);
        self.redo.clear();
        self.redo_numbers.clear();
    }

    /// Keeps [`MAX_UNDO`] steps, remembering the last ones pushed out `by` a step.
    fn cap(&mut self, by: u64) {
        let over = self.undo.len().saturating_sub(MAX_UNDO);
        let out = self.undo.drain(..over).zip(self.undo_numbers.drain(..over)).map(|(step, number)| PushedOut { step, number, by });
        self.pushed_out.extend(out);
        let forget = self.pushed_out.len().saturating_sub(MAX_PUSHED_OUT);
        self.pushed_out.drain(..forget);
    }

    /// The step to go back to (`edit.undo`); [`Self::undone`] then keeps the state it left.
    pub fn take_undo(&mut self) -> Option<Snapshot> {
        let step = self.undo.pop()?;
        self.moving = self.undo_numbers.pop();
        Some(step)
    }

    /// The state an undo left, for redo.
    pub fn undone(&mut self, was: Snapshot) {
        let number = self.moving.take().unwrap_or_else(|| self.number());
        self.redo.push(was);
        self.redo_numbers.push(number);
    }

    /// The step to go forward to (`edit.redo`); [`Self::redone`] then keeps the state it left.
    pub fn take_redo(&mut self) -> Option<Snapshot> {
        let step = self.redo.pop()?;
        self.moving = self.redo_numbers.pop();
        Some(step)
    }

    /// The state a redo left, to undo again.
    pub fn redone(&mut self, was: Snapshot) {
        let number = self.moving.take().unwrap_or_else(|| self.number());
        self.undo.push(was);
        self.undo_numbers.push(number);
        self.cap(number);
    }

    /// A new step's number.
    fn number(&mut self) -> u64 {
        self.next += 1;
        self.next - 1
    }

    /// Where a gesture begins.
    pub fn mark(&self) -> Mark {
        Mark(self.next)
    }

    /// The first step pushed since `mark`, as an index into the undo list.
    fn since(&self, mark: Mark) -> usize {
        self.undo_numbers.partition_point(|&n| n < mark.0)
    }

    /// How many of the steps pushed since `mark` are still there to undo.
    pub fn steps_since(&self, mark: Mark) -> usize {
        self.undo.len() - self.since(mark)
    }

    /// Folds the steps pushed since `mark` into one: the first is kept (it holds the state before
    /// the gesture), the later ones are dropped, giving back what they pushed out. Returns how
    /// many there were.
    pub fn fold_since(&mut self, mark: Mark) -> usize {
        let steps = self.steps_since(mark);
        self.remove_from(self.since(mark) + 1);
        steps
    }

    /// Names the step the next undo goes back to: what a folded gesture did.
    pub fn name_last(&mut self, label: &str) {
        if let Some(step) = self.undo.last_mut() {
            step.label = label.to_string();
        }
    }

    /// Drops every step pushed since `mark`, giving back what they pushed out.
    pub fn drop_since(&mut self, mark: Mark) {
        self.remove_from(self.since(mark));
    }

    /// Removes the undo steps from index `keep` on, and gives back, oldest first, the old steps
    /// they pushed out.
    fn remove_from(&mut self, keep: usize) {
        let Some(&first) = self.undo_numbers.get(keep) else { return };
        self.undo.truncate(keep);
        self.undo_numbers.truncate(keep);
        let back = self.pushed_out.partition_point(|out| out.by < first);
        let given: Vec<PushedOut> = self.pushed_out.drain(back..).collect();
        self.undo_numbers.splice(..0, given.iter().map(|out| out.number));
        self.undo.splice(..0, given.into_iter().map(|out| out.step));
        // A step that pushed one out may have been undone since: never past the cap.
        self.cap(first);
    }

    /// Takes the steps a computed copy of the session recorded ([`crate::Session::accept_computed`]):
    /// its undo steps after this history's, capped, and its redo list in place of this one's.
    pub fn append(&mut self, computed: History) {
        for step in computed.undo {
            let number = self.number();
            self.undo.push(step);
            self.undo_numbers.push(number);
            self.cap(number);
        }
        // Numbered after every undo step, the next to redo lowest, as an undo leaves them.
        let count = computed.redo.len() as u64;
        self.redo_numbers = (0..count).rev().map(|i| self.next + i).collect();
        self.next += count;
        self.redo = computed.redo;
    }

    /// A fresh history (a new or opened design).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.undo_numbers.clear();
        self.redo.clear();
        self.redo_numbers.clear();
        self.pushed_out.clear();
    }

    /// Changes every step, to undo and to redo, in place (Save As moves their links; a cancelled
    /// feature editor puts its original design back in them).
    pub fn for_each_step(&mut self, mut change: impl FnMut(&mut Snapshot)) {
        let pushed_out = self.pushed_out.iter_mut().map(|out| &mut out.step);
        self.undo.iter_mut().chain(self.redo.iter_mut()).chain(pushed_out).for_each(&mut change);
    }

    /// The steps to undo, oldest first.
    pub fn steps(&self) -> &[Snapshot] {
        &self.undo
    }

    /// The steps to redo, the next one last.
    pub fn redo_steps(&self) -> &[Snapshot] {
        &self.redo
    }

    /// How many steps there are to undo.
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    /// Nothing to undo.
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// The step the next undo goes back to.
    pub fn last(&self) -> Option<&Snapshot> {
        self.undo.last()
    }

    /// The oldest step to undo, to change in place (a computed copy's first step, as it is accepted).
    pub fn first_mut(&mut self) -> Option<&mut Snapshot> {
        self.undo.first_mut()
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
