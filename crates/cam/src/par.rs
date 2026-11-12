//! Work split over threads: natively one thread a share of the cores, in order; in the browser,
//! which has no threads, one after the other on the caller's thread. The results are the same
//! either way, in the order of the work.

use std::sync::{Arc, Mutex};

use crate::{CamError, Result};

/// How many threads to split `work` items over (each worth at least `least` of them): one per
/// core up to 16; one in the browser.
pub(crate) fn threads_for(work: usize, least: usize) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (work, least);
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[cfg(test)]
        if SERIAL.with(std::cell::Cell::get) {
            return 1;
        }
        /// Most threads one call uses.
        const MAX_THREADS: usize = 16;
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        cores.min(MAX_THREADS).min(work / least.max(1)).max(1)
    }
}

/// How many threads a [`Crew`] for one clearing's walks has (the caller's one of them): a
/// share of the cores, as the clearings of a job run side by side; one in the browser.
pub(crate) fn crew_threads() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[cfg(test)]
        if SERIAL.with(std::cell::Cell::get) {
            return 1;
        }
        /// Most threads one crew has.
        const MAX_CREW: usize = 8;
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        (cores / 4).clamp(1, MAX_CREW)
    }
}

#[cfg(test)]
thread_local! {
    static SERIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with every split of work on this thread taken as one part ([`threads_for`] says 1):
/// for a test that the work split over threads gives what it gives in one.
#[cfg(test)]
pub(crate) fn serially<R>(f: impl FnOnce() -> R) -> R {
    let was = SERIAL.with(|c| c.replace(true));
    let r = f();
    SERIAL.with(|c| c.set(was));
    r
}

/// `f` run for every item, each on a thread of its own when there is more than one (else here),
/// the results in order. A thread that cannot be had runs its item here; one that stops is an
/// error, never a panic.
pub(crate) fn map<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> Result<R> + Sync) -> Result<Vec<R>> {
    if items.len() <= 1 || cfg!(target_arch = "wasm32") {
        return items.into_iter().map(&f).collect();
    }
    let f = &f;
    // Each thread can be stopped as the caller's can.
    let stop = crate::stop::current();
    let lost = || CamError::Unsupported("a worker thread lost its work".into());
    std::thread::scope(|scope| {
        let mut pending = Vec::with_capacity(items.len());
        for (b, item) in items.into_iter().enumerate() {
            // The item goes through a slot the thread takes it from, so that it is still here
            // when no thread can be had.
            let slot = Arc::new(Mutex::new(Some(item)));
            let mine = Arc::clone(&slot);
            let stop = stop.clone();
            let job = move || match mine.lock().ok().and_then(|mut s| s.take()) {
                Some(item) => crate::stop::scoped(stop, || f(item)),
                None => Err(lost()),
            };
            match std::thread::Builder::new().name(format!("cam-work-{b}")).spawn_scoped(scope, job) {
                Ok(h) => pending.push(Err(h)),
                Err(_) => pending.push(Ok(match slot.lock().ok().and_then(|mut s| s.take()) {
                    Some(item) => f(item),
                    None => Err(lost()),
                })),
            }
        }
        pending
            .into_iter()
            .map(|p| match p {
                Ok(done) => done,
                Err(h) => h.join().unwrap_or_else(|_| Err(CamError::Unsupported("a worker thread stopped".into()))),
            })
            .collect()
    })
}

/// `f` run for every index `0..n` over at most `threads` threads, each taking the next index no
/// thread has taken yet (so items of unequal work spread over them), the results in index order.
/// `f`'s own errors are its results; only a lost thread is an error here.
pub(crate) fn map_pulled<R: Send>(n: usize, threads: usize, f: impl Fn(usize) -> R + Sync) -> Result<Vec<R>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let (f, next) = (&f, &next);
    let workers: Vec<usize> = (0..threads.clamp(1, n.max(1))).collect();
    let parts = map(workers, |_| {
        let mut mine = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            mine.push((i, f(i)));
        }
        Ok(mine)
    })?;
    let mut all: Vec<(usize, R)> = parts.into_iter().flatten().collect();
    all.sort_by_key(|(i, _)| *i);
    Ok(all.into_iter().map(|(_, r)| r).collect())
}

/// The ranges `0..len` split into `parts` runs of about equal `weight` (each item's), in order.
pub(crate) fn split(weights: &[usize], parts: usize) -> Vec<std::ops::Range<usize>> {
    let total: usize = weights.iter().sum();
    let parts = parts.max(1);
    let share = total.div_ceil(parts).max(1);
    let mut out = Vec::with_capacity(parts);
    let (mut start, mut acc) = (0, 0usize);
    for (i, w) in weights.iter().enumerate() {
        acc = acc.saturating_add(*w);
        if acc >= share && out.len() + 1 < parts {
            out.push(start..i + 1);
            (start, acc) = (i + 1, 0);
        }
    }
    if start < weights.len() || out.is_empty() {
        out.push(start..weights.len());
    }
    out
}

/// A task a [`Crew`] runs for every index of a job.
pub(crate) type Task<'env, R> = Arc<dyn Fn(usize) -> R + Send + Sync + 'env>;

/// One job a [`Crew`] shares out: `task` for every index `0..n`, each index taken once by
/// whichever crew thread asks next; the caller works out any its crew has not finished.
struct Job<'env, R> {
    task: Task<'env, R>,
    next: std::sync::atomic::AtomicUsize,
    /// Each index's result, once worked out.
    out: Vec<Mutex<Option<R>>>,
}

impl<R> Job<'_, R> {
    /// Takes indices no thread has taken yet until there are none.
    fn work(&self) {
        use std::sync::atomic::Ordering;
        loop {
            let i = self.next.fetch_add(1, Ordering::AcqRel);
            let Some(slot) = self.out.get(i) else { return };
            let r = (self.task)(i);
            if let Ok(mut s) = slot.lock() {
                *s = Some(r);
            }
        }
    }

    /// Index `i`'s result, taken.
    fn take(&self, i: usize) -> Option<R> {
        self.out.get(i).and_then(|s| s.lock().ok().and_then(|mut s| s.take()))
    }
}

/// What the crew's threads look at: the latest job posted (with its number), and whether the
/// crew is done.
struct Board<'env, R> {
    posted: std::sync::atomic::AtomicU64,
    slot: Mutex<(u64, Option<Arc<Job<'env, R>>>, bool)>,
    wake: std::sync::Condvar,
}

/// How many times a waiting thread of a [`Crew`] looks for a new job before it sleeps: jobs
/// follow each other within microseconds while a clearing walks, too soon to wake a sleeper.
const CREW_SPINS: u32 = 4096;

/// How many times the caller of [`Crew::run`] looks for an index a crew thread is working out
/// before it works it out itself.
const CALLER_SPINS: u32 = 512;

impl<R> Board<'_, R> {
    /// A crew thread: runs the jobs posted until the crew is done.
    fn serve(&self) {
        use std::sync::atomic::Ordering;
        let mut seen = 0u64;
        loop {
            let mut spins = 0;
            while self.posted.load(Ordering::Acquire) == seen && spins < CREW_SPINS {
                std::hint::spin_loop();
                spins += 1;
            }
            let job = {
                let Ok(mut slot) = self.slot.lock() else { return };
                loop {
                    if slot.2 {
                        return;
                    }
                    if slot.0 != seen {
                        seen = slot.0;
                        break slot.1.clone();
                    }
                    slot = match self.wake.wait(slot) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                }
            };
            if let Some(job) = job {
                job.work();
            }
        }
    }
}

/// Threads kept for many small jobs in a row (a clearing's walk asks for one every step, too
/// small to start threads for): the caller works on each job too, and the results are those of
/// the work done in order on one thread, in order. No threads in the browser or with fewer than
/// two cores: every job is run on the caller's thread.
pub(crate) struct Crew<'env, R> {
    board: Arc<Board<'env, R>>,
    helpers: usize,
}

/// Tells the crew's threads it is done, however [`with_crew`]'s work ends.
struct Dismiss<'b, 'env, R>(&'b Board<'env, R>);

impl<R> Drop for Dismiss<'_, '_, R> {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        if let Ok(mut slot) = self.0.slot.lock() {
            slot.2 = true;
            slot.1 = None;
        }
        self.0.posted.fetch_add(1, Ordering::AcqRel);
        self.0.wake.notify_all();
    }
}

/// `f` with a crew of `threads` threads (the caller's one of them) to run its jobs; they end
/// with it.
pub(crate) fn with_crew<'env, R: Send + 'env, T>(threads: usize, f: impl FnOnce(&Crew<'env, R>) -> T) -> T {
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { threads.max(1) };
    let board = Arc::new(Board { posted: std::sync::atomic::AtomicU64::new(0), slot: Mutex::new((0, None, false)), wake: std::sync::Condvar::new() });
    if threads <= 1 {
        return f(&Crew { board, helpers: 0 });
    }
    std::thread::scope(|scope| {
        let _dismiss = Dismiss(&board);
        let mut helpers = 0;
        for t in 1..threads {
            let b = Arc::clone(&board);
            if std::thread::Builder::new().name(format!("cam-crew-{t}")).spawn_scoped(scope, move || b.serve()).is_ok() {
                helpers += 1;
            }
        }
        f(&Crew { board: Arc::clone(&board), helpers })
    })
}

impl<'env, R: Send + 'env> Crew<'env, R> {
    /// Whether the crew has threads besides the caller's.
    pub(crate) fn shared(&self) -> bool {
        self.helpers > 0
    }

    /// How many threads work on each job, the caller's included.
    pub(crate) fn threads(&self) -> usize {
        self.helpers + 1
    }

    /// `task` run for every index `0..n`, by the crew and the caller; the results in index order.
    /// The caller never waits long for a crew thread: an index one has not finished soon (the
    /// thread was set aside on a busy machine) the caller works out itself, to the same result.
    pub(crate) fn run(&self, n: usize, task: Task<'env, R>) -> Vec<R> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        if self.helpers == 0 || n <= 1 {
            return (0..n).map(|i| task(i)).collect();
        }
        let job = Arc::new(Job { task, next: AtomicUsize::new(0), out: (0..n).map(|_| Mutex::new(None)).collect() });
        if let Ok(mut slot) = self.board.slot.lock() {
            slot.0 += 1;
            slot.1 = Some(Arc::clone(&job));
            self.board.posted.store(slot.0, Ordering::Release);
        }
        self.board.wake.notify_all();
        job.work();
        let all = (0..n)
            .map(|i| {
                for _ in 0..CALLER_SPINS {
                    if let Some(r) = job.take(i) {
                        return r;
                    }
                    std::hint::spin_loop();
                }
                job.take(i).unwrap_or_else(|| (job.task)(i))
            })
            .collect();
        if let Ok(mut slot) = self.board.slot.lock() {
            slot.1 = None;
        }
        all
    }
}
