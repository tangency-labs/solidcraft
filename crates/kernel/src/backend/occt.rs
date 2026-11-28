//! Open CASCADE (OCCT)'s adapter to SolidCraft's kernel interface: an opt-in experiment that
//! shows the interface fits a second kernel (`docs/kernel-support.md`). Only builds with the
//! `backend-occt` feature have it, and no build we ship turns that on.
//!
//! OCCT is LGPL-2.1 with the Open CASCADE exception. It lives in its own shared library, the
//! `occt_capi` module (`vendor/occt-capi`), which this build links dynamically and which can be
//! rebuilt and swapped on its own; no OCCT header or code is in SolidCraft's crates.
//!
//! What it takes: the primitives (box, cylinder, sphere, torus), straight extrusions and
//! revolutions of sketch profiles, booleans and constant-radius fillets with mitred turns.
//! Everything else stays on solidcore.
//!
//! An extrusion or revolution is planned by solidcore's builder (the input checked, collinear
//! lines and runs of one curve joined, loops broken at their seams), so OCCT sweeps the very
//! curves solidcore would; the router then names the faces that come back as solidcore names its
//! own, and runs solidcore where they don't name alike.
//!
//! A body crosses as STEP in both directions: solidcore's STEP writer and OCCT's reader on the
//! way in, OCCT's writer and solidcore's reader on the way out, so the result is a record in
//! canonical order like any other. That is the simplest exchange that works, not the exact one
//! the design asks for (§3); what it costs in accuracy is what the support matrix measures. OCCT's
//! history is not carried yet, so a result has none ([`Exported::history`] is `None`), which the
//! record's rules allow: a kernel that can't report history exactly reports none.

use std::sync::OnceLock;

use occt_capi::{Boolean, Shape};
use solidcore_ops::sweep::Curve;
use solidcore_record::brep::Brep;
use solidcore_record::{Loop2, Plane, Region2, Vec3};
use solidcraft_kernel_api::{
    Axis2, Backend, BackendError, BackendId, BoolOp, ChamferSide, Exported, ExtrudeSides, FilletTurn, Modelling, OpKind, Primitive, RevolveSides,
    SplitTool, Support, Taper, TrimKeep,
};
use solidcraft_kernel_api::{Helix, PathSpan, PointPath, RailFit, SweepOptions};

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

/// Input solidcore's builder refuses: solidcore runs it, so the refusal reads the same.
fn refused(e: crate::KernelError) -> BackendError {
    BackendError::Unsupported(format!("input solidcore refuses ({e})"))
}

/// Where a curve starts and ends.
fn ends(c: &Curve) -> Option<([f64; 3], [f64; 3])> {
    match c {
        Curve::Line { start, end } => Some((*start, *end)),
        Curve::Arc { center, axis, reference, radius, start, end } => {
            let y = [
                axis[1] * reference[2] - axis[2] * reference[1],
                axis[2] * reference[0] - axis[0] * reference[2],
                axis[0] * reference[1] - axis[1] * reference[0],
            ];
            let at = |t: f64| [0, 1, 2].map(|i| center[i] + radius * (t.cos() * reference[i] + t.sin() * y[i]));
            Some((at(*start), at(*end)))
        }
        Curve::Nurbs { points, .. } => {
            let (a, b) = (points.first()?, points.last()?);
            Some(([a[0], a[1], a[2]], [b[0], b[1], b[2]]))
        }
    }
}

/// A profile's curves (loop after loop, each starting where the one before ends) in the module's
/// encoding ([`Shape::prism_profile`]): a loop ends at the curve that returns to its start.
fn encode_profile(curves: &[Curve]) -> Result<Vec<f64>, BackendError> {
    let scale = curves.iter().filter_map(ends).flat_map(|(a, b)| a.into_iter().chain(b)).fold(1.0_f64, |m, v| m.max(v.abs()));
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt() <= 1e-9 * scale;
    let mut loops: Vec<Vec<f64>> = Vec::new();
    let mut current: Vec<f64> = Vec::new();
    let (mut count, mut first) = (0usize, None);
    for c in curves {
        let (a, b) = ends(c).ok_or_else(|| BackendError::Unsupported("a profile curve with no control points".into()))?;
        let start = *first.get_or_insert(a);
        match c {
            Curve::Line { start, end } => {
                current.push(0.0);
                current.extend(start.iter().chain(end));
            }
            Curve::Arc { center, axis, reference, radius, start, end } => {
                current.push(1.0);
                current.extend(center.iter().chain(axis).chain(reference));
                current.extend([*radius, *start, *end]);
            }
            Curve::Nurbs { degree, points, knots } => {
                current.push(2.0);
                current.extend([*degree as f64, points.len() as f64, knots.len() as f64]);
                current.extend(points.iter().flatten());
                current.extend(knots);
            }
        }
        count += 1;
        if near(b, start) {
            let mut l = vec![count as f64];
            l.append(&mut current);
            loops.push(l);
            (count, first) = (0, None);
        }
    }
    if count > 0 || loops.is_empty() {
        return Err(BackendError::Unsupported("a profile loop that does not close".into()));
    }
    let mut out = vec![loops.len() as f64];
    out.extend(loops.into_iter().flatten());
    Ok(out)
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
            OpKind::Extrude => Support::Some("straight extrusions; not tapered, not the material of overlapping regions".into()),
            OpKind::Revolve => Support::Some("arcs not joined first".into()),
            OpKind::Fillet => Support::Some("constant radius with mitred turns".into()),
            OpKind::Chamfer => Support::Some("equal chamfers".into()),
            OpKind::Shell => Support::Some("open at one face or more".into()),
            OpKind::Thicken => Support::Yes,
            OpKind::OffsetFaces => Support::Yes,
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

    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64, sides: ExtrudeSides<'_>) -> Result<Vec<Shape>, BackendError> {
        let one_curve = match sides {
            ExtrudeSides::AsDrawn => None,
            ExtrudeSides::Whole(f) => Some(f),
        };
        let plans = crate::build::extrusion_plans(plane, regions, start, end, one_curve, |x| {
            crate::smlib_sweeps::extrusion_curves(plane, &x.pieces, x.from).map(|c| (c, x.distance))
        })
        .map_err(refused)?;
        plans.into_iter().map(|((curves, dir), distance)| Shape::prism_profile(&encode_profile(&curves)?, dir, distance).map_err(failed)).collect()
    }

    fn extrude_material(&self, _plane: &Plane, _regions: &[Region2], _start: f64, _end: f64) -> Result<Shape, BackendError> {
        unsupported("extrude")
    }

    fn extrude_tapered(&self, _plane: &Plane, _region: &Region2, _taper: &Taper) -> Result<Shape, BackendError> {
        unsupported("a tapered extrude")
    }

    fn revolve(
        &self,
        plane: &Plane,
        regions: &[Region2],
        axis: Axis2,
        angle: f64,
        _whole: bool,
        sides: RevolveSides<'_>,
    ) -> Result<Vec<Shape>, BackendError> {
        let one_curve = match sides {
            RevolveSides::AsDrawn => None,
            RevolveSides::JoinFreeForm(f) => Some(f),
            // solidcore's revolution certifies and joins pieces of one circle as it turns them.
            RevolveSides::JoinArcs => return unsupported("a revolution that joins arcs"),
        };
        let plans = crate::build::revolution_plans_of(plane, regions, axis.origin, axis.dir, angle, one_curve).map_err(refused)?;
        plans
            .iter()
            .map(|x| {
                let (curves, origin, k, turn) =
                    crate::smlib_sweeps::revolution_curves(plane, &x.pieces, x.origin, x.axis, x.angle).map_err(refused)?;
                Shape::revolve_profile(&encode_profile(&curves)?, origin, k, turn).map_err(failed)
            })
            .collect()
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

    fn fillet_removing(&self, _body: &Shape, _edges: &[Vec3], _radius: f64, _turn: FilletTurn) -> Result<(Shape, Vec<Vec3>), BackendError> {
        unsupported("a fillet that removes overflowing picks")
    }

    fn fillet_each(&self, _body: &Shape, _edges: &[Vec3], _radii: &[f64], _turn: FilletTurn) -> Result<Shape, BackendError> {
        unsupported("a fillet of several radii")
    }

    fn fillet_setback(&self, _body: &Shape, _edges: &[Vec3], _radius: f64, _setbacks: &[(Vec3, f64)]) -> Result<Shape, BackendError> {
        unsupported("a setback fillet")
    }

    fn chamfer(&self, body: &Shape, edges: &[Vec3], distance: f64) -> Result<Shape, BackendError> {
        if edges.is_empty() || edges.len() > 1000 || !(distance > 1e-6 && distance < 1e6) || edges.iter().any(|p| !finite(&xyz(*p))) {
            return unsupported("a chamfer solidcore refuses");
        }
        let points: Vec<[f64; 3]> = edges.iter().map(|p| xyz(*p)).collect();
        body.chamfer_nearest(&points, distance).map_err(failed)
    }

    fn chamfer_carrying_face(&self, _body: &Shape, _edges: &[Vec3], _distance: f64) -> Result<Shape, BackendError> {
        unsupported("a chamfer")
    }

    fn chamfer_sides_carrying_face(&self, _body: &Shape, _edges: &[Vec3], _d1: f64, _side: ChamferSide, _flip: bool) -> Result<Shape, BackendError> {
        unsupported("a chamfer")
    }

    fn chamfer_sides(&self, _body: &Shape, _edges: &[Vec3], _d1: f64, _side: ChamferSide, _flip: bool) -> Result<Shape, BackendError> {
        unsupported("a chamfer")
    }

    fn shell(&self, body: &Shape, open: &[Vec3], thickness: f64) -> Result<Shape, BackendError> {
        // A closed hollow (no open face) is one solid inside another: OCCT makes it, the record
        // takes it, but solidcore's shell is what the app's checks expect, so it stays there.
        if open.is_empty() || !(thickness > 0.0 && thickness.is_finite()) || open.iter().any(|p| !finite(&xyz(*p))) {
            return unsupported("a shell with no open face, or one solidcore refuses");
        }
        let points: Vec<[f64; 3]> = open.iter().map(|p| xyz(*p)).collect();
        body.shell_nearest(&points, thickness).map_err(failed)
    }

    fn thicken(&self, sheet: &Shape, thickness: f64, symmetric: bool) -> Result<Shape, BackendError> {
        if !(thickness != 0.0 && thickness.is_finite()) {
            return unsupported("a thickness solidcore refuses");
        }
        sheet.thicken(thickness, symmetric).map_err(failed)
    }

    fn offset_faces(&self, body: &Shape, at: &[Vec3], distance: f64) -> Result<Shape, BackendError> {
        if at.is_empty() || !distance.is_finite() || distance == 0.0 || distance.abs() > 1e6 || at.iter().any(|p| !finite(&xyz(*p))) {
            return unsupported("an offset solidcore refuses, or one of 0");
        }
        let points: Vec<[f64; 3]> = at.iter().map(|p| xyz(*p)).collect();
        body.offset_faces_nearest(&points, distance).map_err(failed)
    }

    fn draft(&self, _body: &Shape, _faces: &[Vec3], _neutral: &Plane, _pull: Vec3, _angle: f64) -> Result<Shape, BackendError> {
        unsupported("a draft")
    }

    fn split_faces(&self, _body: &Shape, _faces: &[usize], _tool: &SplitTool<'_, Shape>) -> Result<(Shape, Vec<usize>), BackendError> {
        unsupported("a face split")
    }

    fn split_body(&self, _body: &Shape, _tool: &SplitTool<'_, Shape>) -> Result<Vec<Shape>, BackendError> {
        unsupported("a body split")
    }

    fn split_body_by_faces(&self, _body: &Shape, _sheet: &Shape, _face: usize, _faces: &[usize]) -> Result<Vec<Shape>, BackendError> {
        unsupported("a body split")
    }

    fn split_body_by_sheet_where_it_cuts(&self, _body: &Shape, _sheet: &Shape, _face: usize) -> Result<Option<Vec<Shape>>, BackendError> {
        unsupported("a body split")
    }

    fn trim_keep(&self, _body: &Shape, _tool: &SplitTool<'_, Shape>, _keep: TrimKeep<'_>) -> Result<Shape, BackendError> {
        unsupported("a trim")
    }

    fn sweep(&self, _plane: &Plane, _region: &Region2, _path: &[PathSpan], _options: &SweepOptions) -> Result<Shape, BackendError> {
        unsupported("a sweep")
    }

    fn sweep_points(&self, _plane: &Plane, _region: &Region2, _path: &PointPath, _options: &SweepOptions) -> Result<Shape, BackendError> {
        unsupported("a sweep")
    }

    fn sweep_helix(&self, _plane: &Plane, _region: &Region2, _helix: &Helix<'_>) -> Result<Shape, BackendError> {
        unsupported("a helical sweep")
    }

    fn loft(&self, _sections: &[(Plane, Loop2)], _rails: &[Vec<Vec3>], _fit: RailFit) -> Result<Shape, BackendError> {
        unsupported("a loft")
    }

    fn loft_regions(&self, _sections: &[(Plane, Region2)]) -> Result<Shape, BackendError> {
        unsupported("a loft")
    }

    fn loft_to_point(&self, _plane: &Plane, _section: &Loop2, _apex: Vec3, _rails: &[Vec<Vec3>], _fit: RailFit) -> Result<Shape, BackendError> {
        unsupported("a loft to a point")
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
    fn it_takes_the_operations_it_builds_only() {
        for &op in OpKind::ALL {
            let routed = matches!(
                op,
                OpKind::Box
                    | OpKind::Cylinder
                    | OpKind::Sphere
                    | OpKind::Torus
                    | OpKind::Extrude
                    | OpKind::Revolve
                    | OpKind::Boolean
                    | OpKind::Fillet
                    | OpKind::Chamfer
                    | OpKind::Shell
                    | OpKind::Thicken
                    | OpKind::OffsetFaces
            );
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

    /// What both kernels must agree on for one body: its volume and area (relative), its face and
    /// edge counts, the shared check, and the names its faces carry (history and construction
    /// sources, as sorted lists, since the record's face order may differ).
    struct Made {
        volume: f64,
        area: f64,
        faces: usize,
        edges: usize,
        names: Vec<String>,
        sources: Vec<String>,
    }

    fn made(b: &Body) -> Made {
        let m = crate::measure(b).unwrap();
        assert!(matches!(crate::failing_check(b), Ok(None)), "the shared check fails: {:?}", crate::failing_check(b));
        let mut names: Vec<String> = b.history().map(|h| h.faces.iter().map(|f| format!("{f:?}")).collect()).unwrap_or_default();
        names.sort();
        let mut sources: Vec<String> = b.face_sources().map(|s| s.iter().map(|f| format!("{f:?}")).collect()).unwrap_or_default();
        sources.sort();
        Made { volume: m.volume, area: m.area, faces: b.record().faces.len(), edges: b.record().edges.len(), names, sources }
    }

    /// `build` on solidcore and on Open CASCADE: the same bodies, by [`Made`], each within 1e-9 of
    /// the closed-form `volume` where one is given, and Open CASCADE really ran it.
    fn parity(what: &str, volume: Option<f64>, build: impl Fn() -> crate::Result<Vec<Body>>) {
        use crate::route::foreign::{Kernel, fell_back, set_kernel};
        set_kernel(Kernel::Solidcore);
        let ours: Vec<Made> = build().unwrap().iter().map(made).collect();
        set_kernel(Kernel::Occt);
        let before = fell_back();
        let theirs: Vec<Made> = build().unwrap().iter().map(made).collect();
        set_kernel(Kernel::Solidcore);
        assert_eq!(fell_back(), before, "{what}: solidcore ran it");
        assert_eq!(ours.len(), theirs.len(), "{what}: bodies");
        let total: f64 = theirs.iter().map(|m| m.volume).sum();
        if let Some(v) = volume {
            assert!((total - v).abs() <= 1e-9 * v, "{what}: volume {total} against {v}");
        }
        for (a, b) in ours.iter().zip(&theirs) {
            assert!((a.volume - b.volume).abs() <= 1e-9 * a.volume, "{what}: volume {} against {}", b.volume, a.volume);
            assert!((a.area - b.area).abs() <= 1e-9 * a.area, "{what}: area {} against {}", b.area, a.area);
            assert_eq!((a.faces, a.edges), (b.faces, b.edges), "{what}: faces and edges");
            assert_eq!(a.names, b.names, "{what}: face history");
            assert_eq!(a.sources, b.sources, "{what}: construction sources");
        }
    }

    fn poly(pts: &[(f64, f64)]) -> solidcore_record::Loop2 {
        solidcore_record::Loop2::polygon(&pts.iter().map(|&(x, y)| solidcore_record::Vec2::new(x, y)).collect::<Vec<_>>())
    }

    fn region(outer: solidcore_record::Loop2, holes: Vec<solidcore_record::Loop2>) -> Region2 {
        Region2 { outer, holes }
    }

    #[test]
    fn extrusions_match_solidcore_with_the_same_names() {
        use solidcore_record::{Loop2, Seg2, Vec2};
        use std::f64::consts::PI;
        let pl = Plane::XY;
        let rect = region(poly(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)]), vec![]);
        parity("a rectangle", Some(60.0), || crate::route::extrude(&pl, std::slice::from_ref(&rect), 0.0, 5.0));
        parity("a rectangle, downward", Some(36.0), || crate::route::extrude(&pl, std::slice::from_ref(&rect), 1.0, -2.0));
        let holed = region(poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]), vec![Loop2::circle(Vec2::new(5.0, 5.0), 2.0)]);
        parity("a plate with a hole", Some((100.0 - 4.0 * PI) * 3.0), || crate::route::extrude(&pl, std::slice::from_ref(&holed), -1.0, 2.0));
        let disc = region(Loop2::circle(Vec2::new(1.0, 2.0), 3.0), vec![]);
        parity("a disc", Some(9.0 * PI * 4.0), || crate::route::extrude(&pl, std::slice::from_ref(&disc), 0.0, 4.0));
        let two = [rect.clone(), region(poly(&[(10.0, 0.0), (12.0, 0.0), (12.0, 2.0), (10.0, 2.0)]), vec![])];
        parity("two regions", Some(12.0 * 2.0 + 4.0 * 2.0), || crate::route::extrude(&pl, &two, 0.0, 2.0));
        // A side drawn in two pieces: whole sides make it one face, on both kernels.
        let split = region(poly(&[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)]), vec![]);
        let one: &dyn Fn(&Seg2, &Seg2) -> bool = &|_, _| true;
        parity("a side in two pieces, whole", Some(60.0), || crate::route::extrude_whole_sides(&pl, std::slice::from_ref(&split), 0.0, 5.0, one));
        parity("a side in two pieces, as drawn", Some(60.0), || crate::route::extrude(&pl, std::slice::from_ref(&split), 0.0, 5.0));
        // A free-form side in two cubic spans of one curve, and a slanted plane.
        let (a, b, c) = (Vec2::new(4.0, 0.0), Vec2::new(5.0, 1.5), Vec2::new(4.0, 3.0));
        let wavy = region(
            Loop2 {
                segs: vec![
                    Seg2::Line { a: Vec2::new(0.0, 0.0), b: a },
                    Seg2::Cubic { p0: a, p1: Vec2::new(4.5, 0.2), p2: Vec2::new(5.0, 0.8), p3: b },
                    Seg2::Cubic { p0: b, p1: Vec2::new(5.0, 2.2), p2: Vec2::new(4.5, 2.8), p3: c },
                    Seg2::Line { a: c, b: Vec2::new(0.0, 3.0) },
                    Seg2::Line { a: Vec2::new(0.0, 3.0), b: Vec2::new(0.0, 0.0) },
                ],
            },
            vec![],
        );
        let tilted = Plane { origin: Vec3::new(1.0, 2.0, 3.0), x: Vec3::new(0.6, 0.8, 0.0), y: Vec3::new(0.0, 0.0, 1.0) };
        parity("spline spans, whole", None, || crate::route::extrude_whole_sides(&tilted, std::slice::from_ref(&wavy), 0.0, 2.5, one));
        parity("spline spans, as drawn", None, || crate::route::extrude(&tilted, std::slice::from_ref(&wavy), 0.0, 2.5));
    }

    #[test]
    fn revolutions_match_solidcore_with_the_same_names() {
        use solidcore_record::Vec2;
        use std::f64::consts::{PI, TAU};
        let pl = Plane::XZ;
        let y = Vec2::new(0.0, 1.0);
        // A rectangle 2..4 off the axis, 3 high: a tube by Pappus, 2π · 3 · 6.
        let rect = region(poly(&[(2.0, 0.0), (4.0, 0.0), (4.0, 3.0), (2.0, 3.0)]), vec![]);
        parity("a full tube", Some(TAU * 3.0 * 6.0), || crate::route::revolve_whole(&pl, std::slice::from_ref(&rect), Vec2::ZERO, y, TAU));
        parity("three quarters of a tube", Some(0.75 * TAU * 3.0 * 6.0), || {
            crate::route::revolve(&pl, std::slice::from_ref(&rect), Vec2::ZERO, y, 1.5 * PI)
        });
        // A rectangle against the axis: a cylinder.
        let on = region(poly(&[(0.0, 0.0), (2.0, 0.0), (2.0, 5.0), (0.0, 5.0)]), vec![]);
        parity("a cylinder", Some(PI * 4.0 * 5.0), || crate::route::revolve_whole(&pl, std::slice::from_ref(&on), Vec2::ZERO, y, TAU));
        parity("a quarter cylinder", Some(PI * 5.0), || crate::route::revolve(&pl, std::slice::from_ref(&on), Vec2::ZERO, y, PI / 2.0));
    }

    #[test]
    fn a_revolution_that_joins_arcs_runs_on_solidcore() {
        use crate::route::foreign::{Kernel, fell_back, set_kernel};
        use solidcore_record::Vec2;
        let rect = region(poly(&[(2.0, 0.0), (4.0, 0.0), (4.0, 3.0), (2.0, 3.0)]), vec![]);
        set_kernel(Kernel::Occt);
        let before = fell_back();
        let b = crate::route::revolve_joining_arcs(&Plane::XZ, std::slice::from_ref(&rect), Vec2::ZERO, Vec2::new(0.0, 1.0), 1.0).unwrap();
        set_kernel(Kernel::Solidcore);
        assert!(fell_back() > before);
        assert!(b[0].history().is_some());
    }

    /// `build` on both kernels: Open CASCADE ran it, the shared check passes both, and their
    /// volume, area, face and edge counts and centroid agree with each other and with `volume`.
    /// Open CASCADE's results carry no history yet, so names are not compared.
    fn same_solid(what: &str, volume: f64, build: impl Fn() -> crate::Result<Body>) {
        use crate::route::foreign::{Kernel, fell_back, set_kernel};
        let grade = |b: &Body| {
            assert!(matches!(crate::failing_check(b), Ok(None)), "{what}: the shared check fails");
            let m = crate::measure(b).unwrap();
            (m.volume, m.area, b.record().faces.len(), b.record().edges.len(), m.centroid)
        };
        set_kernel(Kernel::Solidcore);
        let ours = grade(&build().unwrap());
        set_kernel(Kernel::Occt);
        let before = fell_back();
        let theirs = grade(&build().unwrap());
        set_kernel(Kernel::Solidcore);
        assert_eq!(fell_back(), before, "{what}: solidcore ran it");
        assert!((theirs.0 - volume).abs() <= 1e-9 * volume, "{what}: volume {} against {volume}", theirs.0);
        assert!((ours.0 - theirs.0).abs() <= 1e-9 * volume, "{what}: volume {} against {}", theirs.0, ours.0);
        assert!((ours.1 - theirs.1).abs() <= 1e-9 * ours.1, "{what}: area {} against {}", theirs.1, ours.1);
        assert_eq!((ours.2, ours.3), (theirs.2, theirs.3), "{what}: faces and edges");
        let apart = ((ours.4.x - theirs.4.x).powi(2) + (ours.4.y - theirs.4.y).powi(2) + (ours.4.z - theirs.4.z).powi(2)).sqrt();
        assert!(apart <= 1e-9 * volume.cbrt(), "{what}: centroid {:?} against {:?}", theirs.4, ours.4);
    }

    #[test]
    fn chamfers_shells_and_thickened_sheets_match_solidcore() {
        let cube = || crate::route::box_solid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        // One edge, 1 back: a prism of section 1/2 along it.
        same_solid("a chamfered edge", 1000.0 - 5.0, || crate::route::chamfer(&cube()?, &[v(5.0, 0.0, 10.0)], 1.0));
        // Open at the top, walls 1 thick: the 8 x 8 x 9 hollow taken out.
        same_solid("a box shelled open at the top", 1000.0 - 576.0, || crate::route::shell(&cube()?, &[v(5.0, 5.0, 10.0)], 1.0));
        let square = Region2 { outer: poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]), holes: vec![] };
        let sheet = || crate::surfaces::patch_region(&Plane::XY, &square);
        same_solid("a sheet thickened up", 200.0, || crate::route::thicken(&sheet()?, 2.0, false));
        same_solid("a sheet thickened down", 200.0, || crate::route::thicken(&sheet()?, -2.0, false));
        same_solid("a sheet thickened both ways", 200.0, || crate::route::thicken(&sheet()?, 2.0, true));
        // The top face up 2 (10 x 10 x 12); the side face at x = 10 in 1 (9 x 10 x 10).
        same_solid("a face moved out", 1200.0, || crate::route::offset_faces(&cube()?, &[v(5.0, 5.0, 10.0)], 2.0));
        same_solid("a face moved in", 900.0, || crate::route::offset_faces(&cube()?, &[v(10.0, 5.0, 5.0)], -1.0));
    }
}
