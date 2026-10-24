//! FOCAS machine adapter stub (Fanuc).
//!
//! Full SDK binding lands later; this registers the protocol for machine profiles.

use crate::job::Availability;
use crate::machine_host::{MachineBackend, MachineBackendDescriptor, MachineLiveStatus, MachineResult};

/// Placeholder Fanuc FOCAS backend (not linked to the OEM SDK).
pub struct FocasBackend;

impl MachineBackend for FocasBackend {
    fn descriptor(&self) -> MachineBackendDescriptor {
        MachineBackendDescriptor {
            id: "focas".into(),
            label: "Fanuc FOCAS (stub)".into(),
            protocol: "focas".into(),
            availability: Availability::Unavailable,
            note: Some("FOCAS SDK not linked in this build".into()),
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
            message: Some("FOCAS stub: SDK not linked".into()),
            source: "focas".into(),
            ..MachineLiveStatus::default()
        })
    }
}
