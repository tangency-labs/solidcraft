//! Ordered parallel map over independent items, on the process-wide worker pool (desktop
//! only; the browser build runs serially). Each item runs under [`crate::guard`], so a panic in
//! one item is that item's error and never reaches the pool.

use crate::{Result, guard};

/// `f` over `items`, results in item order: on the pool when `parallel`, else on the calling
/// thread. Waking the pool's workers costs tens of microseconds, so callers ask for it only
/// when the items carry more work than that (decided from the work, not the item count).
pub(crate) fn map<T: Sync, R: Send>(what: &'static str, items: &[T], parallel: bool, f: impl Fn(&T) -> Result<R> + Sync) -> Vec<Result<R>> {
    // The workers run under the caller's validation mode (`crate::validation`).
    let scope = crate::validation::current();
    let one = |item: &T| crate::validation::within(scope.clone(), || guard(what, || f(item)));
    // The browser build has no worker pool: everything runs on the calling thread.
    #[cfg(target_arch = "wasm32")]
    let _ = parallel;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if parallel && items.len() > 1 && rayon::current_num_threads() > 1 {
            return items.par_iter().map(one).collect();
        }
    }
    items.iter().map(one).collect()
}

/// Run `f` on every item, spread over plain threads (one per core, at most `max_threads`),
/// each pulling the next item; on the calling thread in the browser build or when threads
/// cannot be started. Each call runs under [`crate::guard`] (a panic is that item's loss).
///
/// Plain threads rather than the worker pool: an item may wait on a lazily filled cell that
/// another item is filling, and a pool worker that picked up a second item while it waited
/// inside its own item's parallel work could wait on itself.
pub fn for_each_on_threads<T: Sync>(items: &[T], max_threads: usize, f: impl Fn(&T) + Sync) {
    let _ = map_on_threads(items, max_threads, f);
}

/// Independent kernel jobs, with one computation lane per native dispatcher. Each dispatcher
/// owns a single-thread Rayon pool: nested triangulation neither queues behind a common
/// one-thread pool nor multiplies the requested concurrency. No second body can be stolen by
/// a worker while it fills a shared lazy mesh cell. The browser runs serially.
pub fn map_with_local_pools<T: Sync, R: Send>(items: &[T], max_threads: usize, f: impl Fn(&T) -> Result<R> + Sync) -> Vec<Result<R>> {
    #[cfg(not(target_arch = "wasm32"))]
    let one = |item: &T| {
        use std::cell::OnceCell;
        thread_local! {
            static POOL: OnceCell<rayon::ThreadPool> = const { OnceCell::new() };
        }
        POOL.with(|cell| {
            if cell.get().is_none() {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .build()
                    .map_err(|e| crate::KernelError::Failed(format!("cannot start mesh worker: {e}")))?;
                let _ = cell.set(pool);
            }
            let pool = cell.get().ok_or_else(|| crate::KernelError::Failed("missing mesh worker".into()))?;
            let scope = crate::validation::current();
            pool.install(|| crate::validation::within(scope, || guard("mesh worker", || f(item))))
        })
    };
    #[cfg(target_arch = "wasm32")]
    let one = |item: &T| guard("mesh worker", || f(item));
    map_on_threads(items, max_threads, one)
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err(crate::KernelError::Internal("mesh worker failed".into()))))
        .collect()
}

/// `f` over `items` as [`for_each_on_threads`] runs it, results in item order (`None` where an
/// item panicked).
pub fn map_on_threads<T: Sync, R: Send>(items: &[T], max_threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    // The threads run under the caller's validation mode (`crate::validation`).
    let scope = crate::validation::current();
    let one = |item: &T| crate::validation::within(scope.clone(), || guard("parallel item", || Ok(f(item))).ok());
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let threads = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(max_threads).min(items.len());
        if threads > 1 {
            let next = AtomicUsize::new(0);
            let work = || {
                let mut done = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    done.push((i, one(item)));
                }
                done
            };
            let mut out: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
            std::thread::scope(|scope| {
                // A thread that cannot start leaves its share to the others.
                let helpers: Vec<_> =
                    (1..threads).filter_map(|_| std::thread::Builder::new().name("solidcraft-each".into()).spawn_scoped(scope, work).ok()).collect();
                let mut all = work();
                for h in helpers {
                    // `work` never panics (every item runs under the guard).
                    if let Ok(done) = h.join() {
                        all.extend(done);
                    }
                }
                for (i, r) in all {
                    if let Some(slot) = out.get_mut(i) {
                        *slot = r;
                    }
                }
            });
            return out;
        }
    }
    let _ = max_threads;
    items.iter().map(one).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KernelError;

    #[test]
    fn local_pools_keep_order_and_errors_and_bound_nested_parallelism() {
        let items: Vec<usize> = (0..32).collect();
        let result = map_with_local_pools(&items, 8, |i| {
            #[cfg(not(target_arch = "wasm32"))]
            assert_eq!(rayon::current_num_threads(), 1);
            if *i == 7 { Err(KernelError::Invalid("seven".into())) } else { Ok(*i * 2) }
        });
        for (i, r) in result.iter().enumerate() {
            assert_eq!(r.as_ref().ok().copied(), (i != 7).then_some(i * 2));
        }
    }

    #[test]
    fn each_item_once_on_threads() {
        let items: Vec<u32> = (0..1000).collect();
        let seen: Vec<std::sync::atomic::AtomicU32> = (0..1000).map(|_| std::sync::atomic::AtomicU32::new(0)).collect();
        for_each_on_threads(&items, 8, |x| {
            seen[*x as usize].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            assert!(*x != 500, "a panicking item is that item's loss");
        });
        assert!(seen.iter().all(|s| s.load(std::sync::atomic::Ordering::Relaxed) == 1));
    }

    #[test]
    fn map_on_threads_keeps_order() {
        let items: Vec<u32> = (0..1000).collect();
        let out = map_on_threads(&items, 8, |x| {
            assert!(*x != 7, "a panicking item is that item's loss");
            x * 3
        });
        assert_eq!(out.len(), 1000);
        for (i, r) in out.iter().enumerate() {
            assert_eq!(*r, (i != 7).then_some(3 * i as u32));
        }
    }

    #[test]
    fn keeps_order_and_errors() {
        let items: Vec<u32> = (0..1000).collect();
        let out = map("test", &items, true, |x| if *x == 500 { Err(KernelError::Invalid("five hundred".into())) } else { Ok(x * 2) });
        assert_eq!(out.len(), 1000);
        for (i, r) in out.iter().enumerate() {
            match r {
                Ok(v) => assert_eq!(*v, 2 * i as u32),
                Err(_) => assert_eq!(i, 500),
            }
        }
    }
}
