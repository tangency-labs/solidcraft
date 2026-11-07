//! Re-basing a component's frame (`component.set_origin`): what a component holds is written
//! in its own frame, so moving its origin rewrites those coordinates by the inverse of the move.
//! The helpers here make the references a feature has to the component's own origin (its
//! planes, axes and point) into explicit geometry first, so they stay where they are when the
//! origin moves, and map the other component-frame data (joint snaps, imported meshes, Move
//! features) that [`FeatureKind::transform_geometry`] leaves alone.

use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;

use crate::construct::{AxisDef, GeoRef};
use crate::joints::{EdgeAxis, Snap, XRef};
use crate::{AxisRef, Dir, FeatureKind, Mat, MeshData, PlaneRef, apply_point, apply_vector, map_point_expr, mat_mul};

/// The world X, Y or Z axis an origin reference names.
fn origin_axis(axis: &str) -> Option<Vec3> {
    match axis.trim().to_ascii_uppercase().as_str() {
        "X" => Some(Vec3::X),
        "Y" => Some(Vec3::Y),
        "Z" => Some(Vec3::Z),
        _ => None,
    }
}

/// A plane given by the component's origin planes, made explicit (`Custom`) wherever it sits in
/// the reference, and every model reference inside it made explicit as [`explicit_geo`] does.
/// `false` when something in it refers to the origin in a way that can't be written out.
pub fn explicit_plane(p: &mut PlaneRef) -> bool {
    match p {
        PlaneRef::Origin { name } => match Plane::named(name) {
            Some(plane) => {
                *p = PlaneRef::Custom { plane };
                true
            }
            None => false,
        },
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => explicit_plane(base),
        PlaneRef::ThroughPoint { base, point } => explicit_plane(base) & explicit_geo(point),
        PlaneRef::AtAngle { base, line, pivot, .. } => {
            let mut ok = explicit_plane(base);
            for g in [line, pivot].into_iter().flatten() {
                ok &= explicit_geo(g);
            }
            ok
        }
        PlaneRef::Tangent { face, reference, .. } => {
            let mut ok = explicit_geo(face);
            if let Some(r) = reference {
                ok &= explicit_geo(r);
            }
            ok
        }
        PlaneRef::Custom { .. } | PlaneRef::Placed { .. } | PlaneRef::Face { .. } | PlaneRef::Construction { .. } => true,
        PlaneRef::Midplane { .. }
        | PlaneRef::TwoEdges { .. }
        | PlaneRef::AxisPoint { .. }
        | PlaneRef::ThreePoints { .. }
        | PlaneRef::AlongPath { .. }
        | PlaneRef::Perpendicular { .. } => p.refs_mut().into_iter().fold(true, |ok, r| explicit_geo(r) & ok),
    }
}

/// A model reference to the component's origin point made an explicit point, and a plane in it
/// made explicit. `false` for an origin axis (`World`), which only the reference holding it can
/// write out (see [`explicit_origin_refs`]).
pub fn explicit_geo(g: &mut GeoRef) -> bool {
    match g {
        GeoRef::Origin => {
            *g = GeoRef::Point { p: Vec3::ZERO };
            true
        }
        GeoRef::Plane { plane } => explicit_plane(plane),
        GeoRef::World { .. } => false,
        _ => true,
    }
}

/// Every reference `kind` has to its component's origin (origin planes, axes and point) made
/// explicit geometry in the component's current frame, so that mapping the feature's geometry
/// keeps it where it is. `false` when one couldn't be written out (the feature then still
/// refers to the origin, and would follow it).
pub fn explicit_origin_refs(kind: &mut FeatureKind) -> bool {
    let mut ok = true;
    match kind {
        FeatureKind::Revolve { axis, .. } | FeatureKind::SurfaceRevolve { axis, .. } => {
            if let AxisRef::World { axis: name } = axis {
                match origin_axis(name) {
                    Some(d) => *axis = AxisRef::Line { origin: crate::point_expr(Vec3::ZERO), dir: Dir(d) },
                    None => ok = false,
                }
            }
        }
        FeatureKind::ConstructionAxis { def } => {
            if let AxisDef::Edge { edge: GeoRef::World { axis: name } } = def {
                match origin_axis(name) {
                    Some(d) => *def = AxisDef::TwoPoints { a: GeoRef::Point { p: Vec3::ZERO }, b: GeoRef::Point { p: d } },
                    None => ok = false,
                }
            }
        }
        _ => {}
    }
    use crate::feature_geometry::Geo;
    use crate::walk::Walk;
    kind.walk_geometry_mut(&mut |g| match g {
        Geo::Plane(p) => ok &= explicit_plane(p),
        Geo::Construct(r) => ok &= explicit_geo(r),
        _ => {}
    });
    ok
}

/// [`FeatureKind::transform_geometry`]'s plane mapping, for planes held outside a feature (a
/// canvas's).
pub fn transform_plane(m: &Mat, p: &mut PlaneRef) {
    crate::assembly::map_plane_ref(m, p);
}

/// A Move feature (`solid.move`), which turns about its component's origin, re-expressed in a
/// frame `d` maps to: the same motion, `d · M · d⁻¹`. The turn's axis is turned by `d` and the
/// translation becomes `R_d·t + (t_d − R'·t_d)`, where `R'` is the turn in the new frame. Its
/// expressions keep their parameters. `angle` is the turn's current value (radians), `constant`
/// whether it uses no parameters. `false` when it does and the origin moves off the turn's axis,
/// so the translation would have to follow the angle.
pub fn conjugate_move(d: &Mat, translate: &mut [crate::Expr; 3], rotate_axis: &mut Option<Dir>, angle: f64, constant: bool) -> bool {
    let axis = rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z);
    let turned = crate::rigid(Vec3::ZERO, Vec3::ZERO, apply_vector(d, axis), angle);
    let td = Vec3::new(d[3][0], d[3][1], d[3][2]);
    let shift = td - apply_point(&turned, td);
    if shift.len() > 1e-9 && angle.abs() > 1e-12 && !constant {
        return false;
    }
    let mut m = *d;
    m[3] = [shift.x, shift.y, shift.z, 1.0];
    map_point_expr(&m, translate);
    if rotate_axis.is_some() {
        *rotate_axis = Some(Dir(apply_vector(d, axis)));
    } else if angle.abs() > 1e-12 {
        // The default axis is Z: write the turned one out.
        *rotate_axis = Some(Dir(apply_vector(d, Vec3::Z)));
    }
    true
}

/// Imported mesh triangles mapped by the rigid transform `m` (the arrays are copied once).
pub fn transform_meshes(m: &Mat, meshes: &mut [MeshData]) {
    for mesh in meshes {
        let mapped: Vec<f64> = mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|[x, y, z]| {
                let p = apply_point(m, Vec3::new(*x, *y, *z));
                [p.x, p.y, p.z]
            })
            .collect();
        mesh.positions = mapped.into();
    }
}

impl Snap {
    /// The snap with its points and directions (component coordinates) mapped by the rigid `m`.
    pub fn transform(&mut self, m: &Mat) {
        let point = |p: &mut Vec3| *p = apply_point(m, *p);
        let vector = |v: &mut Vec3| *v = apply_vector(m, *v);
        let x_ref = |x: &mut Option<XRef>| match x {
            Some(XRef::Dir(d)) => *d = apply_vector(m, *d),
            Some(XRef::Face { pick: Some(p), .. }) => *p = apply_point(m, *p),
            _ => {}
        };
        match self {
            Snap::Frame { origin, z, x } => {
                point(origin);
                vector(z);
                if let Some(x) = x {
                    vector(x);
                }
            }
            Snap::FaceCenter { pick, x, .. } | Snap::CircleCenter { pick, x, .. } => {
                point(pick);
                x_ref(x);
            }
            Snap::Edge { pick, axis, .. } => {
                point(pick);
                if let Some(EdgeAxis::Along(d) | EdgeAxis::Fixed(d)) = axis {
                    vector(d);
                }
            }
            Snap::Point { point: p, z, x } => {
                point(p);
                for v in [z, x].into_iter().flatten() {
                    vector(v);
                }
            }
        }
    }
}

/// `a · b · c`.
pub fn mat_mul3(a: &Mat, b: &Mat, c: &Mat) -> Mat {
    mat_mul(&mat_mul(a, b), c)
}
