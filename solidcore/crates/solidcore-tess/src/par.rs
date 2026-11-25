//! Ordered parallel map over independent items (desktop only; the browser build runs serially).
//! Each item runs under [`guard`], so a panic in one item is that item's error and never reaches
//! the pool.

use crate::{Result, TessError};

/// The stack of a thread that plans faces: the kernel's evaluator and its projections recurse,
/// and the kernel asks for 8 MiB, a native main thread's stack, where Rust starts other threads
/// with 2 MiB.
const STACK: usize = 8 << 20;

thread_local! {
    /// How many [`guard`]s the current thread is inside.
    static GUARDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether the current thread runs inside the mesher's [`guard`]: a panic there is caught and
/// becomes that item's error, so a panic hook may treat it as handled.
#[must_use]
pub fn guarded() -> bool {
    GUARDED.with(std::cell::Cell::get) > 0
}

/// `f`, with a panic in it caught and returned as [`TessError::Internal`] naming `what`.
pub(crate) fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
        }
    }
    GUARDED.with(|g| g.set(g.get() + 1));
    let depth = Depth;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    drop(depth);
    match r {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            Err(TessError::Internal(format!("{what}: {msg}")))
        }
    }
}

/// The mesher's worker pool, its threads with [`STACK`]; `None` when it cannot start (the work
/// then runs on the calling thread).
#[cfg(not(target_arch = "wasm32"))]
fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .thread_name(|i| format!("solidcore-tess-{i}"))
            .stack_size(STACK)
            .build()
            .ok()
    })
    .as_ref()
}

/// `f` over `items`, results in item order: on a pool when `parallel`, else on the calling
/// thread. Called on a worker of a pool (the caller's own), that pool's threads, as many as it
/// has; otherwise the mesher's pool. Waking workers costs tens of microseconds, so callers ask
/// for it only when the items carry more work than that.
pub(crate) fn map<T: Sync, R: Send>(
    what: &'static str,
    items: &[T],
    parallel: bool,
    f: impl Fn(&T) -> Result<R> + Sync,
) -> Vec<Result<R>> {
    let one = |item: &T| guard(what, || f(item));
    #[cfg(target_arch = "wasm32")]
    let _ = parallel;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if parallel && items.len() > 1 {
            if rayon::current_thread_index().is_some() {
                if rayon::current_num_threads() > 1 {
                    return items.par_iter().map(one).collect();
                }
            } else if let Some(pool) = pool()
                && pool.current_num_threads() > 1
            {
                return pool.install(|| items.par_iter().map(one).collect());
            }
        }
    }
    items.iter().map(one).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_errors() {
        let items: Vec<u32> = (0..1000).collect();
        let out = map("test", &items, true, |x| {
            if *x == 500 {
                Err(TessError::Invalid("five hundred".into()))
            } else {
                Ok(x * 2)
            }
        });
        assert_eq!(out.len(), 1000);
        for (i, r) in out.iter().enumerate() {
            match r {
                Ok(v) => assert_eq!(*v, 2 * u32::try_from(i).unwrap()),
                Err(_) => assert_eq!(i, 500),
            }
        }
    }

    #[test]
    fn a_panicking_item_is_that_items_error() {
        let out = map("test", &[1u32, 2, 3], true, |x| {
            assert!(*x != 2, "two");
            Ok(*x)
        });
        assert!(matches!(&out[1], Err(TessError::Internal(m)) if m.contains("two")));
        assert_eq!(out[0].as_ref().ok(), Some(&1));
        assert!(!guarded());
    }
}
