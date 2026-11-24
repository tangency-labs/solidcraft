//! solidcore's adapter to SolidCraft's kernel interface ([`solidcraft_kernel_api::Backend`]).
//!
//! The one adapter SolidCraft maintains, and the default. Each operation is the call this crate
//! made before the interface existed, unchanged: the router ([`crate::route`]) sends every
//! operation here, so results are the same bytes as calling those functions directly.
//!
//! solidcore's error is [`KernelError`] itself, so the words a user reads can't change; it sorts
//! into [`BackendError`] for anything that compares kernels.

use solidcore_record::brep::Brep;
use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_kernel_api::{Axis2, Backend, BackendError, BackendId, BoolOp, ChamferSide, Exported, FilletTurn, OpKind, Primitive, Support};

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
        Ok(Exported { record: form.record().clone(), history: form.history().cloned() })
    }

    fn primitive(&self, p: &Primitive) -> Result<Body> {
        match *p {
            Primitive::Box { a, b } => crate::build::box_solid(a, b),
            Primitive::Cylinder { base, axis, radius, height } => crate::build::cylinder(base, axis, radius, height),
            Primitive::Sphere { center, radius } => crate::build::sphere(center, radius),
            Primitive::Torus { center, major, minor } => crate::build::torus(center, major, minor),
        }
    }

    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
        crate::build::extrude(plane, regions, start, end)
    }

    fn revolve(&self, plane: &Plane, regions: &[Region2], axis: Axis2, angle: f64, whole: bool) -> Result<Vec<Body>> {
        if whole {
            crate::build::revolve_whole(plane, regions, axis.origin, axis.dir, angle)
        } else {
            crate::build::revolve(plane, regions, axis.origin, axis.dir, angle)
        }
    }

    fn boolean(&self, a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        crate::ops::boolean(a, b, op)
    }

    fn fillet(&self, body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Body> {
        crate::blend::fillet_turning(body, edges, radius, turn)
    }

    fn chamfer(&self, body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
        crate::blend::chamfer(body, edges, distance)
    }

    fn chamfer_sides(&self, body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
        crate::blend::chamfer_sides(body, edges, d1, side, flip)
    }

    fn shell(&self, body: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
        crate::polyhedron::shell(body, open, thickness)
    }

    fn thicken(&self, sheet: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
        crate::surfaces::thicken(sheet, thickness, symmetric)
    }

    fn offset_faces(&self, body: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
        crate::polyhedron::offset_faces(body, at, distance)
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
            e @ (KernelError::Lip(_) | KernelError::Repair(_) | KernelError::Heal(_)) => BackendError::Failed(e.to_string()),
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
        for op in OpKind::ALL {
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
