//! The router's body-making operations ([`solidcraft_kernel_api::Modelling`]).

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{
    Axis2, BoolOp, ChamferSide, DraftCorner, ExtrudeSides, FilletTurn, Helix, Modelling, OpKind, Primitive, RailFit, RevolveSides, Taper, TrimKeep,
};

use crate::{PathSeg, PointPath, SplitTool, SweepControl};

use super::{Which, which};
use crate::Result;
use crate::backend::Solidcore;
use crate::body::Body;

/// The axis-aligned box between two opposite corners.
pub fn box_solid(a: Vec3, b: Vec3) -> Result<Body> {
    primitive(&Primitive::Box { a, b })
}

/// A cylinder standing on `base`, along `axis`, `height` long.
pub fn cylinder(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Result<Body> {
    primitive(&Primitive::Cylinder { base, axis, radius, height })
}

/// A sphere.
pub fn sphere(center: Vec3, radius: f64) -> Result<Body> {
    primitive(&Primitive::Sphere { center, radius })
}

/// A ring torus about the z axis through `center`; needs `major > minor > 0`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
    primitive(&Primitive::Torus { center, major, minor })
}

fn primitive(p: &Primitive) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::of_primitive(p), &[], |k, _| k.primitive(p).map(Some)) {
        let named =
            made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the primitive is empty".into()))).and_then(|b| named_primitive(p, b));
        match named {
            Ok(b) => return Ok(b),
            Err(e) => log::warn!("{}: Open CASCADE's faces don't name as solidcore's do ({e}); solidcore runs it", OpKind::of_primitive(p).name()),
        }
    }
    match which(OpKind::of_primitive(p)) {
        Which::Solidcore => Solidcore.primitive(p),
    }
}

/// Another kernel's primitive with the history and construction sources solidcore gives its own:
/// a cylinder's wall and caps, a sphere's or torus's faces swept from its one profile segment. A
/// box keeps what came back.
#[cfg(feature = "backend-occt")]
fn named_primitive(p: &Primitive, body: Body) -> Result<Body> {
    match *p {
        Primitive::Cylinder { base, axis, radius, height } => {
            let c = crate::build::cylinder_plan(base, axis, radius, height)?;
            crate::smlib_sweeps::name_cylinder(body, &c.plane, c.lo, c.hi, &c.region)
        }
        Primitive::Sphere { .. } | Primitive::Torus { .. } => crate::smlib_sweeps::name_revolved_primitive(body),
        Primitive::Box { .. } => Ok(body),
    }
}

/// `regions` on `plane`, extruded from `start` to `end` along the plane's normal: one body per
/// region that stays apart.
pub fn extrude(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    extruded(plane, regions, start, end, ExtrudeSides::AsDrawn)
}

/// [`extrude`] with each run of profile lines that continue one another in a straight line swept
/// into one side face, and each run of free-form segments that `one_curve` says are pieces of one
/// curve swept as that curve into one wall ([`ExtrudeSides::Whole`]). For the Extrude feature;
/// other builders keep a face per segment.
pub fn extrude_whole_sides(plane: &Plane, regions: &[Region2], start: f64, end: f64, one_curve: &dyn Fn(&Seg2, &Seg2) -> bool) -> Result<Vec<Body>> {
    extruded(plane, regions, start, end, ExtrudeSides::Whole(one_curve))
}

fn extruded(plane: &Plane, regions: &[Region2], start: f64, end: f64, sides: ExtrudeSides<'_>) -> Result<Vec<Body>> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run_each(
        &crate::backend::Occt,
        OpKind::Extrude,
        |k| k.extrude(plane, regions, start, end, sides),
        |bodies| {
            let one_curve = match sides {
                ExtrudeSides::AsDrawn => None,
                ExtrudeSides::Whole(f) => Some(f),
            };
            let mut bodies = bodies.into_iter();
            crate::build::extrusion_plans(plane, regions, start, end, one_curve, |x| {
                let body = bodies.next().ok_or_else(|| crate::KernelError::Failed("fewer solids than regions".into()))?;
                crate::smlib_sweeps::name_extrusion(body, plane, &x.pieces, Some(x.region), x.from, x.distance)
            })
        },
    ) {
        return made;
    }
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude(plane, regions, start, end, sides),
    }
}

/// The material of `regions` on `plane` (overlapping ones merged) extruded from `start` to `end`
/// along its normal, as one body.
pub fn extrude_planar_regions(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude_material(plane, regions, start, end),
    }
}

/// `region` extruded `length` along `dir_sign` × the plane's normal with its sides sloping by
/// `taper` (radians), sharp corners closed as `corner` says.
pub fn extrude_tapered(plane: &Plane, region: &Region2, length: f64, dir_sign: f64, taper: f64, corner: DraftCorner) -> Result<Body> {
    extrude_tapered_between(plane, region, 0.0, length, 0.0, dir_sign, taper, corner)
}

/// [`extrude_tapered`] from `from` to `to`, the region being the section at 0, standing straight
/// for `foot` above `from` before the sides slope ([`Taper`]).
#[allow(clippy::too_many_arguments)]
pub fn extrude_tapered_between(
    plane: &Plane,
    region: &Region2,
    from: f64,
    to: f64,
    foot: f64,
    dir_sign: f64,
    taper: f64,
    corner: DraftCorner,
) -> Result<Body> {
    let taper = Taper { from, to, foot, dir_sign, angle: taper, corner };
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude_tapered(plane, region, &taper),
    }
}

/// `regions` on `plane`, turned by `angle` (radians) about the axis through `axis_origin` along
/// `axis_dir` (plane coordinates).
pub fn revolve(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::AsDrawn)
}

/// [`revolve`], as the whole revolution's form where the angle is a full turn.
pub fn revolve_whole(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, true, RevolveSides::AsDrawn)
}

/// [`revolve`] with contiguous pieces of the same circle joined before turning each profile.
pub fn revolve_joining_arcs(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::JoinArcs)
}

/// [`revolve`] with each run of free-form segments that `one_curve` says are pieces of one curve
/// turned as that curve into one face ([`RevolveSides::JoinFreeForm`]).
pub fn revolve_whole_sides(
    plane: &Plane,
    regions: &[Region2],
    axis_origin: Vec2,
    axis_dir: Vec2,
    angle: f64,
    one_curve: &dyn Fn(&Seg2, &Seg2) -> bool,
) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::JoinFreeForm(one_curve))
}

fn revolved(plane: &Plane, regions: &[Region2], origin: Vec2, dir: Vec2, angle: f64, whole: bool, sides: RevolveSides<'_>) -> Result<Vec<Body>> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run_each(
        &crate::backend::Occt,
        OpKind::Revolve,
        |k| k.revolve(plane, regions, Axis2 { origin, dir }, angle, whole, sides),
        |bodies| {
            let one_curve = match sides {
                RevolveSides::JoinFreeForm(f) => Some(f),
                _ => None,
            };
            let plans = crate::build::revolution_plans_of(plane, regions, origin, dir, angle, one_curve)?;
            if plans.len() != bodies.len() {
                return Err(crate::KernelError::Failed("not one solid per region".into()));
            }
            plans
                .iter()
                .zip(bodies)
                .map(|(x, body)| {
                    let (_, o, k, turn) = crate::smlib_sweeps::revolution_curves(plane, &x.pieces, x.origin, x.axis, x.angle)?;
                    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
                    crate::smlib_sweeps::name_revolution(body, plane, &x.pieces, v(o), v(k), turn)
                })
                .collect()
        },
    ) {
        return made;
    }
    match which(OpKind::Revolve) {
        Which::Solidcore => Solidcore.revolve(plane, regions, Axis2 { origin, dir }, angle, whole, sides),
    }
}

/// Boolean of two bodies. The result may be empty (`Ok(None)`) for a cut that removes
/// everything or an intersection of disjoint bodies. The result carries its exact history
/// ([`Body::history`]) over the operands `[a, b]` where the kernel reports it.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Boolean, &[a, b], |k, f| match f {
        [fa, fb] => k.boolean(fa, fb, op),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a boolean takes two bodies".into())),
    }) {
        return made;
    }
    match which(OpKind::Boolean) {
        Which::Solidcore => Solidcore.boolean(a, b, op),
    }
}

/// Constant-radius fillet of the edges nearest to the given points. A radius too large for the
/// faces around the edges is refused with [`crate::KernelError::TooLarge`] ([`crate::fillet_max`]
/// finds the largest that works).
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Fillet, &[body], |k, f| match f {
        [fb] => k.fillet(fb, edges, radius, FilletTurn::Mitre).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a fillet takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the fillet is empty".into())));
    }
    match which(OpKind::Fillet) {
        Which::Solidcore => Solidcore.fillet(body, edges, radius, FilletTurn::Mitre),
    }
}

/// [`fillet`], its rounds turning at side edges as `turn` says, with the picks whose edges are
/// gone: where some rounds overflow their faces those are built first, and a picked edge one of
/// them removes whole is not rounded; a single concave round running off the flat face it rolls
/// on carries that face on.
pub fn fillet_removing(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<(Body, Vec<Vec3>)> {
    #[cfg(feature = "backend-occt")]
    {
        let mut gone = Vec::new();
        if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Fillet, &[body], |k, f| match f {
            [fb] => k.fillet_removing(fb, edges, radius, turn).map(|(s, g)| {
                gone = g;
                Some(s)
            }),
            _ => Err(solidcraft_kernel_api::BackendError::Invalid("a fillet takes one body".into())),
        }) {
            return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the fillet is empty".into()))).map(|b| (b, gone));
        }
    }
    match which(OpKind::Fillet) {
        Which::Solidcore => Solidcore.fillet_removing(body, edges, radius, turn),
    }
}

/// The fillet of each edge at its own radius (`radii` in `edges`' order), in one blend.
pub fn fillet_each(body: &Body, edges: &[Vec3], radii: &[f64], turn: FilletTurn) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Fillet, &[body], |k, f| match f {
        [fb] => k.fillet_each(fb, edges, radii, turn).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a fillet takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the fillet is empty".into())));
    }
    match which(OpKind::Fillet) {
        Which::Solidcore => Solidcore.fillet_each(body, edges, radii, turn),
    }
}

/// Constant-radius rounds set back from the corners at the vertices nearest each point by its
/// distance, with a patch between the rounds there.
pub fn fillet_setback(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
    match which(OpKind::Fillet) {
        Which::Solidcore => Solidcore.fillet_setback(body, edges, radius, setbacks),
    }
}

/// [`chamfer`], and where a single concave chamfer runs off the flat face it rolls on, that face
/// carried on.
pub fn chamfer_carrying_face(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Chamfer, &[body], |k, f| match f {
        [fb] => k.chamfer_carrying_face(fb, edges, distance).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a chamfer takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the chamfer is empty".into())));
    }
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer_carrying_face(body, edges, distance),
    }
}

/// [`chamfer_sides`], carrying a flat face on as [`chamfer_carrying_face`].
pub fn chamfer_sides_carrying_face(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer_sides_carrying_face(body, edges, d1, side, flip),
    }
}

/// An equal chamfer of the edges nearest to the given points.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Chamfer, &[body], |k, f| match f {
        [fb] => k.chamfer(fb, edges, distance).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("it takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the result is empty".into())));
    }
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer(body, edges, distance),
    }
}

/// A chamfer with different setbacks on the two faces of each edge: `d1` along the first face
/// (the one facing up most, the other with `flip`), the second side by distance or angle.
pub fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer_sides(body, edges, d1, side, flip),
    }
}

/// The body hollowed to `thickness`, open at the faces nearest `open`.
pub fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Shell, &[b], |k, f| match f {
        [fb] => k.shell(fb, open, thickness).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("it takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the result is empty".into())));
    }
    match which(OpKind::Shell) {
        Which::Solidcore => Solidcore.shell(b, open, thickness),
    }
}

/// A sheet thickened to `thickness`, to both sides when `symmetric`.
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Thicken, &[body], |k, f| match f {
        [fb] => k.thicken(fb, thickness, symmetric).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("it takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the result is empty".into())));
    }
    match which(OpKind::Thicken) {
        Which::Solidcore => Solidcore.thicken(body, thickness, symmetric),
    }
}

/// The faces nearest `at` moved `distance` along their normals.
pub fn offset_faces(b: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::OffsetFaces, &[b], |k, f| match f {
        [fb] => k.offset_faces(fb, at, distance).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("it takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the result is empty".into())));
    }
    match which(OpKind::OffsetFaces) {
        Which::Solidcore => Solidcore.offset_faces(b, at, distance),
    }
}

/// Tilt the faces near `faces` by `angle` about their line on the neutral plane, so that they
/// lean in (positive angle) going along `pull`.
pub fn draft(b: &Body, faces: &[Vec3], neutral: &Plane, pull: Vec3, angle: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Draft, &[b], |k, f| match f {
        [fb] => k.draft(fb, faces, neutral, pull, angle).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("it takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the result is empty".into())));
    }
    match which(OpKind::Draft) {
        Which::Solidcore => Solidcore.draft(b, faces, neutral, pull, angle),
    }
}

/// Split the given faces (indices in `body`'s face order) where the tool meets them, with the
/// body face each face of the result came from (in face order). The solid is unchanged; the
/// result's history is over `[body]`.
pub fn split_faces_with_map(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<(Body, Vec<usize>)> {
    match which(OpKind::Split) {
        Which::Solidcore => Solidcore.split_faces(body, faces, tool),
    }
}

/// [`split_faces_with_map`] without the map.
pub fn split_faces(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<Body> {
    split_faces_with_map(body, faces, tool).map(|(b, _)| b)
}

/// Split a body into the closed parts on either side of a tool, those on its back first; a body
/// the tool misses or only nicks comes back whole. Each part's history is over `[body]`.
pub fn split_body(body: &Body, tool: &SplitTool) -> Result<Vec<Body>> {
    #[cfg(feature = "backend-occt")]
    if let SplitTool::Plane(plane) = tool
        && let Some(made) = super::foreign::run_parts(
            &crate::backend::Occt,
            OpKind::Split,
            &[body],
            |k, f| match f {
                [fb] => k.split_body(fb, &solidcraft_kernel_api::SplitTool::Plane(*plane)),
                _ => Err(solidcraft_kernel_api::BackendError::Invalid("a split takes one body".into())),
            },
            |parts| back_then_front(plane, parts),
        )
    {
        return made;
    }
    match which(OpKind::Split) {
        Which::Solidcore => Solidcore.split_body(body, tool),
    }
}

/// Another kernel's pieces of a body split by `plane` in solidcore's order, the one on the
/// plane's back first: refused unless there is one piece on each side (solidcore's whole body
/// where the plane misses or nicks it, and its order of several pieces a side, are its own).
#[cfg(feature = "backend-occt")]
fn back_then_front(plane: &Plane, parts: Vec<Body>) -> std::result::Result<Vec<Body>, solidcraft_kernel_api::BackendError> {
    use solidcraft_geom::PlaneGeom as _;
    use solidcraft_kernel_api::BackendError;
    let mut sided = Vec::with_capacity(parts.len());
    for part in parts {
        let at = crate::measure(&part).map_err(|e| BackendError::Exchange(format!("a piece can't be measured ({e})")))?.centroid;
        sided.push((plane.height(at), part));
    }
    match <[_; 2]>::try_from(sided) {
        Ok([(a, back), (b, front)]) if a < 0.0 && b > 0.0 => Ok(vec![back, front]),
        Ok([(a, front), (b, back)]) if a > 0.0 && b < 0.0 => Ok(vec![back, front]),
        Ok(_) => Err(BackendError::Unsupported("a split that leaves both pieces on one side".into())),
        Err(sided) => Err(BackendError::Unsupported(format!("a split into {} pieces", sided.len()))),
    }
}

/// Split a body with faces of another body exactly as they are, never extended: `faces` (in
/// `sheet`'s face order) and `face` are the tool, `face` gives the sides. Refused unless they cut
/// it into closed solids.
pub fn split_body_by_faces(body: &Body, sheet: &Body, face: usize, faces: &[usize]) -> Result<Vec<Body>> {
    match which(OpKind::Split) {
        Which::Solidcore => Solidcore.split_body_by_faces(body, sheet, face, faces),
    }
}

/// Split a body with every face of the surface body `sheet` as it is, where that cuts it into
/// closed solids (`None` where it doesn't); `face` gives the sides.
pub fn split_body_by_sheet_where_it_cuts(body: &Body, sheet: &Body, face: usize) -> Result<Option<Vec<Body>>> {
    match which(OpKind::Split) {
        Which::Solidcore => Solidcore.split_body_by_sheet_where_it_cuts(body, sheet, face),
    }
}

/// A surface body trimmed by a tool, keeping what `keep` says. Each kept face is the face it is
/// a piece of (its history is over `[body]`).
pub fn trim_keep(body: &Body, tool: &SplitTool, keep: TrimKeep) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let SplitTool::Plane(plane) = tool
        && body.is_surface()
        && let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Split, &[body], |k, f| match f {
            [fb] => k.trim_keep(fb, &solidcraft_kernel_api::SplitTool::Plane(*plane), keep).map(Some),
            _ => Err(solidcraft_kernel_api::BackendError::Invalid("a trim takes one body".into())),
        })
    {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the trim is empty".into())));
    }
    match which(OpKind::Split) {
        Which::Solidcore => Solidcore.trim_keep(body, tool, keep),
    }
}

/// Sweep a planar region along a chain of path segments, keeping the profile perpendicular to
/// the path; at sharp corners the walls meet in mitres.
pub fn sweep(plane: &Plane, region: &Region2, path: &[PathSeg]) -> Result<Body> {
    sweep_with(plane, region, path, &SweepControl::default())
}

/// [`sweep`] with sweep options. The default options are exactly [`sweep`]; a twist, an end
/// scale, guide rails or an orientation direction place the profile at stations along the path.
pub fn sweep_with(plane: &Plane, region: &Region2, path: &[PathSeg], ctl: &SweepControl) -> Result<Body> {
    let (path, options) = (crate::sweep_api::spans(path), crate::sweep_api::options(ctl));
    match which(OpKind::Sweep) {
        Which::Solidcore => Solidcore.sweep(plane, region, &path, &options),
    }
}

/// Sweep a planar region along a path through 3D sketch curves: its splines exactly, its
/// polylines as lines.
pub fn sweep_point_path_with(plane: &Plane, region: &Region2, path: &PointPath, ctl: &SweepControl) -> Result<Body> {
    let (path, options) = (crate::sweep_api::point_path(path), crate::sweep_api::options(ctl));
    match which(OpKind::Sweep) {
        Which::Solidcore => Solidcore.sweep_points(plane, region, &path, &options),
    }
}

/// Sweep a planar region's outer loop along a helix about `axis` through `axis_origin`: `turns`
/// revolutions (right-handed about `axis`), rising `pitch` per turn, the profile keeping its
/// attitude to the axis as a screw motion does.
pub fn sweep_helix(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
    helical(plane, region, &Helix { origin: axis_origin, axis, pitch, turns, law: None, continuous_flanks: false })
}

/// [`sweep_helix`] with the profile moved off the axis as it turns, `offset` piecewise linear
/// through `law`'s (turn, offset) pairs; `continuous_flanks` joins the spline walls across
/// cylinder seams.
#[allow(clippy::too_many_arguments)]
pub fn sweep_helix_radial(
    plane: &Plane,
    region: &Region2,
    axis_origin: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
    continuous_flanks: bool,
) -> Result<Body> {
    helical(plane, region, &Helix { origin: axis_origin, axis, pitch, turns, law: Some(law), continuous_flanks })
}

fn helical(plane: &Plane, region: &Region2, helix: &Helix<'_>) -> Result<Body> {
    match which(OpKind::Sweep) {
        Which::Solidcore => Solidcore.sweep_helix(plane, region, helix),
    }
}

/// A solid through closed planar sections in order.
pub fn loft(sections: &[(Plane, Loop2)]) -> Result<Body> {
    loft_with_rails(sections, &[], RailFit::Scale)
}

/// [`loft`] following 1…16 guide rails (polylines from the first section to the last, through
/// every section's curve), `fit` saying how they reshape the sections between. Without rails,
/// the plain loft.
pub fn loft_with_rails(sections: &[(Plane, Loop2)], rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run_each(
        &crate::backend::Occt,
        OpKind::Loft,
        |k| k.loft(sections, rails, fit).map(|s| vec![s]),
        |bodies| {
            let (pieces, first) = crate::build::loft_plan(sections)?;
            bodies.into_iter().map(|b| crate::smlib_sweeps::name_loft(b, &pieces, first)).collect()
        },
    ) {
        return made.and_then(|mut b| b.pop().ok_or_else(|| crate::KernelError::Failed("the loft is empty".into())));
    }
    match which(OpKind::Loft) {
        Which::Solidcore => Solidcore.loft(sections, rails, fit),
    }
}

/// A loft through regions with matching hole counts: hole walls are built alongside the outline
/// and share the end caps.
pub fn loft_regions(sections: &[(Plane, Region2)]) -> Result<Body> {
    match which(OpKind::Loft) {
        Which::Solidcore => Solidcore.loft_regions(sections),
    }
}

/// A solid from a planar loop of straight segments to an apex point off its plane (a pyramid).
pub fn loft_to_point(plane: &Plane, lp: &Loop2, apex: Vec3) -> Result<Body> {
    loft_to_point_with_rails(plane, lp, apex, &[], RailFit::Scale)
}

/// [`loft_to_point`] following one or two guide `rails` (polylines from the loop to the apex),
/// `fit` saying how they reshape the section on the way. Without rails, the plain loft to a point.
pub fn loft_to_point_with_rails(plane: &Plane, lp: &Loop2, apex: Vec3, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if rails.is_empty()
        && let Some(made) = super::foreign::run_each(
            &crate::backend::Occt,
            OpKind::Loft,
            |k| k.loft_to_point(plane, lp, apex, rails, fit).map(|s| vec![s]),
            |bodies| {
                let pieces = crate::helix::loft_to_point_plan(plane, lp, apex)?;
                bodies.into_iter().map(|b| crate::smlib_sweeps::name_loft_to_point(b, plane, &pieces)).collect()
            },
        )
    {
        return made.and_then(|mut b| b.pop().ok_or_else(|| crate::KernelError::Failed("the loft is empty".into())));
    }
    match which(OpKind::Loft) {
        Which::Solidcore => Solidcore.loft_to_point(plane, lp, apex, rails, fit),
    }
}
