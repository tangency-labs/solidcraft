//! Host memory headroom probe for memory-bounded kernel and inspection workloads.
//!
//! Uses `sysinfo` on native platforms (accounting for Windows commit headroom when commit charge
//! exceeds physical RAM, and container cgroup limits on Linux) and returns `None` on wasm32 or
//! whenever the host does not report memory figures.

use std::cell::Cell;

thread_local! {
    /// Test-only override for [`available_memory_bytes`] on the current thread.
    static OVERRIDE: Cell<Option<Option<u64>>> = const { Cell::new(None) };
}

/// Estimated bytes of memory headroom currently available for new allocations on this host, or
/// `None` when the platform cannot report it (for example in the browser build).
///
/// On Windows, when system commit charge already exceeds physical RAM (`used_swap > 0`), the
/// remaining commit limit (`free_swap`) can be much smaller than free physical RAM and is what
/// causes `VirtualAlloc` / Rust allocations to fail; the minimum of both is returned.
pub fn available_memory_bytes() -> Option<u64> {
    if let Some(forced) = OVERRIDE.with(Cell::get) {
        return forced;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(mut system) = sysinfo::System::new() else {
            return None;
        };
        #[cfg(target_os = "windows")]
        {
            system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram().with_swap());
            if system.total_memory() == 0 {
                return None;
            }
            Some(windows_headroom(system.available_memory(), system.used_swap(), system.free_swap()))
        }
        #[cfg(not(target_os = "windows"))]
        {
            system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
            if system.total_memory() == 0 {
                return None;
            }
            #[cfg(target_os = "linux")]
            let cgroup = {
                let pid = sysinfo::Pid::from_u32(std::process::id());
                system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[pid]), false, sysinfo::ProcessRefreshKind::nothing());
                system.process(pid).and_then(sysinfo::Process::cgroup_limits).or_else(|| system.cgroup_limits())
            };
            #[cfg(not(target_os = "linux"))]
            let cgroup = system.cgroup_limits();
            let cgroup_free = cgroup.map(|limits| limits.free_memory.min(limits.total_memory));
            Some(system.available_memory().min(cgroup_free.unwrap_or(u64::MAX)))
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// On Windows `sysinfo` maps `swap_total = CommitLimit - PhysicalTotal` and
/// `swap_used = CommitTotal - PhysicalTotal`. Once commit charge exceeds physical RAM
/// (`used_swap > 0`), `free_swap` is the exact remaining commit headroom (`CommitLimit - CommitTotal`).
#[cfg(any(target_os = "windows", test))]
fn windows_headroom(available_ram: u64, used_swap: u64, free_swap: u64) -> u64 {
    if used_swap > 0 { available_ram.min(free_swap) } else { available_ram }
}

/// Runs `f` on the current thread with [`available_memory_bytes`] returning `bytes`.
#[doc(hidden)]
pub fn with_available_memory_for_test<R>(bytes: Option<u64>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Option<u64>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(OVERRIDE.with(|slot| slot.replace(Some(bytes))));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_headroom_respects_commit_limit_when_commit_exceeds_ram() {
        const GB: u64 = 1 << 30;
        const MB: u64 = 1 << 20;
        // Commit below physical RAM (used_swap == 0): available RAM governs even with no pagefile.
        assert_eq!(windows_headroom(42 * GB, 0, 0), 42 * GB);
        assert_eq!(windows_headroom(42 * GB, 0, 16 * GB), 42 * GB);
        // Commit exceeds physical RAM: remaining commit headroom caps available RAM.
        assert_eq!(windows_headroom(42 * GB, 15 * GB, 128 * MB), 128 * MB);
        assert_eq!(windows_headroom(2 * GB, GB, 8 * GB), 2 * GB);
    }

    #[test]
    fn test_override_restores_previous_value() {
        let before = with_available_memory_for_test(Some(1234), available_memory_bytes);
        assert_eq!(before, Some(1234));
        let none = with_available_memory_for_test(None, available_memory_bytes);
        assert_eq!(none, None);
    }
}
