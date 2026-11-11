//! Shared native RAM admission. Active allocation promises keep one snapshot;
//! completed immutable backing can remain owned when the next epoch observes RAM.
use serde::Serialize;

/// Machine-local native admission policy. Every mode retains the same shared
/// reservation accounting and each host's finite software/allocation limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeMemoryPolicy {
    #[default]
    AvailableRam,
    /// Restore the previous software-pool admission without a physical-RAM gate.
    SoftwarePoolOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostMemorySource {
    SystemAvailable,
    SystemAndCgroup,
    ConservativeFallback,
}

/// Point-in-time admission against the process-wide native render allowance.
/// Required bytes include this request's retained metadata and capture credit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct HostMemoryAdmission {
    /// This request's total debit, including any already-held capture credit.
    pub required_bytes: u64,
    /// Already-held credit included in this request, never a new allocation.
    pub credited_bytes: u64,
    /// New allocation promise: required_bytes minus credited_bytes.
    pub additional_bytes: u64,
    /// Other held leases, including all epochs and software-only admissions.
    pub admitted_bytes: u64,
    /// Current-epoch debit, or all held bytes under the absolute fallback limit.
    pub physical_debit_bytes: u64,
    /// Snapshot capacity remaining for additional_bytes, saturating at zero.
    pub available_bytes: u64,
    /// Quiescent available-RAM/cgroup observation, or the absolute fallback allowance.
    pub capacity_bytes: u64,
    /// Shared process-wide software ceiling, enforced under either policy.
    pub software_limit_bytes: u64,
    pub source: HostMemorySource,
    /// Whether the available-RAM observation also gates this admission.
    pub policy: NativeMemoryPolicy,
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{HostMemoryAdmission, HostMemorySource, NativeMemoryPolicy};
    use crate::job::{ErrorCode, JobError, NATIVE_CPU_POOL_BYTES, Result};
    use std::sync::{Arc, Mutex, OnceLock};

    const FALLBACK_BYTES: u64 = 512 * 1024 * 1024;

    #[derive(Clone, Copy)]
    pub(crate) struct MemorySnapshot {
        pub(crate) capacity_bytes: u64,
        pub(crate) source: HostMemorySource,
    }
    impl MemorySnapshot {
        pub(crate) fn from_probe(total: u64, available: u64, cgroup: Option<u64>) -> Self {
            if total == 0 {
                return Self { capacity_bytes: FALLBACK_BYTES.min(cgroup.unwrap_or(u64::MAX)), source: HostMemorySource::ConservativeFallback };
            }
            let capacity_bytes = available.min(total).min(cgroup.unwrap_or(u64::MAX));
            Self { capacity_bytes, source: if cgroup.is_some() { HostMemorySource::SystemAndCgroup } else { HostMemorySource::SystemAvailable } }
        }
    }

    fn system_snapshot() -> MemorySnapshot {
        let Ok(mut system) = sysinfo::System::new() else {
            return MemorySnapshot::from_probe(0, 0, None);
        };
        system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
        // Linux service/container ceilings can be below the root cgroup's limit.
        // Refresh only our process header, without task, CPU, disk or environment data.
        #[cfg(target_os = "linux")]
        let cgroup = {
            let pid = sysinfo::Pid::from_u32(std::process::id());
            system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[pid]), false, sysinfo::ProcessRefreshKind::nothing());
            system.process(pid).and_then(sysinfo::Process::cgroup_limits).or_else(|| system.cgroup_limits())
        };
        #[cfg(not(target_os = "linux"))]
        let cgroup = system.cgroup_limits();
        MemorySnapshot::from_probe(system.total_memory(), system.available_memory(), cgroup.map(|limits| limits.free_memory.min(limits.total_memory)))
    }

    struct State {
        snapshot: MemorySnapshot,
        used: u64,
        epoch: u64,
        debit: u64,
        unsettled: u64,
        activities: u64,
        invalid: bool,
        #[cfg(test)]
        injected: Option<MemorySnapshot>,
    }
    impl State {
        fn inconsistent(&mut self) -> JobError {
            self.snapshot.capacity_bytes = 0;
            self.invalid = true;
            JobError::new(ErrorCode::Internal, "native render memory ledger is inconsistent")
        }

        fn valid(&self) -> Result<()> {
            if self.invalid { Err(JobError::new(ErrorCode::Internal, "native render memory ledger is inconsistent")) } else { Ok(()) }
        }

        fn refresh_if_quiescent(&mut self) -> Result<()> {
            self.valid()?;
            if self.unsettled != 0 || self.activities != 0 {
                return Ok(());
            }
            let epoch = self.epoch.checked_add(1).ok_or_else(|| self.inconsistent())?;
            #[cfg(test)]
            let snapshot = self.injected.unwrap_or_else(system_snapshot);
            #[cfg(not(test))]
            let snapshot = system_snapshot();
            // Existing settled backing is part of the new observation. No byte
            // estimate is added to OS availability, and no future promise is erased.
            self.snapshot = snapshot;
            self.epoch = epoch;
            self.debit = 0;
            Ok(())
        }

        fn receipt(&self, required_bytes: u64, credit: u64, policy: NativeMemoryPolicy) -> Result<HostMemoryAdmission> {
            self.valid()?;
            let additional_bytes =
                required_bytes.checked_sub(credit).ok_or_else(|| JobError::new(ErrorCode::Internal, "native memory credit exceeds the request"))?;
            let admitted_bytes = self
                .used
                .checked_sub(credit)
                .ok_or_else(|| JobError::new(ErrorCode::Internal, "native memory credit exceeds the held render leases"))?;
            if self.used.checked_add(additional_bytes).is_none_or(|bytes| bytes > NATIVE_CPU_POOL_BYTES) {
                return Err(JobError::new(
                    ErrorCode::LimitExceeded,
                    format!(
                        "native render shared software pool needs {additional_bytes} additional bytes; {} bytes already admitted, \
                         {NATIVE_CPU_POOL_BYTES} bytes allowed",
                        self.used
                    ),
                ));
            }
            // The fallback is an absolute allowance, not measured free RAM. It
            // must still charge settled leases from every earlier epoch.
            let physical_debit_bytes = if self.snapshot.source == HostMemorySource::ConservativeFallback { self.used } else { self.debit };
            let capacity_bytes = self.snapshot.capacity_bytes;
            let available_bytes = capacity_bytes.saturating_sub(physical_debit_bytes);
            if policy == NativeMemoryPolicy::AvailableRam && additional_bytes > available_bytes {
                let source = match self.snapshot.source {
                    HostMemorySource::SystemAvailable => "system available RAM",
                    HostMemorySource::SystemAndCgroup => "system and cgroup available RAM",
                    HostMemorySource::ConservativeFallback => "512 MiB fallback because the RAM query was unavailable",
                };
                return Err(JobError::new(
                    ErrorCode::LimitExceeded,
                    format!(
                        "native render memory needs {additional_bytes} additional bytes; {} bytes already admitted, \
                         {available_bytes} bytes available from the {capacity_bytes}-byte {source} snapshot \
                         ({physical_debit_bytes} bytes charged to this snapshot)",
                        self.used
                    ),
                ));
            }
            Ok(HostMemoryAdmission {
                required_bytes,
                credited_bytes: credit,
                additional_bytes,
                admitted_bytes,
                physical_debit_bytes,
                available_bytes,
                capacity_bytes,
                software_limit_bytes: NATIVE_CPU_POOL_BYTES,
                source: self.snapshot.source,
                policy,
            })
        }
    }

    pub(crate) struct NativeMemoryPool {
        state: Mutex<State>,
    }
    impl NativeMemoryPool {
        fn new() -> Self {
            Self {
                state: Mutex::new(State {
                    snapshot: MemorySnapshot::from_probe(0, 0, None),
                    used: 0,
                    epoch: 0,
                    debit: 0,
                    unsettled: 0,
                    activities: 0,
                    invalid: false,
                    #[cfg(test)]
                    injected: None,
                }),
            }
        }

        pub(crate) fn check(&self, required_bytes: u64, credit: u64, policy: NativeMemoryPolicy) -> Result<HostMemoryAdmission> {
            let mut state = self.state.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "native render memory lock poisoned"))?;
            state.refresh_if_quiescent()?;
            state.receipt(required_bytes, credit, policy)
        }

        pub(crate) fn reserve(self: &Arc<Self>, bytes: u64, policy: NativeMemoryPolicy) -> Result<MemoryLease> {
            let mut state = self.state.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "native render memory lock poisoned"))?;
            state.refresh_if_quiescent()?;
            state.receipt(bytes, 0, policy)?;
            let used = state.used.checked_add(bytes).ok_or_else(|| state.inconsistent())?;
            let debit = state.debit.checked_add(bytes).ok_or_else(|| state.inconsistent())?;
            let unsettled = state.unsettled.checked_add(bytes).ok_or_else(|| state.inconsistent())?;
            state.used = used;
            state.debit = debit;
            state.unsettled = unsettled;
            Ok(MemoryLease { pool: self.clone(), bytes, epoch: state.epoch, settled: false })
        }

        pub(crate) fn activity(self: &Arc<Self>) -> Result<MemoryActivity> {
            let mut state = self.state.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "native render memory lock poisoned"))?;
            state.refresh_if_quiescent()?;
            state.activities = state.activities.checked_add(1).ok_or_else(|| state.inconsistent())?;
            Ok(MemoryActivity { pool: self.clone() })
        }

        fn release(&self, bytes: u64, epoch: u64, unsettled: bool) -> Result<()> {
            if bytes == 0 {
                return Ok(());
            }
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.valid()?;
            let used = state.used.checked_sub(bytes).ok_or_else(|| state.inconsistent())?;
            let debit = if epoch == state.epoch {
                state.debit.checked_sub(bytes).ok_or_else(|| state.inconsistent())?
            } else if unsettled {
                return Err(state.inconsistent());
            } else {
                state.debit
            };
            let pending = if unsettled { state.unsettled.checked_sub(bytes).ok_or_else(|| state.inconsistent())? } else { state.unsettled };
            state.used = used;
            state.debit = debit;
            state.unsettled = pending;
            Ok(())
        }

        #[cfg(test)]
        pub(crate) fn testing(capacity_bytes: u64) -> Arc<Self> {
            let snapshot = MemorySnapshot { capacity_bytes, source: HostMemorySource::SystemAvailable };
            Arc::new(Self {
                state: Mutex::new(State {
                    snapshot,
                    used: 0,
                    epoch: 0,
                    debit: 0,
                    unsettled: 0,
                    activities: 0,
                    invalid: false,
                    injected: Some(snapshot),
                }),
            })
        }

        #[cfg(test)]
        pub(crate) fn set_test_snapshot(&self, snapshot: MemorySnapshot) {
            self.state.lock().unwrap().injected = Some(snapshot);
        }

        #[cfg(test)]
        pub(crate) fn set_test_capacity(&self, capacity_bytes: u64) {
            self.set_test_snapshot(MemorySnapshot { capacity_bytes, source: HostMemorySource::SystemAvailable });
        }

        #[cfg(test)]
        pub(crate) fn set_test_epoch(&self, epoch: u64) {
            let mut state = self.state.lock().unwrap();
            assert_eq!((state.used, state.activities), (0, 0));
            state.epoch = epoch;
        }
    }

    /// A zero-byte lifetime barrier: temporary task backing must drop before it.
    pub(crate) struct MemoryActivity {
        pool: Arc<NativeMemoryPool>,
    }
    impl Drop for MemoryActivity {
        fn drop(&mut self) {
            let mut state = self.pool.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(activities) = state.activities.checked_sub(1) {
                state.activities = activities;
            } else {
                let _ = state.inconsistent();
            }
        }
    }

    pub(crate) struct MemoryLease {
        pool: Arc<NativeMemoryPool>,
        bytes: u64,
        epoch: u64,
        settled: bool,
    }
    impl MemoryLease {
        /// The owned backing is complete and cannot allocate from this promise.
        pub(crate) fn settle(&mut self) -> Result<()> {
            if self.settled {
                return Ok(());
            }
            let mut state = self.pool.state.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "native render memory lock poisoned"))?;
            state.valid()?;
            if self.bytes != 0 && self.epoch != state.epoch {
                return Err(state.inconsistent());
            }
            state.unsettled = state.unsettled.checked_sub(self.bytes).ok_or_else(|| state.inconsistent())?;
            self.settled = true;
            Ok(())
        }

        pub(crate) fn split(&mut self, bytes: u64) -> Result<Self> {
            if self.settled {
                return Err(JobError::new(ErrorCode::InvalidRequest, "settled native memory cannot fund a new allocation"));
            }
            if bytes > self.bytes {
                return Err(JobError::new(ErrorCode::LimitExceeded, "native memory split exceeds its lease"));
            }
            self.bytes -= bytes;
            Ok(Self { pool: self.pool.clone(), bytes, epoch: self.epoch, settled: false })
        }

        pub(crate) fn merge_from(&mut self, other: &mut Self) -> Result<()> {
            if !Arc::ptr_eq(&self.pool, &other.pool) {
                return Err(JobError::new(ErrorCode::WrongOwner, "native memory leases belong to different pools"));
            }
            let mut state = self.pool.state.lock().map_err(|_| JobError::new(ErrorCode::HostUnavailable, "native render memory lock poisoned"))?;
            state.valid()?;
            if (self.bytes != 0 && (self.settled || self.epoch != state.epoch)) || (other.bytes != 0 && other.epoch != state.epoch) {
                return Err(JobError::new(ErrorCode::InvalidRequest, "retained native memory from an earlier epoch cannot fund new work"));
            }
            let bytes = self.bytes.checked_add(other.bytes).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "native memory merge overflow"))?;
            // A dropped checkpoint can return its already-funded slot while
            // the same active epoch remains pinned by Work/free-slot metadata.
            if other.settled {
                state.unsettled = state.unsettled.checked_add(other.bytes).ok_or_else(|| state.inconsistent())?;
            }
            if other.bytes != 0 {
                self.epoch = state.epoch;
                self.settled = false;
            }
            self.bytes = bytes;
            other.bytes = 0;
            Ok(())
        }

        pub(crate) fn shrink(&mut self, bytes: u64) -> Result<()> {
            let released =
                self.bytes.checked_sub(bytes).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "native memory shrink exceeds its lease"))?;
            self.pool.release(released, self.epoch, !self.settled)?;
            self.bytes = bytes;
            Ok(())
        }
    }
    impl Drop for MemoryLease {
        fn drop(&mut self) {
            let _ = self.pool.release(self.bytes, self.epoch, !self.settled);
        }
    }

    pub(crate) fn shared_native_memory() -> Arc<NativeMemoryPool> {
        static MEMORY: OnceLock<Arc<NativeMemoryPool>> = OnceLock::new();
        MEMORY.get_or_init(|| Arc::new(NativeMemoryPool::new())).clone()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use native::MemorySnapshot;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{MemoryActivity, MemoryLease, NativeMemoryPool, shared_native_memory};
