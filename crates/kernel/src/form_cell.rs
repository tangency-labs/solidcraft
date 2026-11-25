//! A body's kernel form, kept between uses and let go on request (#19629).
//!
//! A body is its record. The kernel works on forms made from that record: the SMLib form its
//! operations take ([`crate::Body::smlib`]) and the solidcore-ops form its faces are measured on
//! (`Body::ops_body`). A form is several times its record once operations have measured and
//! checked it, so a body that is only kept to find its result again lets its forms go
//! ([`crate::Body::release_kernel_form`]); the record gives each again on its next use.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

/// One form of a body: made from the record on first use, kept for the uses to come and shared
/// by the body's clones, until [`FormCell::release`]. `R` is why the kernel refused the record,
/// kept for good: the same record is refused the same way again.
#[derive(Debug)]
pub(crate) struct FormCell<T, R> {
    made: Mutex<Option<Arc<T>>>,
    /// Held while a form is made by a caller that makes others wait ([`FormCell::making`]).
    making: Mutex<()>,
    refused: OnceLock<R>,
    /// Releases so far: a form whose making a release overtook goes to its caller and is not kept.
    releases: AtomicU64,
    /// The record gave a form once: a later failure is of the moment, never the record's.
    gave: AtomicBool,
    /// Counted up, each of them, when a form is kept after the cell was asked to let its form go
    /// ([`FormCell::tell`]): whoever let it go hears that the body is in use again. A body can be
    /// kept in more than one place that lets forms go (an import two designs share), so each
    /// count given stays for as long as its owner keeps it.
    made_again: Mutex<Vec<Weak<AtomicU64>>>,
}

impl<T, R> Default for FormCell<T, R> {
    fn default() -> Self {
        FormCell {
            made: Mutex::new(None),
            making: Mutex::new(()),
            refused: OnceLock::new(),
            releases: AtomicU64::new(0),
            gave: AtomicBool::new(false),
            made_again: Mutex::new(Vec::new()),
        }
    }
}

impl<T, R> FormCell<T, R> {
    fn slot(&self) -> MutexGuard<'_, Option<Arc<T>>> {
        self.made.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The counts to raise ([`FormCell::tell`]). Taken alone or inside the slot's lock, never
    /// around it.
    fn told(&self) -> MutexGuard<'_, Vec<Weak<AtomicU64>>> {
        self.made_again.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The form kept now.
    pub(crate) fn get(&self) -> Option<Arc<T>> {
        self.slot().clone()
    }

    /// Whether a form is kept now.
    pub(crate) fn holds(&self) -> bool {
        self.slot().is_some()
    }

    /// The turn to make the form, for a caller whose form others wait for rather than make too:
    /// look for the form again once it is taken.
    pub(crate) fn making(&self) -> MutexGuard<'_, ()> {
        self.making.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The count of releases, read before making a form and given to [`FormCell::keep`].
    pub(crate) fn releases(&self) -> u64 {
        self.releases.load(Ordering::Acquire)
    }

    /// Keeps `made`, a form whose making began at `releases`, and hands it out. The first form
    /// kept stays, as a cell filled once keeps its first; after a release since `releases` the
    /// form goes to its caller only, since the body let its forms go while it was made.
    pub(crate) fn keep(&self, made: T, releases: u64) -> Arc<T> {
        let made = Arc::new(made);
        let (kept, spare) = {
            let mut slot = self.slot();
            self.gave.store(true, Ordering::Release);
            if self.releases.load(Ordering::Acquire) != releases {
                (made, None)
            } else if let Some(first) = slot.as_ref() {
                (first.clone(), Some(made))
            } else {
                *slot = Some(made.clone());
                if releases > 0 {
                    for count in self.told().iter().filter_map(Weak::upgrade) {
                        count.fetch_add(1, Ordering::AcqRel);
                    }
                }
                (made, None)
            }
        };
        // A second maker's form is freed after the lock, as a released one is.
        drop(spare);
        kept
    }

    /// Has `count` go up whenever a form is kept here after a release (the cell need not have
    /// had a form then), for as long as the caller keeps `count`. Every count given goes up.
    pub(crate) fn tell(&self, count: &Arc<AtomicU64>) {
        let mut told = self.told();
        told.retain(|known| known.strong_count() > 0);
        if !told.iter().any(|known| std::ptr::eq(known.as_ptr(), Arc::as_ptr(count))) {
            told.push(Arc::downgrade(count));
        }
    }

    /// Why the record was refused, if it was.
    pub(crate) fn refusal(&self) -> Option<&R> {
        self.refused.get()
    }

    /// Keeps `why` as the record's refusal and returns the refusal kept (the first one). A record
    /// that gave a form before is not refused by a later failure: `None`, and nothing is kept.
    pub(crate) fn refuse(&self, why: R) -> Option<&R> {
        // Decided with the slot held, as [`FormCell::keep`] decides: a form kept meanwhile by
        // another maker of the same record is not followed by a refusal of that record.
        let _slot = self.slot();
        if self.gave.load(Ordering::Acquire) {
            return None;
        }
        Some(self.refused.get_or_init(|| why))
    }

    /// Lets the kept form go. A caller that holds it keeps it until it is done; one being made
    /// now goes to its caller and is not kept ([`FormCell::keep`]).
    pub(crate) fn release(&self) {
        self.releases.fetch_add(1, Ordering::AcqRel);
        // Taken under the lock, dropped after it: freeing a large form holds nobody up.
        let form = self.slot().take();
        drop(form);
    }

    /// The cell of a record the kernel refused for `why`.
    #[cfg(test)]
    pub(crate) fn refused_for(why: R) -> Self {
        FormCell { refused: OnceLock::from(why), ..FormCell::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_is_kept_until_released_and_its_holder_keeps_it() {
        let cell = FormCell::<u32, ()>::default();
        assert!(!cell.holds());
        let held = cell.keep(7, cell.releases());
        assert!(cell.holds());
        // A second maker finds the first form kept.
        assert_eq!(*cell.keep(8, cell.releases()), 7);
        cell.release();
        assert!(!cell.holds() && cell.get().is_none());
        assert_eq!(*held, 7, "a caller that holds the form keeps it");
        assert_eq!(*cell.keep(9, cell.releases()), 9, "made again after the release");
    }

    #[test]
    fn a_form_kept_after_a_release_is_counted_for_whoever_asked() {
        let cell = FormCell::<u32, ()>::default();
        let count = Arc::new(AtomicU64::new(0));
        cell.tell(&count);
        cell.keep(7, cell.releases());
        assert_eq!(count.load(Ordering::Acquire), 0, "a first form");
        cell.release();
        cell.keep(8, cell.releases());
        assert_eq!(count.load(Ordering::Acquire), 1);
        // A second maker that finds the form kept adds nothing.
        cell.keep(9, cell.releases());
        assert_eq!(count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_form_kept_after_a_release_is_counted_for_everyone_who_asked() {
        let cell = FormCell::<u32, ()>::default();
        let (first, second, gone) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        cell.tell(&first);
        cell.tell(&gone);
        drop(gone);
        // The same count given twice goes up once; one given later goes up as the first does.
        cell.tell(&first);
        cell.tell(&second);
        assert_eq!(cell.told().len(), 2, "a count nobody keeps is forgotten");
        cell.release();
        cell.keep(7, cell.releases());
        assert_eq!((first.load(Ordering::Acquire), second.load(Ordering::Acquire)), (1, 1));
        drop(second);
        cell.release();
        cell.keep(8, cell.releases());
        assert_eq!(first.load(Ordering::Acquire), 2, "a count dropped since is passed over");
    }

    #[test]
    fn a_form_whose_making_a_release_overtook_is_not_kept() {
        let cell = FormCell::<u32, ()>::default();
        let began = cell.releases();
        cell.release();
        assert_eq!(*cell.keep(7, began), 7, "its caller gets it");
        assert!(!cell.holds(), "the body let its forms go meanwhile");
    }

    #[test]
    fn a_release_while_another_thread_makes_the_form_does_not_wait_for_it() {
        let cell = std::sync::Arc::new(FormCell::<u32, ()>::default());
        let (started, start) = std::sync::mpsc::channel();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let maker = {
            let cell = cell.clone();
            std::thread::spawn(move || {
                let _turn = cell.making();
                let began = cell.releases();
                started.send(()).unwrap();
                wait.recv().unwrap();
                *cell.keep(7, began)
            })
        };
        start.recv().unwrap();
        cell.release();
        go.send(()).unwrap();
        assert_eq!(maker.join().unwrap(), 7);
        assert!(!cell.holds());
    }

    #[test]
    fn a_refusal_is_kept_unless_the_record_gave_a_form_before() {
        let cell = FormCell::<u32, &str>::default();
        assert_eq!(cell.refuse("no"), Some(&"no"));
        assert_eq!(cell.refuse("other"), Some(&"no"), "the first refusal stays");
        cell.release();
        assert_eq!(cell.refusal(), Some(&"no"), "a release keeps the refusal");
        let good = FormCell::<u32, &str>::default();
        good.keep(1, good.releases());
        good.release();
        assert_eq!(good.refuse("busy"), None, "a failure of the moment");
        assert!(good.refusal().is_none());
    }

    #[test]
    fn a_poisoned_cell_still_releases() {
        let cell = std::sync::Arc::new(FormCell::<u32, ()>::default());
        cell.keep(7, cell.releases());
        let poison = cell.clone();
        let _ = std::thread::spawn(move || {
            let _slot = poison.made.lock().unwrap();
            std::panic::resume_unwind(Box::new("poison"));
        })
        .join();
        assert!(cell.made.is_poisoned());
        cell.release();
        assert!(!cell.holds());
    }
}
