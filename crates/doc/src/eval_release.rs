//! Bodies that leave the model let go of their kernel forms (#19629).
//!
//! The evaluation cache keeps the results of earlier evaluations so that a value set back finds
//! its result again, and counts each body by its record. A body's kernel forms (the SMLib form
//! its operations work on and the solidcore-ops form its faces are measured on) are several
//! times its record once operations have measured and checked it, and the cache does not count
//! them: every result kept held those forms too, so a session that tried many parameter values
//! held several times what the cache's budget allows. The record gives each form again on its
//! next use, so a body that is no longer in the model keeps only its record.
//!
//! A design's states are evaluated by its session's model and by that model's copies (previews,
//! trials, work done ahead on another thread), which share one cache. Every evaluation notes
//! the bodies of the states it starts from and ends with there ([`Model::note_states`]), by
//! their forms and not by their states: the cache keeps a body in what a feature changed
//! (`Change`) long after the state it was noted in is gone. The session's own model then lets
//! go of the forms of the noted bodies it does not hold ([`Model::release_left`]).

use super::{Model, ModelState, kernel};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// The bodies of the states evaluated in a cache, to let go of their forms.
#[derive(Debug, Default)]
pub(super) struct Noted {
    /// The states looked at, by address. Held weakly, which keeps a state's address its own:
    /// a state seen again is not looked at again.
    states: HashMap<usize, Weak<ModelState>>,
    /// The forms of their bodies, by solid ([`kernel::Body::solid_id`]). They stay noted while
    /// the body lives anywhere, so one that is used again after its forms went is let go again.
    forms: HashMap<usize, kernel::KernelForms>,
    /// A state was noted since forms were last let go.
    fresh: bool,
    /// Counted up by a noted body that keeps a form after it was let go
    /// ([`kernel::KernelForms::tell`]), and its value when forms were last let go: more since
    /// means a body off the model is in use again.
    made_again: Arc<AtomicU64>,
    made_again_seen: u64,
    /// The states the session's model held when forms were last let go. Kept here and not in
    /// the model: a model adopted from a copy, or put back from an earlier one, is compared with
    /// what the session held, not with what that model once held.
    released_for: Vec<usize>,
    /// The states noted after the last sweep of those that are gone.
    swept_at: usize,
}

impl Noted {
    fn note<'a>(&mut self, states: impl Iterator<Item = &'a Arc<ModelState>>) {
        for state in states {
            let Entry::Vacant(new) = self.states.entry(Arc::as_ptr(state) as usize) else { continue };
            new.insert(Arc::downgrade(state));
            self.fresh = true;
            for b in &state.bodies {
                match self.forms.entry(b.body.solid_id()) {
                    Entry::Vacant(forms) => {
                        forms.insert(b.body.kernel_forms()).tell(&self.made_again);
                    }
                    // Another body has the address of one that is gone, or going.
                    Entry::Occupied(mut forms) if !forms.get().of(&b.body) => {
                        forms.insert(b.body.kernel_forms());
                        forms.get().tell(&self.made_again);
                    }
                    Entry::Occupied(_) => {}
                }
            }
        }
        // Copies evaluate many times between two releases: what is gone since goes here, once
        // the states noted have doubled.
        if self.states.len() > self.swept_at.saturating_mul(2).max(4096) {
            self.states.retain(|_, state| state.strong_count() > 0);
            self.forms.retain(|_, forms| !forms.gone());
            self.swept_at = self.states.len();
        }
    }
}

impl Model {
    /// The states this model holds: after every feature, and of the features the history marker
    /// hides ([`Model::rolled_results`]).
    fn held_states(&self) -> impl Iterator<Item = &Arc<ModelState>> {
        self.results.iter().map(|r| &r.state).chain(self.rolled_results.values().map(|(_, _, r)| &r.state))
    }

    /// The addresses of the states this model holds, in one order whatever order they are held in.
    fn held_now(&self) -> Vec<usize> {
        let mut held: Vec<usize> = self.held_states().map(|s| Arc::as_ptr(s) as usize).collect();
        held.sort_unstable();
        held
    }

    /// Notes this model's states in the cache it shares with its copies, before an evaluation
    /// (the states it leaves, which no evaluation here may have made: a saved model opened) and
    /// after it.
    pub(super) fn note_states(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.noted.note(self.held_states());
        }
    }

    /// Lets go of the kernel forms ([`kernel::KernelForms::release`]) of the noted bodies this
    /// model does not hold: those of its own earlier states and of the states its copies
    /// evaluated, wherever the cache still keeps them. For the model a session shows, never for
    /// a copy, whose session still holds the bodies the copy started from; a second session on
    /// the same cache would let go of the first one's model. A body another model still uses (a
    /// preview on screen, work ahead on another thread) makes its form again on its next use,
    /// which costs that use the time and changes nothing else, and is let go again by the next
    /// call.
    ///
    /// No form is let go and no body looked at unless something happened since the last call:
    /// the model holds other states, a state was noted, or a body that was asked to let its
    /// forms go made one again. Then the bodies of each state the model holds are looked at,
    /// state by state (features times bodies), and every noted body once.
    pub fn release_left(&self) {
        let _span = solidcraft_profile::span("doc", "release");
        let held_now = self.held_now();
        let Ok(mut cache) = self.cache.lock() else { return };
        let noted = &mut cache.noted;
        let made_again = noted.made_again.load(Ordering::Acquire);
        let due = std::mem::take(&mut noted.fresh) | (noted.made_again_seen != made_again) | (noted.released_for != held_now);
        if !due {
            return;
        }
        noted.made_again_seen = made_again;
        noted.released_for = held_now;
        noted.forms.retain(|_, forms| !forms.gone());
        let forms: Vec<(usize, kernel::KernelForms)> = noted.forms.iter().map(|(id, forms)| (*id, forms.clone())).collect();
        cache.release_passes += 1;
        // The forms are let go without the cache's lock: freeing them holds no evaluation up.
        drop(cache);
        let shown = self.state();
        let held: HashSet<usize> =
            self.held_states().chain(std::iter::once(&shown)).flat_map(|s| s.bodies.iter().map(|b| b.body.solid_id())).collect();
        for (_, forms) in forms.iter().filter(|(id, _)| !held.contains(id)) {
            forms.release();
        }
    }
}
