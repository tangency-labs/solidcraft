//! Open CASCADE (OCCT)'s adapter to SolidCraft's kernel interface: an opt-in experiment that
//! shows the interface fits a second kernel (`docs/kernel-support.md`). Only builds with the
//! `backend-occt` feature have it, and no build we ship turns that on.
//!
//! OCCT is LGPL-2.1 with the Open CASCADE exception. It lives in its own shared library, the
//! `occt_capi` module (`vendor/occt-capi`), which this build links dynamically and which can be
//! rebuilt and swapped on its own; no OCCT header or code is in SolidCraft's crates.
//!
//! What it takes: the primitives (box, cylinder, sphere, torus), booleans and constant-radius
//! fillets with mitred turns. Everything else stays on solidcore.
//!
//! A body crosses as STEP in both directions: solidcore's STEP writer and OCCT's reader on the
//! way in, OCCT's writer and solidcore's reader on the way out, so the result is a record in
//! canonical order like any other. That is the simplest exchange that works, not the exact one
//! the design asks for (§3); what it costs in accuracy is what the support matrix measures. OCCT's
//! history is not carried yet, so a result has none ([`Exported::history`] is `None`), which the
//! record's rules allow: a kernel that can't report history exactly reports none.

use std::sync::OnceLock;

use occt_capi::{Boolean, Shape};
use solidcore_record::brep::Brep;
use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_kernel_api::{
    Axis2, Backend, BackendError, BackendId, BoolOp, ChamferSide, Exported, FilletTurn, Modelling, OpKind, Primitive, Support,
};

use crate::body::Body;

/// Open CASCADE, through its C ABI module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Occt;

fn unsupported<T>(what: &str) -> Result<T, BackendError> {
    Err(BackendError::Unsupported(format!("{what} on Open CASCADE")))
}

fn failed(e: occt_capi::Error) -> BackendError {
    BackendError::Failed(format!("Open CASCADE: {e}"))
}

fn exchange(what: &str, e: impl std::fmt::Display) -> BackendError {
    BackendError::Exchange(format!("Open CASCADE exchange, {what}: {e}"))
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn xyz(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl Backend for Occt {
    type Form = Shape;
    type Error = BackendError;

    const ID: BackendId = BackendId("occt");

    fn version(&self) -> &'static str {
        static VERSION: OnceLock<String> = OnceLock::new();
        VERSION.get_or_init(|| format!("occt {} (occt_capi ABI {})", occt_capi::library_version(), occt_capi::abi_version()))
    }

    fn supports(&self, op: OpKind) -> Support {
        match op {
            OpKind::Box | OpKind::Cylinder | OpKind::Sphere | OpKind::Torus | OpKind::Boolean => Support::Yes,
            OpKind::Fillet => Support::Some("constant radius with mitred turns".into()),
            _ => Support::No,
        }
    }

    fn import(&self, record: &Brep) -> Result<Shape, BackendError> {
        let body = Body::from_record(record.clone()).map_err(|e| exchange("the record", e))?;
        let text = crate::step_export(&[&body], "SolidCraft").map_err(|e| exchange("solidcore's STEP writer", e))?;
        Shape::from_step(&text).map_err(|e| exchange("OCCT's STEP reader", e))
    }

    fn export(&self, form: &Shape) -> Result<Exported, BackendError> {
        let text = form.to_step().map_err(|e| exchange("OCCT's STEP writer", e))?;
        let read = crate::step_import(&text).map_err(|e| exchange("solidcore's STEP reader", e))?;
        match read.bodies.as_slice() {
            [one] => Ok(Exported { record: one.body.record().clone(), history: None }),
            // Apart solids would be one record of several regions; the STEP crossing gives them
            // as separate parts, so such a result stays on solidcore.
            many => Err(BackendError::Unsupported(format!("a result of {} solids through the STEP exchange", many.len()))),
        }
    }
}

impl Modelling for Occt {
    fn primitive(&self, p: &Primitive) -> Result<Shape, BackendError> {
        // Input solidcore refuses goes to solidcore, so the refusal reads the same.
        match *p {
            Primitive::Box { a, b } => {
                let (lo, size) = ([a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)], [(b.x - a.x).abs(), (b.y - a.y).abs(), (b.z - a.z).abs()]);
                if !finite(&lo) || !finite(&size) || size.iter().any(|s| *s <= 0.0) {
                    return unsupported("a degenerate box");
                }
                Shape::make_box(lo, size).map_err(failed)
            }
            Primitive::Cylinder { base, axis, radius, height } => {
                if !finite(&[base.x, base.y, base.z, axis.x, axis.y, axis.z, radius, height])
                    || radius <= 0.0
                    || height <= 0.0
                    || (axis.x == 0.0 && axis.y == 0.0 && axis.z == 0.0)
                {
                    return unsupported("a degenerate cylinder");
                }
                Shape::make_cylinder(xyz(base), xyz(axis), radius, height).map_err(failed)
            }
            Primitive::Sphere { center, radius } => {
                if !finite(&[center.x, center.y, center.z, radius]) || radius <= 0.0 {
                    return unsupported("a degenerate sphere");
                }
                Shape::make_sphere(xyz(center), radius).map_err(failed)
            }
            Primitive::Torus { center, major, minor } => {
                if !finite(&[center.x, center.y, center.z, major, minor]) || !(minor > 0.0 && major > minor) {
                    return unsupported("a degenerate torus");
                }
                Shape::make_torus(xyz(center), [0.0, 0.0, 1.0], major, minor).map_err(failed)
            }
        }
    }

    fn extrude(&self, _plane: &Plane, _regions: &[Region2], _start: f64, _end: f64) -> Result<Vec<Shape>, BackendError> {
        unsupported("extrude")
    }

    fn revolve(&self, _plane: &Plane, _regions: &[Region2], _axis: Axis2, _angle: f64, _whole: bool) -> Result<Vec<Shape>, BackendError> {
        unsupported("revolve")
    }

    fn boolean(&self, a: &Shape, b: &Shape, op: BoolOp) -> Result<Option<Shape>, BackendError> {
        let op = match op {
            BoolOp::Union => Boolean::Fuse,
            BoolOp::Cut => Boolean::Cut,
            BoolOp::Intersect => Boolean::Common,
        };
        let out = a.boolean(b, op).map_err(failed)?;
        Ok((out.solid_count().map_err(failed)? > 0).then_some(out))
    }

    fn fillet(&self, body: &Shape, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Shape, BackendError> {
        if turn != FilletTurn::Mitre {
            return unsupported("swept fillet turns");
        }
        if edges.is_empty() || !(radius > 0.0) || !radius.is_finite() || edges.iter().any(|p| !finite(&xyz(*p))) {
            return unsupported("a fillet solidcore refuses");
        }
        let points: Vec<[f64; 3]> = edges.iter().map(|p| xyz(*p)).collect();
        body.fillet_nearest(&points, radius).map_err(failed)
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

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn volume(b: &Body) -> f64 {
        crate::measure(b).unwrap().volume
    }

    #[test]
    fn it_takes_primitives_booleans_and_fillets_only() {
        for &op in OpKind::ALL {
            let routed = matches!(op, OpKind::Box | OpKind::Cylinder | OpKind::Sphere | OpKind::Torus | OpKind::Boolean | OpKind::Fillet);
            assert_eq!(Occt.supports(op) != Support::No, routed, "{}", op.name());
        }
    }

    #[test]
    fn a_box_made_there_comes_back_as_a_record() {
        let made = Occt.primitive(&Primitive::Box { a: v(0.0, 0.0, 0.0), b: v(1.0, 2.0, 3.0) }).unwrap();
        let out = Occt.export(&made).unwrap();
        let body = Body::from_record(out.record).unwrap();
        assert!((volume(&body) - 6.0).abs() < 1e-9);
        assert_eq!(body.record().faces.len(), 6);
    }

    #[test]
    fn a_record_goes_there_and_back() {
        let b = crate::build::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 3.0).unwrap();
        let back = Body::from_record(Occt.export(&Occt.import(b.record()).unwrap()).unwrap().record).unwrap();
        assert!((volume(&back) - volume(&b)).abs() < 1e-9 * volume(&b));
    }

    #[test]
    fn a_routed_cut_is_made_there() {
        crate::route::foreign::set_kernel(crate::route::foreign::Kernel::Occt);
        let a = crate::route::box_solid(v(0.0, 0.0, 0.0), v(2.0, 2.0, 2.0)).unwrap();
        let b = crate::route::box_solid(v(1.0, 1.0, 1.0), v(3.0, 3.0, 3.0)).unwrap();
        let cut = crate::route::boolean(&a, &b, BoolOp::Cut).unwrap().unwrap();
        assert!((volume(&cut) - 7.0).abs() < 1e-9);
        assert!(cut.history().is_none(), "the STEP exchange carries no history yet");
    }

    #[test]
    fn a_cut_that_removes_everything_is_empty() {
        let a = Occt.primitive(&Primitive::Box { a: v(0.0, 0.0, 0.0), b: v(1.0, 1.0, 1.0) }).unwrap();
        let b = Occt.primitive(&Primitive::Box { a: v(-1.0, -1.0, -1.0), b: v(2.0, 2.0, 2.0) }).unwrap();
        assert!(Occt.boolean(&a, &b, BoolOp::Cut).unwrap().is_none());
    }

    #[test]
    fn a_fillet_rounds_the_edge_nearest_the_point() {
        let b = Occt.primitive(&Primitive::Box { a: v(0.0, 0.0, 0.0), b: v(10.0, 10.0, 10.0) }).unwrap();
        let r = 1.0;
        let f = Occt.fillet(&b, &[v(5.0, 0.0, 10.0)], r, FilletTurn::Mitre).unwrap();
        // One edge rounded: the box less a prism of section r² − πr²/4, 10 long.
        let expected = 1000.0 - 10.0 * (r * r - std::f64::consts::PI * r * r / 4.0);
        assert!((f.volume() - expected).abs() < 1e-6, "{}", f.volume());
    }

    #[test]
    fn input_solidcore_refuses_goes_to_solidcore() {
        let e = Occt.primitive(&Primitive::Sphere { center: v(0.0, 0.0, 0.0), radius: -1.0 }).unwrap_err();
        assert!(matches!(e, BackendError::Unsupported(_)), "{e:?}");
        let routed = crate::route::sphere(v(0.0, 0.0, 0.0), -1.0).unwrap_err();
        assert_eq!(routed, crate::build::sphere(v(0.0, 0.0, 0.0), -1.0).unwrap_err());
    }
}
