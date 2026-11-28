//! solidcore's adapter to SolidCraft's kernel interface ([`solidcraft_kernel_api::Backend`]).
//!
//! The one adapter SolidCraft maintains, and the default. Each operation is the call this crate
//! made before the interface existed, unchanged: the router ([`crate::route`]) sends every
//! operation here, so results are the same bytes as calling those functions directly.
//!
//! solidcore's error is [`KernelError`] itself, so the words a user reads can't change; it sorts
//! into [`BackendError`] for anything that compares kernels.

use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{Backend, BackendError, BackendId, ExactBody, ExactExported, Exported, OpKind, Support};

mod affine;
mod cell_grid;
mod curves;
mod develop;
mod dxf;
mod exchange;
mod frames;
mod hatch;
mod hole_stacks;
mod mesh;
mod modelling;
#[cfg(feature = "backend-occt")]
mod occt;
mod planar;
mod points;
mod profiles;
mod projection;
mod query;
mod runtime;
mod sections;
mod shaft_spline;
mod tessellate;
mod tube_cope;

use crate::body::Body;
use crate::{KernelError, Result};

#[cfg(feature = "backend-occt")]
pub(crate) use occt::Occt;

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

    /// An exact body assembled by solidcore's assembly stage, with no history (it has no
    /// operation's operands to be over).
    fn import_exact(&self, body: &ExactBody) -> Option<Result<Body>> {
        Some(crate::exact::body_from_exact(ExactExported { body: body.clone(), history: None }, &[]))
    }

    fn export_exact(&self, form: &Body) -> Option<Result<ExactExported>> {
        Some(crate::exact::exact_exported(form))
    }
}

/// The application's error, sorted into the interface's kinds. A message keeps its words.
impl From<KernelError> for BackendError {
    fn from(e: KernelError) -> BackendError {
        match e {
            KernelError::Invalid(m) => BackendError::Invalid(m),
            // The same words the record check gave before this had its own variant.
            KernelError::CoordinateCeiling => BackendError::Invalid(solidcore_record::brep::Invalid(crate::BREP_COORDINATE_CEILING).to_string()),
            KernelError::TooLarge(why) => {
                let largest = crate::blend::named_fit(&why);
                BackendError::TooLarge { why, largest }
            }
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
            BackendError::TooLarge { why, largest } => {
                let why = match largest {
                    Some(size) if size.is_finite() && size > 0.0 && !crate::blend::names_largest_fit(&why) => {
                        format!("{why}; {}", crate::blend::largest_fit_note("radius", size))
                    }
                    _ => why,
                };
                KernelError::TooLarge(why)
            }
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
    fn too_large_carries_largest_fit_across_backend_boundary() {
        let sized = KernelError::TooLarge("the fillet is larger than the faces beside its edges allow; the largest radius that fits is 3 mm".into());
        assert_eq!(sized.largest_fit(), Some(3.0));
        assert_eq!(sized.blend_fit(), Some(crate::BlendFit::Radius(3.0)));
        let backend = BackendError::from(sized.clone());
        assert_eq!(
            backend,
            BackendError::TooLarge {
                why: "the fillet is larger than the faces beside its edges allow; the largest radius that fits is 3 mm".into(),
                largest: Some(3.0),
            }
        );
        assert_eq!(KernelError::from(backend), sized);

        // A backend reporting `largest` separately without embedding the note still surfaces it.
        let external = BackendError::TooLarge { why: "the fillet is larger than the faces beside its edges allow".into(), largest: Some(2.5) };
        let converted = KernelError::from(external);
        assert_eq!(converted.largest_fit(), Some(2.5));
        assert!(converted.to_string().contains("the largest radius that fits is 2.5 mm"), "{converted}");

        // A backend that reports no fit size stays clean without inventing one.
        let without_size = BackendError::TooLarge { why: "the fillet is larger than the faces beside its edges allow".into(), largest: None };
        assert_eq!(KernelError::from(without_size).largest_fit(), None);
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
