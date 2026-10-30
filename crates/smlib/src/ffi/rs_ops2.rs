//! Offset of faces, draft, chamfers with two setbacks and chord blends on solidcore: the
//! operations of the C shim (`capi/src/smlib_ops.cpp`) that run on `SmBrep::LocalOperation`
//! (a face moved or tilted, the faces around it extended or trimmed to meet its new surface)
//! and on the fillet executive with solvers `rs_blend` does not set up.
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_executive::p2::faceuse_offset;
use solidcore::query::api_queries::face_internal_point;
use solidcore::query::brep::{BoundingBoxOptions, calculate_bounding_box};
use solidcore::surface::surface::Surface;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{StitchAndOrientOptions, stitch_and_orient};
use solidcore::topology::edge::{blend_edgeuse, tolerance as edge_tolerance};
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::edgeuse::p3::{BinormalOptions, evaluate_binormal_uv};
use solidcore::topology::face::p2 as face_ops;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, FaceuseId};
use solidcore::{Result as CoreResult, SolidError};

use super::ops::{ERR_NO_ROOM, ERR_NOT_SOLID, ERR_UNSUPPORTED, PickError};
use super::rs::{State, state, status};
use super::rs_blend::{check_args, pick_edges, pick_faces, working};
use super::rs_ops::result;
use super::rs_shell::notified;
use super::{ERR_ARG, Handle};

fn plain(s: i32) -> PickError {
    (s, None)
}

fn solid_status(e: &SolidError) -> PickError {
    plain(status(e))
}

/// The outer faceuse of `f`: the upward one if its shell bounds the infinite region, else its
/// mate.
fn outer_faceuse(b: &Brep, f: FaceId) -> CoreResult<FaceuseId> {
    let invalid = || SolidError::invalid_input("smlib::outer_faceuse", "face without face use");
    let up = face_ops::upward_faceuse(b, f)?.ok_or_else(invalid)?;
    let shell = eu::faceuse_shell(b, up)?;
    let region = match shell {
        Some(s) => eu::shell_region(b, s)?,
        None => None,
    };
    if shell.is_some() && region == b.infinite_region() { Ok(up) } else { eu::faceuse_mate(b, up)?.ok_or_else(invalid) }
}

/// `r` handed on if `solid_in` says the input was a solid and the result is one again.
pub(super) fn finish(s: &mut State, b: Brep, solid_in: bool) -> Result<Handle, PickError> {
    if solid_in && !is_manifold_solid(&b).map_err(|e| solid_status(&e))? {
        return Err(plain(ERR_NOT_SOLID));
    }
    result(s, b).map_err(plain)
}

/// The distance across `b` along the unit `n`.
fn extent_along(k: &Kernel, b: &mut Brep, n: Vec3) -> CoreResult<f64> {
    let e = calculate_bounding_box(k, b, BoundingBoxOptions::default())?;
    let corner = |i: usize, pick_max: bool| if pick_max { e.max_at(i) } else { e.min_at(i) };
    // the box's corners' projections, the widest span
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for bits in 0..8 {
        let p = Vec3::new(corner(0, bits & 1 != 0), corner(1, bits & 2 != 0), corner(2, bits & 4 != 0));
        let d = p.dot(n);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    Ok(hi - lo)
}

/// `SmBrep::LocalOperation` of `b` under throwaway notification stores.
fn local(s: &mut State, b: &mut Brep, faces: &[FaceId], surfaces: Vec<Surface>) -> Result<(), i32> {
    let kernel = &mut s.kernel;
    notified(b, |b, notes| local_operation(kernel, b, faces, surfaces, notes)).map_err(|e| status(&e))
}

/// The picked faces of `b` moved `distance` along their outward normals: each gets its surface's
/// offset, and the local operation extends or trims the faces around them to meet it.
pub(super) fn offset_faces(s: &mut State, mut b: Brep, domain: u64, points: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Handle, PickError> {
    let picked = pick_faces(&s.kernel, &mut b, domain, points, tolerance)?;
    if !is_manifold_solid(&b).map_err(|e| solid_status(&e))? {
        return Err(plain(ERR_ARG));
    }
    // ponytail: a face moved in by the body's whole depth (or more) along its normal leaves no
    // room; the shim's test is a ball search behind the moved faces. Planar faces only; others
    // are left to the kernel.
    if distance < 0.0 {
        for &f in &picked {
            let surface = face_ops::surface(&b, f).map_err(|e| solid_status(&e))?.ok_or(plain(ERR_ARG))?;
            let normal = match surface.kind() {
                SurfaceKind::Plane(p) => Some(p.evaluate_normal()),
                _ => None,
            };
            if let Some(n) = normal {
                let depth = extent_along(&s.kernel, &mut b, n).map_err(|e| solid_status(&e))?;
                if -distance >= depth - 4.0 * b.tolerance() {
                    return Err(plain(ERR_NO_ROOM));
                }
            }
        }
    }
    let tolerance = b.tolerance();
    let mut surfaces = Vec::new();
    for &f in &picked {
        let mut fu = outer_faceuse(&b, f).map_err(|e| solid_status(&e))?;
        // The offset goes along the faceuse's normal: outward from the outer one, inward from
        // its mate.
        if distance < 0.0 {
            fu = eu::faceuse_mate(&b, fu).map_err(|e| solid_status(&e))?.ok_or(plain(ERR_ARG))?;
        }
        let surface = faceuse_offset(&s.kernel, &b, fu, distance.abs(), tolerance).map_err(|e| solid_status(&e))?;
        surfaces.push(surface.ok_or(plain(ERR_ARG))?);
    }
    local(s, &mut b, &picked, surfaces).map_err(plain)?;
    finish(s, b, true)
}

/// The faces under `points` tilted by `angle` radians about their lines on the neutral plane
/// (through `origin`, normal `normal`), leaning in going along `pull`; the faces around them
/// extended or trimmed to meet the tilted planes. A face must be a plane parallel to the pull.
pub fn draft(
    h: &Handle,
    points: &[[f64; 3]],
    tolerance: f64,
    origin: &[f64; 3],
    normal: &[f64; 3],
    pull: &[f64; 3],
    angle: f64,
) -> Result<Handle, PickError> {
    check_args(points, tolerance, &[angle])?;
    let unit = |v: &[f64; 3]| {
        let u = Vec3::new(v[0], v[1], v[2]);
        let len = u.length();
        (v.iter().all(|c| c.is_finite()) && len > 1e-12 && len.is_finite()).then(|| u / len)
    };
    let (Some(normal), Some(pull)) = (unit(normal), unit(pull)) else { return Err(plain(ERR_ARG)) };
    if points.is_empty() || origin.iter().any(|c| !c.is_finite() || c.abs() > 1e9) || angle == 0.0 || angle.abs() >= 1.5697963267948966 {
        return Err(plain(ERR_ARG));
    }
    let origin = Point3::new(origin[0], origin[1], origin[2]);
    let along = pull.dot(normal);
    if along.abs() < 1e-12 {
        return Err(plain(ERR_ARG));
    }
    let mut s = state();
    let (mut b, domain) = working(&mut s, h)?;
    let picked = pick_faces(&s.kernel, &mut b, domain, points, tolerance)?;
    let solid_in = is_manifold_solid(&b).map_err(|e| solid_status(&e))?;
    let mut surfaces = Vec::new();
    for &f in &picked {
        let fu = outer_faceuse(&b, f).map_err(|e| solid_status(&e))?;
        let surface = face_ops::surface(&b, f).map_err(|e| solid_status(&e))?.ok_or(plain(ERR_ARG))?;
        if let SurfaceKind::Cone(c) = surface.kind()
            && c.is_cylinder()
        {
            let same = eu::faceuse_orientation(&b, fu).map_err(|e| solid_status(&e))? == OrientType::Same;
            surfaces.push(Surface::from(lean_cylinder(&s.kernel, c, same, origin, normal, pull, angle)?));
            continue;
        }
        let SurfaceKind::Plane(plane) = surface.kind() else { return Err(plain(ERR_UNSUPPORTED)) };
        // The face's outward normal: the plane's, turned with the outer faceuse.
        let mut n = plane.evaluate_normal();
        if eu::faceuse_orientation(&b, fu).map_err(|e| solid_status(&e))? != OrientType::Same {
            n *= -1.0;
        }
        if n.dot(pull).abs() > 1e-6 {
            return Err(plain(ERR_UNSUPPORTED));
        }
        let mut tilted = surface.clone();
        let inside = face_internal_point(&s.kernel, &mut b, f).map_err(|e| solid_status(&e))?;
        // The hinge: where a line from a point inside the face, along the pull, meets the neutral plane.
        let hinge = inside + pull * ((origin - inside).dot(normal) / along);
        // Turning n about n x pull by `angle` gives n cos(angle) + pull sin(angle).
        let axis = n.cross(pull);
        let len = axis.length();
        if !(len > 0.5) {
            return Err(plain(ERR_UNSUPPORTED));
        }
        let mut turn = Axis2Placement::IDENTITY;
        turn.rotate_about_axis_at_point(angle, hinge, axis / len);
        tilted.transform(&s.kernel, &turn, None).map_err(|e| solid_status(&e))?;
        surfaces.push(tilted);
    }
    local(&mut s, &mut b, &picked, surfaces).map_err(plain)?;
    finish(&mut s, b, solid_in)
}

/// The outward unit normal, at `u`'s edge's midpoint, of the face the edgeuse `u` bounds, and the
/// unit direction into that face square to the edge.
pub(super) fn face_frame(k: &Kernel, b: &mut Brep, u: EdgeuseId) -> Option<(Vec3, Vec3)> {
    let edge = eu::edge(b, u).ok()??;
    let face = eu::face(b, u).ok()??;
    let fu = outer_faceuse(b, face).ok()?;
    let t = b.edge(edge).ok()?.interval.evaluate(0.5);
    let at = evaluate_binormal_uv(k, b, u, t, false, &BinormalOptions::default()).ok()?.eval;
    let surface = face_ops::surface(b, face).ok()??;
    let n = surface.evaluate_normal(k, at.uv, Side::Right, Side::Right).ok()?;
    let len = n.length();
    if !(len > 0.0) || !len.is_finite() {
        return None;
    }
    let sign = if eu::faceuse_orientation(b, fu).ok()? == OrientType::Same { 1.0 } else { -1.0 };
    Some((n * (sign / len), at.binormal))
}

/// The surface offsets of `edge` (its blend edgeuse's surface first) at which a chamfer `d1` back
/// along the face whose outward normal at the edge's midpoint leans most along `up`, and `second`
/// back along the other (`angle`: the angle in radians the chamfer makes with the first face), is
/// cut. In the section square to the edge, with b0, b1 the directions into the faces and theta
/// the angle between them, setbacks s0, s1 lie at offsets r0 = (s1 - s0 cos theta) / sin theta and
/// r1 = (s0 - s1 cos theta) / sin theta (s tan(theta/2) both, for equal setbacks); an angle phi
/// to the first face gives s1 = s0 sin phi / sin(theta + phi).
fn sides_offsets(k: &Kernel, b: &mut Brep, edge: EdgeId, d1: f64, second: f64, angle: bool, up: Vec3) -> Option<(f64, f64)> {
    let u0 = blend_edgeuse(k, b, edge).ok()??;
    let u1 = eu::radial(b, u0).ok()??;
    let (n0, b0) = face_frame(k, b, u0)?;
    let (n1, b1) = face_frame(k, b, u1)?;
    let c = b0.dot(b1).clamp(-1.0, 1.0);
    let sn = (1.0 - c * c).max(0.0).sqrt();
    if !(sn > 1e-6) {
        return None;
    }
    let (mut s0, mut s1) = (d1, second);
    if angle {
        let d = (c.acos() + second).sin();
        if !(d > 1e-9) {
            return None;
        }
        s1 = d1 * second.sin() / d;
    }
    if n1.dot(up) > n0.dot(up) {
        std::mem::swap(&mut s0, &mut s1);
    }
    let (r0, r1) = ((s1 - s0 * c) / sn, (s0 - s1 * c) / sn);
    (r0.is_finite() && r1.is_finite() && r0 > 0.0 && r1 > 0.0).then_some((r0, r1))
}

/// Chamfers `edges` of `b`, edge i with its first edgeuse's surface offset `first[i]` and the
/// other's `second[i]`: a constant-radius solver with a linear cross-section per edge, bevelled
/// corners, then stitching as `SmApiFilletEdges` does. The executive keeps `SetFilletParameters`'
/// options (no global merge, which leaves a hole rim open).
fn chamfer_offsets(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], first: &[f64], second: &[f64]) -> CoreResult<()> {
    let tolerances = edges.iter().map(|&e| edge_tolerance(b, e)).collect::<CoreResult<Vec<_>>>()?;
    {
        let mut session = FilletSession::new(k, b)?;
        let generator = session.add_surface_generator(FilletSurfaceGenerator::Linear)?;
        let (angle, tangent) = (20f64.to_radians(), 10f64.to_radians());
        for (i, &edge) in edges.iter().enumerate() {
            let edgeuse = session.blend_edgeuse(k, edge)?.ok_or(SolidError::sm_err("smlib chamfer: edge has no blendable sector"))?;
            // SetFilletParameters' tolerance factor
            let approx = 4.0 * tolerances.get(i).copied().unwrap_or(0.0);
            let (r0, r1) = (first.get(i).copied().unwrap_or(0.0), second.get(i).copied().unwrap_or(0.0));
            let solver = session.new_constant_radius_solver_with_second(k, edgeuse, r0, r1, approx, angle, tangent)?;
            session.set_solver_generator(solver, generator)?;
            session.load_solver(k, solver)?;
        }
        session.create_corners(k, None)?;
        for corner in session.fillet_corners()? {
            session.set_corner_bevel(corner, true)?;
        }
        session.do_filleting(k, None)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())
}

/// A chamfer `distance` back along the face whose normal leans most along `first`, and `second`
/// (a distance, or with `angle` the angle in radians to the first face) back along the other.
pub fn chamfer_sides(
    h: &Handle,
    points: &[[f64; 3]],
    tolerance: f64,
    distance: f64,
    second: f64,
    angle: bool,
    first: &[f64; 3],
) -> Result<Handle, PickError> {
    check_args(points, tolerance, &[distance, second])?;
    let up = Vec3::new(first[0], first[1], first[2]);
    let angle_ok = !angle || (second > 1e-6 && second < std::f64::consts::PI - 1e-6);
    if !first.iter().all(|v| v.is_finite()) || !(up.length() > 0.0) || distance <= 0.0 || second <= 0.0 || !angle_ok || points.is_empty() {
        return Err(plain(ERR_ARG));
    }
    let mut s = state();
    let (mut b, _) = working(&mut s, h)?;
    let edges = pick_edges(&s.kernel, &b, points, tolerance)?;
    let solid_in = is_manifold_solid(&b).map_err(|e| solid_status(&e))?;
    let (mut r0, mut r1) = (Vec::new(), Vec::new());
    for &e in &edges {
        let (a, c) = sides_offsets(&s.kernel, &mut b, e, distance, second, angle, up).ok_or(plain(ERR_ARG))?;
        r0.push(a);
        r1.push(c);
    }
    chamfer_offsets(&mut s.kernel, &mut b, &edges, &r0, &r1).map_err(|e| solid_status(&e))?;
    finish(&mut s, b, solid_in)
}

/// A circular blend of constant chord width `sizes[i]` on `edges[i]` (the constant-distance
/// solver with a circular cross-section), then stitching as `SmApiFilletEdgesPerEdge` does.
pub(super) fn chord_edges(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], sizes: &[f64]) -> CoreResult<()> {
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType::Circular;
    {
        let mut session = FilletSession::new(k, b)?;
        session.prepare_per_edge_constant_distance(k, edges, sizes, &vec![Circular; edges.len()], 1, 1.0)?;
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())
}

/// The cylinder `c` (its axis along the pull) as the cone that leans like a drafted plane: its
/// radius at the neutral plane is the cylinder's, and along the pull it changes at `-tan(angle)`
/// for a face whose outward normal points away from the axis (`+` towards it), which turns the
/// outward normal `n` to `n cos(angle) + pull sin(angle)`, as a drafted plane's. The cone keeps the
/// cylinder's parameters ([`solidcore::surface::cone::Cone::lean_cylinder`]).
fn lean_cylinder(
    k: &Kernel,
    c: &solidcore::surface::cone::Cone,
    outer_same: bool,
    neutral_origin: Point3,
    neutral_normal: Vec3,
    pull: Vec3,
    angle: f64,
) -> Result<solidcore::surface::cone::Cone, PickError> {
    let cn = c.canonical();
    let frame = cn.origin;
    let z = frame.x_axis().cross(frame.y_axis());
    let z = z / z.length();
    if z.dot(pull).abs() < 1.0 - 1e-6 {
        return Err(plain(ERR_UNSUPPORTED));
    }
    let across = z.dot(neutral_normal);
    if across.abs() < 1e-12 {
        return Err(plain(ERR_ARG));
    }
    // +1 where the outward normal points away from the axis (the surface's normal does, turned with the faceuse and the cone's flip).
    let sigma = if outer_same != cn.inside_out { 1.0 } else { -1.0 };
    let slope = -sigma * angle.tan() * z.dot(pull);
    let neutral_height = (neutral_origin - frame.origin()).dot(neutral_normal) / across;
    c.lean_cylinder(k, slope, neutral_height).map_err(|e| solid_status(&e))
}
