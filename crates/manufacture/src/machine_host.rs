//! Machine plugin host — OEM SDK / protocol adapters as swappable backends.
//!
//! Mirrors [`crate::job::CamHost`]: core Manufacture keeps profiles + posts; live machine
//! I/O lives behind [`MachineBackend`]. Test with [`FakeMachineHost`]; shop floor with
//! MTConnect / FOCAS / THINC plugins installed on the session.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::job::Availability;

/// Description of an installed machine plugin (metadata only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineBackendDescriptor {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub availability: Availability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// Live snapshot from a machine plugin (not persisted).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineLiveStatus {
    pub available: bool,
    /// MTConnect Execution / controller state (ACTIVE, READY, STOPPED, …).
    pub execution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_rpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedrate_mmpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// `fake`, `mtconnect`, …
    pub source: String,
}

impl Default for MachineLiveStatus {
    fn default() -> Self {
        Self {
            available: false,
            execution: "UNAVAILABLE".into(),
            program: None,
            tool_number: None,
            spindle_rpm: None,
            feedrate_mmpm: None,
            message: None,
            source: "none".into(),
        }
    }
}

/// Soft error from a machine plugin.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct MachineError {
    pub message: String,
}

impl MachineError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

pub type MachineResult<T> = Result<T, MachineError>;

/// OEM / protocol adapter. Session-owned; never stored in the document.
pub trait MachineBackend: Send + Sync + 'static {
    fn descriptor(&self) -> MachineBackendDescriptor;
    fn is_available(&self) -> bool;
    fn status(&self) -> MachineResult<MachineLiveStatus>;
    fn pump(&self) {}
}

/// Caller-owned handle to a machine plugin. `Clone` is cheap (`Arc`).
#[derive(Clone)]
pub struct MachineHost(Arc<dyn MachineBackend>);

impl MachineHost {
    pub fn new(backend: impl MachineBackend) -> Self {
        MachineHost(Arc::new(backend))
    }

    pub fn fake() -> Self {
        MachineHost::new(FakeMachineHost::default())
    }

    pub fn fake_with(status: MachineLiveStatus) -> Self {
        MachineHost::new(FakeMachineHost { status })
    }

    pub fn descriptor(&self) -> MachineBackendDescriptor {
        self.0.descriptor()
    }

    pub fn is_available(&self) -> bool {
        self.0.is_available()
    }

    pub fn status(&self) -> MachineResult<MachineLiveStatus> {
        self.0.status()
    }

    pub fn pump(&self) {
        self.0.pump();
    }
}

/// Deterministic in-process plugin for unit tests (no network).
#[derive(Clone, Debug)]
pub struct FakeMachineHost {
    pub status: MachineLiveStatus,
}

impl Default for FakeMachineHost {
    fn default() -> Self {
        Self {
            status: MachineLiveStatus {
                available: true,
                execution: "READY".into(),
                program: Some("O1000".into()),
                tool_number: Some(1),
                spindle_rpm: Some(0.0),
                feedrate_mmpm: Some(0.0),
                message: Some("fake machine plugin".into()),
                source: "fake".into(),
            },
        }
    }
}

impl MachineBackend for FakeMachineHost {
    fn descriptor(&self) -> MachineBackendDescriptor {
        MachineBackendDescriptor {
            id: "fake-machine".into(),
            label: "Fake machine (tests)".into(),
            protocol: "fake".into(),
            availability: Availability::Available,
            note: Some("Deterministic status for plugin wiring tests".into()),
            endpoint: None,
        }
    }

    fn is_available(&self) -> bool {
        self.status.available
    }

    fn status(&self) -> MachineResult<MachineLiveStatus> {
        Ok(self.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_host_reports_ready() {
        let host = MachineHost::fake();
        assert!(host.is_available());
        let st = host.status().unwrap();
        assert_eq!(st.execution, "READY");
        assert_eq!(st.tool_number, Some(1));
        assert_eq!(host.descriptor().protocol, "fake");
    }
}
