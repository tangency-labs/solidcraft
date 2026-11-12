//! Threads, where there are threads.
//!
//! The flow solver's two heavy loops touch one cell each and nothing else, so they divide over
//! cores with nothing to coordinate. The browser build has no threads at all, and runs the same
//! loops on one; a solver that needed threads to be correct would be a solver with a bug.

/// Call `f(index, chunk)` for every `chunk`-sized run of `data`, in parallel where that is
/// possible.
///
/// `index` is the run's number, which for the flow solver is the cell. The last run is short if
/// the length is not a multiple of the size, so `f` reads the run's own length rather than
/// assuming it.
pub fn for_each_chunk_mut<T, F>(data: &mut [T], chunk: usize, f: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Send + Sync,
{
    if chunk == 0 {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        // Below a few thousand items the threads cost more than they save, and the verification
        // cases are all small.
        if data.len() / chunk >= 4096 {
            data.par_chunks_mut(chunk).enumerate().for_each(|(i, c)| f(i, c));
            return;
        }
    }
    for (i, c) in data.chunks_mut(chunk).enumerate() {
        f(i, c);
    }
}
