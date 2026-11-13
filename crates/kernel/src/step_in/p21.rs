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

/// [`solidcore_step::p21::parse`] under the current [`crate::watch`], up to [`max_bytes`], and
/// refused by name when reading it would take more than [`memory_to_spare`] (#15588).
pub fn parse(text: &str) -> Result<Exchange, String> {
    let watch = crate::watch::current().map(Watched);
    let watch = watch.as_ref().map(|w| w as &dyn solidcore_step::p21::Progress);
    solidcore_step::p21::parse_with_budget(text, watch, max_bytes(), memory_to_spare()).map_err(String::from)
}

/// The memory a STEP import may take: half of the machine's memory (of a container's limit when
/// that is lower), so a large file opens on a machine that can hold it and is refused, with the
/// memory it needs, on one that can't. Half of the total, not of what the system reports
/// available: that leaves out the caches and compressed memory the system hands back when asked
/// (on a 32 GB Mac it read about 10 GB, and half of it refused a 200 MB file that opens there
/// with 5.5 GB). With no reading of the machine's memory, and in the browser, there is no such
/// limit: the parser's counts still apply.
pub fn memory_to_spare() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(mut system) = sysinfo::System::new() else {
            return usize::MAX;
        };
        system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
        share_of(system.total_memory(), system.cgroup_limits().map(|c| c.total_memory))
    }
    #[cfg(target_arch = "wasm32")]
    {
        usize::MAX
    }
}

/// Half of `total`, or of a container's limit when that is lower; no limit when the machine
/// reports no memory at all.
fn share_of(total: u64, container: Option<u64>) -> usize {
    let memory = total.min(container.unwrap_or(u64::MAX));
    if memory == 0 {
        return usize::MAX;
    }
    usize::try_from(memory / 2).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_import_may_take_half_the_machine_s_memory() {
        const GB: u64 = 1 << 30;
        assert_eq!(share_of(32 * GB, None), 16 << 30);
        // A container's lower limit is what counts; a higher one changes nothing.
        assert_eq!(share_of(32 * GB, Some(4 * GB)), 2 << 30);
        assert_eq!(share_of(32 * GB, Some(64 * GB)), 16 << 30);
        // A machine that reports nothing sets no limit.
        assert_eq!(share_of(0, None), usize::MAX);
        assert!(memory_to_spare() > 0);
    }

    /// #15588: a file whose reading would take more than the machine can spare is refused with
    /// the memory it needs; the reporter's 350 MB files read on their 32 GB machine, and a file
    /// at the 512 MB size limit does too.
    #[test]
    fn a_step_file_too_large_for_the_memory_is_refused_by_name() {
        let text = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";
        let err = solidcore_step::p21::parse_with_budget(text, None, max_bytes(), 1).map(|_| ()).map_err(String::from).unwrap_err();
        assert!(err.starts_with("STEP file needs about"), "{err}");
        let machine = share_of(32 << 30, None);
        for mb in [350usize, 512] {
            assert!((mb << 20) * solidcore_step::p21::PEAK_PER_BYTE <= machine, "{mb} MB");
        }
    }
}
