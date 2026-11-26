//! solidcore's adapter to SolidCraft's kernel interface ([`solidcraft_kernel_api::Backend`]).
//!
//! The one adapter SolidCraft maintains, and the default. Each operation is the call this crate
//! made before the interface existed, unchanged: the router ([`crate::route`]) sends every
//! operation here, so results are the same bytes as calling those functions directly.
//!
//! solidcore's error is [`KernelError`] itself, so the words a user reads can't change; it sorts
//! into [`BackendError`] for anything that compares kernels.

use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{Backend, BackendError, BackendId, Exported, OpKind, Support};

mod affine;
mod cell_grid;
mod exchange;
mod frames;
mod modelling;
mod projection;

use crate::body::Body;
use crate::{KernelError, Result};

/// solidcore, through `solidcore-ops`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Solidcore;

impl Backend for Solidcore {
    type Form = Body;
    type Error = KernelError;

    const ID: BackendId = BackendId("solidcore");

    fn version(&self) -> &'static str {
        concat!("solidcore (solidcraft-kernel ", env!("CARGO_PKG_VERSION"), ")")
    }

    fn supports(&self, _op: OpKind) -> Support {
        Support::Yes
    }

    fn import(&self, record: &Brep) -> Result<Body> {
        Body::from_record(record.clone())
    }

    fn export(&self, form: &Body) -> Result<Exported> {
        // A mesh body's record is empty: its geometry is the mesh, which no record carries.
        if form.is_mesh() {
            return Err(KernelError::Failed("a mesh body has no B-rep record to export".into()));
        }
        Ok(Exported { record: form.record().clone(), history: form.history().cloned() })
    }
}

/// The application's error, sorted into the interface's kinds. A message keeps its words.
impl From<KernelError> for BackendError {
    fn from(e: KernelError) -> BackendError {
        match e {
            KernelError::Invalid(m) => BackendError::Invalid(m),
            KernelError::TooLarge(why) => BackendError::TooLarge { why, largest: None },
            KernelError::Internal(m) => BackendError::Internal(m),
            KernelError::Failed(m) => BackendError::Failed(m),
            // The words after "the operation failed: ", so the way back reads the same.
            KernelError::Lip(r) => BackendError::Failed(format!("raising a band: {r}")),
            KernelError::Repair(r) => BackendError::Failed(r.to_string()),
            KernelError::Heal(h) => BackendError::Failed(h.to_string()),
            KernelError::Dome(r) => BackendError::Failed(r.to_string()),
        }
    }
}

/// Another kernel's failure as the application's error: what the user reads.
impl From<BackendError> for KernelError {
    fn from(e: BackendError) -> KernelError {
        match e {
            BackendError::Invalid(m) => KernelError::Invalid(m),
            BackendError::TooLarge { why, .. } => KernelError::TooLarge(why),
            BackendError::Internal(m) => KernelError::Internal(m),
            e @ BackendError::Cancelled(_) => KernelError::Failed(e.to_string()),
            e @ BackendError::Unsupported(_) => KernelError::Failed(e.to_string()),
            BackendError::NoRoom { why: m, .. }
            | BackendError::Numerical(m)
            | BackendError::InvalidResult(m)
            | BackendError::Failed(m)
            | BackendError::Exchange(m) => KernelError::Failed(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    #[test]
    fn errors_keep_their_words_both_ways() {
        for e in [
            KernelError::Invalid("radius must be positive".into()),
            KernelError::Failed("boolean cancelled".into()),
            KernelError::TooLarge("the radius is too large for the faces".into()),
            KernelError::Internal("caught a panic".into()),
        ] {
            assert_eq!(KernelError::from(BackendError::from(e.clone())), e);
        }
    }

    #[test]
    fn refusals_of_their_own_read_the_same_both_ways() {
        for e in [
            KernelError::Lip(crate::LipRefusal::BandFillsRim),
            KernelError::Repair(crate::RepairRefusal::NothingToRepair),
            KernelError::Heal(crate::HealFailure::Stopped),
        ] {
            assert_eq!(KernelError::from(BackendError::from(e.clone())).to_string(), e.to_string());
        }
    }

    #[test]
    fn a_mesh_body_does_not_export_as_an_empty_record() {
        let points = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        let mesh = crate::mesh_body(&points, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
        assert!(mesh.is_mesh());
        assert!(Solidcore.export(&mesh).is_err());
    }

    #[test]
    fn a_cancelled_call_reads_as_the_kernel_says_it() {
        assert_eq!(KernelError::from(BackendError::Cancelled("boolean".into())), crate::watch::cancelled_error("boolean"));
    }

    #[test]
    fn unsupported_reads_as_not_supported_yet() {
        assert_eq!(
            KernelError::from(BackendError::Unsupported("swept corners".into())).to_string(),
            "the operation failed: not supported yet: swept corners"
        );
    }

    #[test]
    fn the_adapter_takes_every_operation() {
        for &op in OpKind::ALL {
            assert_eq!(Solidcore.supports(op), Support::Yes, "{}", op.name());
        }
    }

    #[test]
    fn a_body_exports_as_its_record_and_imports_back() {
        let b = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let out = Solidcore.export(&b).unwrap();
        assert_eq!(&out.record, b.record());
        let back = Solidcore.import(&out.record).unwrap();
        assert_eq!(back.record(), b.record());
    }
}
