//! THINC machine adapter stub (Okuma).
//!
//! Full API binding lands later; this registers the protocol for machine profiles.

use crate::job::Availability;
use crate::machine_host::{MachineBackend, MachineBackendDescriptor, MachineLiveStatus, MachineResult};

/// Placeholder Okuma THINC backend (not linked to the OEM API).
pub struct ThincBackend;

impl MachineBackend for ThincBackend {
    fn descriptor(&self) -> MachineBackendDescriptor {
        MachineBackendDescriptor {
            id: "thinc".into(),
            label: "Okuma THINC (stub)".into(),
            protocol: "thinc".into(),
            availability: Availability::Unavailable,
            note: Some("THINC API not linked in this build".into()),
            endpoint: None,
        }
    }

    fn is_available(&self) -> bool {
        false
    }

    fn status(&self) -> MachineResult<MachineLiveStatus> {
        Ok(MachineLiveStatus {
            available: false,
            execution: "UNAVAILABLE".into(),
            message: Some("THINC stub: API not linked".into()),
            source: "thinc".into(),
            ..MachineLiveStatus::default()
        })
    }
}
