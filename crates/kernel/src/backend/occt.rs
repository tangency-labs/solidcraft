//! Open CASCADE (OCCT)'s adapter to SolidCraft's kernel interface: an opt-in experiment that
//! shows the interface fits a second kernel (`docs/kernel-support.md`). Only builds with the
//! `backend-occt` feature have it, and no build we ship turns that on.
//!
//! This is the slot the router sends to ([`crate::route::foreign`]). It takes no operation yet,
//! so every one stays on solidcore.

use solidcore_record::brep::Brep;
use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_kernel_api::{
    Axis2, Backend, BackendError, BackendId, BoolOp, ChamferSide, Exported, FilletTurn, Modelling, OpKind, Primitive, Support,
};

/// Open CASCADE, through its C ABI module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Occt;

/// An Open CASCADE shape.
#[derive(Debug)]
pub struct Shape;

fn unsupported<T>(what: &str) -> Result<T, BackendError> {
    Err(BackendError::Unsupported(format!("{what} on Open CASCADE")))
}

impl Backend for Occt {
    type Form = Shape;
    type Error = BackendError;

    const ID: BackendId = BackendId("occt");

    fn version(&self) -> &'static str {
        "occt (not built in)"
    }

    fn supports(&self, _op: OpKind) -> Support {
        Support::No
    }

    fn import(&self, _record: &Brep) -> Result<Shape, BackendError> {
        unsupported("a record")
    }

    fn export(&self, _form: &Shape) -> Result<Exported, BackendError> {
        unsupported("a record")
    }
}

impl Modelling for Occt {
    fn primitive(&self, _p: &Primitive) -> Result<Shape, BackendError> {
        unsupported("a primitive")
    }

    fn extrude(&self, _plane: &Plane, _regions: &[Region2], _start: f64, _end: f64) -> Result<Vec<Shape>, BackendError> {
        unsupported("extrude")
    }

    fn revolve(&self, _plane: &Plane, _regions: &[Region2], _axis: Axis2, _angle: f64, _whole: bool) -> Result<Vec<Shape>, BackendError> {
        unsupported("revolve")
    }

    fn boolean(&self, _a: &Shape, _b: &Shape, _op: BoolOp) -> Result<Option<Shape>, BackendError> {
        unsupported("a boolean")
    }

    fn fillet(&self, _body: &Shape, _edges: &[Vec3], _radius: f64, _turn: FilletTurn) -> Result<Shape, BackendError> {
        unsupported("a fillet")
    }

    fn chamfer(&self, _body: &Shape, _edges: &[Vec3], _distance: f64) -> Result<Shape, BackendError> {
        unsupported("a chamfer")
    }

    fn chamfer_sides(&self, _body: &Shape, _edges: &[Vec3], _d1: f64, _side: ChamferSide, _flip: bool) -> Result<Shape, BackendError> {
        unsupported("a chamfer")
    }

    fn shell(&self, _body: &Shape, _open: &[Vec3], _thickness: f64) -> Result<Shape, BackendError> {
        unsupported("a shell")
    }

    fn thicken(&self, _sheet: &Shape, _thickness: f64, _symmetric: bool) -> Result<Shape, BackendError> {
        unsupported("thicken")
    }

    fn offset_faces(&self, _body: &Shape, _at: &[Vec3], _distance: f64) -> Result<Shape, BackendError> {
        unsupported("an offset")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_takes_nothing_yet() {
        for &op in OpKind::ALL {
            assert_eq!(Occt.supports(op), Support::No, "{}", op.name());
        }
    }

    #[test]
    fn a_routed_box_stays_on_solidcore() {
        let (a, b) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 3.0, 4.0));
        let routed = crate::route::box_solid(a, b).unwrap();
        let direct = crate::build::box_solid(a, b).unwrap();
        assert_eq!(routed.record(), direct.record());
    }
}
