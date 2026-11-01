//! Stage counters shared by the export worker and its bounded computation workers.
use std::sync::{Arc, Mutex};

pub(super) const CANCELLED: &str = "DXF export cancelled";

pub(crate) struct Stage {
    watch: Option<Arc<dyn solidcraft_kernel::watch::Watch>>,
    name: &'static str,
    total: usize,
    done: Mutex<usize>,
}

impl Stage {
    pub fn new(name: &'static str, total: usize) -> Self {
        let stage = Self { watch: solidcraft_kernel::watch::current(), name, total: total.max(1), done: Mutex::new(0) };
        if let Some(watch) = &stage.watch {
            watch.progress(name, 0, stage.total);
        }
        stage
    }
    pub fn check(&self) -> Result<(), &'static str> {
        if self.watch.as_ref().is_some_and(|w| w.cancelled()) { Err(CANCELLED) } else { Ok(()) }
    }
    pub fn advance(&self, count: usize) -> Result<(), &'static str> {
        self.check()?;
        if let Some(watch) = &self.watch {
            // Serialize reports as well as increments: eight workers must not report a
            // smaller completed count after a larger one.
            let mut done = self.done.lock().map_err(|_| "export progress unavailable")?;
            *done = done.saturating_add(count).min(self.total);
            watch.progress(self.name, *done, self.total);
        }
        self.check()
    }
}
