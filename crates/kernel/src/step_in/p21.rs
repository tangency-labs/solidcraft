//! The Part 21 parser is `solidcore-record`'s (re-exported by `solidcore-step`); this reads with the kernel's progress watch.

pub use solidcore_step::p21::{Entity, Exchange, Param};

use std::sync::atomic::{AtomicUsize, Ordering};

struct Watched(std::sync::Arc<dyn crate::watch::Watch>);

impl solidcore_step::p21::Progress for Watched {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn progress(&self, stage: &'static str, done: usize, total: usize) {
        self.0.progress(stage, done, total);
    }
}

/// The largest STEP text [`parse`] reads, in bytes; `solidcraft_io::limits` keeps it at the
/// STEP limit in force (Preferences › General › Largest file to open, or `--max-file-size`), so
/// the parser reads every file the size check let through (#13329).
static MAX_BYTES: AtomicUsize = AtomicUsize::new(solidcore_step::p21::MAX_BYTES);

/// Read STEP texts of up to `bytes` from now on (never less than the parser's default).
pub fn set_max_bytes(bytes: usize) {
    MAX_BYTES.store(bytes.max(solidcore_step::p21::MAX_BYTES), Ordering::Relaxed);
}

/// The largest STEP text [`parse`] reads now.
pub fn max_bytes() -> usize {
    MAX_BYTES.load(Ordering::Relaxed)
}

/// [`solidcore_step::p21::parse`] under the current [`crate::watch`], up to [`max_bytes`].
pub fn parse(text: &str) -> Result<Exchange, String> {
    let watch = crate::watch::current().map(Watched);
    let watch = watch.as_ref().map(|w| w as &dyn solidcore_step::p21::Progress);
    solidcore_step::p21::parse_with_max_bytes(text, watch, max_bytes()).map_err(String::from)
}
