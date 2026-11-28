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

/// A noted body's kernel forms together with a witness state known to hold it.
#[derive(Debug)]
struct NotedForm {
    forms: kernel::KernelForms,
    /// A noted state that holds this solid: while it stays among the model's held states, this
    /// solid is held without scanning every historical state's bodies. Holding a `Weak` prevents
    /// allocator address reuse while the witness is checked.
    witness: Option<Weak<ModelState>>,
}

/// The bodies of the states evaluated in a cache, to let go of their forms.
#[derive(Debug, Default)]
pub(super) struct Noted {
    /// The states looked at, by address. Held weakly, which keeps a state's address its own:
    /// a state seen again is not looked at again.
    states: HashMap<usize, Weak<ModelState>>,
    /// The forms of their bodies, by solid ([`kernel::Body::solid_id`]). They stay noted while
    /// the body lives anywhere, so one that is used again after its forms went is let go again.
    forms: HashMap<usize, NotedForm>,
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
    fn note_body(&mut self, body: &kernel::Body, witness: Option<&Weak<ModelState>>) {
        let mut cur = Some(body);
        let mut direct = witness;
        while let Some(b) = cur {
            match self.forms.entry(b.solid_id()) {
                Entry::Vacant(forms) => {
                    forms.insert(NotedForm { forms: b.kernel_forms(), witness: direct.cloned() }).forms.tell(&self.made_again);
                }
                // Another body has the address of one that is gone, or going.
                Entry::Occupied(mut forms) if !forms.get().forms.of(b) => {
                    forms.insert(NotedForm { forms: b.kernel_forms(), witness: direct.cloned() });
                    forms.get().forms.tell(&self.made_again);
                }
                Entry::Occupied(mut forms) => {
                    if let Some(w) = direct {
                        forms.get_mut().witness = Some(w.clone());
                    }
                }
            }
            direct = None;
            cur = b.rounds().map(|(earlier, _)| earlier);
        }
    }

    fn note<'a>(&mut self, states: impl Iterator<Item = &'a Arc<ModelState>>) {
        let mut ancestors: Vec<Arc<ModelState>> = Vec::new();
        for state in states {
            let Entry::Vacant(new) = self.states.entry(Arc::as_ptr(state) as usize) else { continue };
            let weak = Arc::downgrade(state);
            new.insert(weak.clone());
            self.fresh = true;
            for b in &state.bodies {
                self.note_body(&b.body, Some(&weak));
                if let Ok(cuts) = b.cuts.try_lock() {
                    for cut in cuts.iter() {
                        if let Some(left) = &cut.left {
                            self.note_body(&left.body, None);
                        }
                    }
                }
                if let Ok(sections) = b.sections.try_lock() {
                    for (_, section) in sections.iter() {
                        if let Some(sec) = section {
                            self.note_body(&sec.body, None);
                        }
                    }
                }
                if let Some(origin) = &b.names.origin
                    && !self.states.contains_key(&(Arc::as_ptr(&origin.before) as usize))
                {
                    ancestors.push(origin.before.clone());
                }
                if let Some(copy) = b.names.copy_source() {
                    self.note_body(&copy.body, None);
                    if let Some(origin) = &copy.names.origin
                        && !self.states.contains_key(&(Arc::as_ptr(&origin.before) as usize))
                    {
                        ancestors.push(origin.before.clone());
                    }
                }
            }
        }
        while let Some(state) = ancestors.pop() {
            let Entry::Vacant(new) = self.states.entry(Arc::as_ptr(&state) as usize) else { continue };
            new.insert(Arc::downgrade(&state));
            self.fresh = true;
            for b in &state.bodies {
                self.note_body(&b.body, None);
                if let Ok(cuts) = b.cuts.try_lock() {
                    for cut in cuts.iter() {
                        if let Some(left) = &cut.left {
                            self.note_body(&left.body, None);
                        }
                    }
                }
                if let Ok(sections) = b.sections.try_lock() {
                    for (_, section) in sections.iter() {
                        if let Some(sec) = section {
                            self.note_body(&sec.body, None);
                        }
                    }
                }
                if let Some(origin) = &b.names.origin
                    && !self.states.contains_key(&(Arc::as_ptr(&origin.before) as usize))
                {
                    ancestors.push(origin.before.clone());
                }
                if let Some(copy) = b.names.copy_source() {
                    self.note_body(&copy.body, None);
                    if let Some(origin) = &copy.names.origin
                        && !self.states.contains_key(&(Arc::as_ptr(&origin.before) as usize))
                    {
                        ancestors.push(origin.before.clone());
                    }
                }
            }
        }
        // Copies evaluate many times between two releases: what is gone since goes here, once
        // the states noted have doubled.
        if self.states.len() > self.swept_at.saturating_mul(2).max(4096) {
            self.states.retain(|_, state| state.strong_count() > 0);
            self.forms.retain(|_, entry| !entry.forms.gone());
            self.swept_at = self.states.len();
        }
    }
}

impl Model {
    /// Every state this model holds: after every feature, and of the features the history marker
    /// hides ([`Model::rolled_results`]).
    fn all_states(&self) -> impl Iterator<Item = &Arc<ModelState>> {
        self.results.iter().map(|r| &r.state).chain(self.rolled_results.values().map(|(_, _, r)| &r.state))
    }

    /// The addresses of the states whose bodies keep their kernel forms (the active shown state
    /// and any results kept behind the history marker), in one order whatever order they are held in.
    fn held_now(&self, shown: &Arc<ModelState>) -> Vec<usize> {
        let mut held: Vec<usize> =
            std::iter::once(shown).chain(self.rolled_results.values().map(|(_, _, r)| &r.state)).map(|s| Arc::as_ptr(s) as usize).collect();
        held.sort_unstable();
        held
    }

    /// Notes this model's states in the cache it shares with its copies, before an evaluation
    /// (the states it leaves, which no evaluation here may have made: a saved model opened) and
    /// after it.
    pub(super) fn note_states(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.noted.note(self.all_states());
        }
    }

    /// Lets go of the kernel forms ([`kernel::KernelForms::release`]) of the noted bodies this
    /// model no longer shows: its superseded intermediate states, earlier states, and states its
    /// copies evaluated, wherever the cache still keeps them. Only the active shown state and
    /// results kept behind the history marker retain kernel forms. For the model a session shows,
    /// never for a copy, whose session still holds the bodies the copy started from; a second
    /// session on the same cache would let go of the first one's model. A body another model still
    /// uses (a preview on screen, work ahead on another thread) makes its form again on its next
    /// use, which costs that use the time and changes nothing else, and is let go again by the
    /// next call.
    ///
    /// No form is let go and no body looked at unless something happened since the last call:
    /// the model holds other states, a state was noted, or a body that was asked to let its
    /// forms go made one again. Each noted solid keeps a witness state known to contain it, so
    /// when that state is still among the model's held states the solid is proved held in O(1);
    /// only solids whose witness left the model scan held states to re-verify or release them.
    pub fn release_left(&self) {
        let _span = solidcraft_profile::span("doc", "release");
        let shown = self.state();
        let shown_stale = self.results.iter().rev().find(|r| !r.skipped).is_some_and(|r| r.stale);
        let held_now = self.held_now(&shown);
        let held_addrs: HashSet<usize> = held_now.iter().copied().collect();
        let Ok(mut cache) = self.cache.lock() else { return };
        let noted = &mut cache.noted;
        let made_again = noted.made_again.load(Ordering::Acquire);
        let due = std::mem::take(&mut noted.fresh) | (noted.made_again_seen != made_again) | (noted.released_for != held_now);
        if !due {
            return;
        }
        noted.made_again_seen = made_again;
        noted.released_for = held_now;
        noted.forms.retain(|_, entry| !entry.forms.gone());
        let mut unresolved: HashMap<usize, kernel::KernelForms> = HashMap::new();
        for (&id, entry) in &mut noted.forms {
            let live = entry.witness.as_ref().is_some_and(|w| w.strong_count() > 0 && held_addrs.contains(&(Weak::as_ptr(w) as usize)));
            if !live {
                if entry.witness.as_ref().is_some_and(|w| w.strong_count() == 0) {
                    entry.witness = None;
                }
                unresolved.insert(id, entry.forms.clone());
            }
        }
        cache.release_passes += 1;
        // The forms are let go without the cache's lock: freeing them holds no evaluation up.
        drop(cache);
        if unresolved.is_empty() {
            return;
        }
        let mut found_witnesses: Vec<(usize, Weak<ModelState>)> = Vec::new();
        for (state, skip_witness) in std::iter::once((&shown, shown_stale)).chain(self.rolled_results.values().map(|(_, _, r)| (&r.state, false))) {
            #[cfg(test)]
            super::cost_tests::count(|c| c.release_states_scanned += 1);
            for b in &state.bodies {
                let id = b.body.solid_id();
                if unresolved.remove(&id).is_some() {
                    // Do not attach a `Weak` witness to a stale live-drag state, or `Arc::make_mut`
                    // on the next live frame would reallocate instead of swapping in place.
                    if !skip_witness {
                        found_witnesses.push((id, Arc::downgrade(state)));
                    }
                    if unresolved.is_empty() {
                        break;
                    }
                }
            }
            if unresolved.is_empty() {
                break;
            }
        }
        for forms in unresolved.into_values() {
            forms.release();
        }
        if !found_witnesses.is_empty()
            && let Ok(mut cache) = self.cache.lock()
        {
            for (id, witness) in found_witnesses {
                if witness.strong_count() > 0
                    && let Some(entry) = cache.noted.forms.get_mut(&id)
                    && entry.witness.as_ref().is_none_or(|old| old.strong_count() == 0 || !held_addrs.contains(&(Weak::as_ptr(old) as usize)))
                {
                    entry.witness = Some(witness);
                }
            }
        }
    }
}
