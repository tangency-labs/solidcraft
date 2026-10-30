//! A solidcore B-rep as flat arrays (`smlib.h`, "Flat B-rep arrays"): regions, shells, faceuses,
//! faces, loops, edgeuse records, edges and vertices in the body's own order, with the analytic
//! geometry of planes, cylinders, cones, spheres and tori and of lines and circles/ellipses.
//! Refused (`SM_ERR_INVALID_INPUT`, nothing returned): NURBS and approximated surfaces and
//! curves, wire and point shells, and bodies whose regions are not yet classified.
//! Not written yet: UV trims, NURBS forms and per-item tolerances; the
//! consumer reads each of them as absent.
use std::collections::HashMap;
use std::f64::consts::PI;

use solidcore::curve::curve::p2::CurveKind;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::Brep;
use solidcore::topology::EdgeuseId;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::edge::other_vertex;
use solidcore::topology::edgeuse::p1::{loopuse_edgeuses, mate, uv_trim_curve};
use solidcore::topology::trimming_tools::{create_uv_trim_curves, face_uv_curves_present};
use solidcore::topology::{LoopuseStart, vertexuse::edge_is_wire};
use solidcore::{EdgeId, FaceId, SolidError, VertexId};

use super::rs::{SM_ERR_INVALID_INPUT, status};
use super::rs_ops2::face_frame;
use crate::flat::*;
use crate::import::Arrays;

/// The refusal for what the arrays cannot carry yet.
const REFUSED: i32 = SM_ERR_INVALID_INPUT;

fn s(e: SolidError) -> i32 {
    status(&e)
}

fn int(n: usize) -> Result<i64, i32> {
    i64::try_from(n).map_err(|_| REFUSED)
}

fn xyz(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn vec3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// `(origin, z axis, x axis)` of a placement: what the frame arrays carry.
fn frame(p: &Axis2Placement) -> [[f64; 3]; 3] {
    [xyz(p.origin()), vec3(p.z_axis()), vec3(p.x_axis())]
}

struct Out(Arrays);

impl Out {
    fn i(&mut self, id: i32, v: i64) {
        self.0.ints.entry(id).or_default().push(v);
    }
    fn r(&mut self, id: i32, v: f64) {
        self.0.reals.entry(id).or_default().push(v);
    }
    fn r3(&mut self, id: i32, v: [f64; 3]) {
        self.0.reals.entry(id).or_default().extend(v);
    }
    fn frame(&mut self, ids: [i32; 3], f: [[f64; 3]; 3]) {
        for (id, v) in ids.into_iter().zip(f) {
            self.r3(id, v);
        }
    }
}

/// Each id's position in `list`.
fn positions<T: Copy + std::hash::Hash + Eq>(list: &[T]) -> HashMap<T, usize> {
    list.iter().enumerate().map(|(i, t)| (*t, i)).collect()
}

pub fn arrays(k: &Kernel, b: &Brep) -> Result<Arrays, i32> {
    // A face's stored domain is not kept up to date on a fresh primitive (a torus: [0, 1]² of a
    // [0, 360]² surface); its UV trims are, so a body without them is exported from a copy that has
    // them made.
    let mut missing = false;
    for &f in b.face_list() {
        missing |= !face_uv_curves_present(b, f).map_err(s)?;
    }
    let with_trims;
    let b = if missing {
        let mut c = b.clone();
        for f in c.face_list().to_vec() {
            create_uv_trim_curves(k, &mut c, f).map_err(s)?;
        }
        with_trims = c;
        &with_trims
    } else {
        b
    };
    let mut o = Out(Arrays::default());
    let faces: Vec<FaceId> = b.face_list().to_vec();
    let face_at = positions(&faces);
    let vertices: Vec<VertexId> = b.vertex_list().to_vec();
    let vertex_at = positions(&vertices);
    let mut edges: Vec<EdgeId> = Vec::new();
    for &e in b.edge_list() {
        if !edge_is_wire(b, e).map_err(s)? {
            edges.push(e);
        }
    }
    let edge_at = positions(&edges);

    // Regions (the infinite one first), their shells and faceuses.
    // Per face: whether the region its same use faces and the region its other use faces hold material.
    let mut material_sides: Vec<[Option<bool>; 2]> = vec![[None; 2]; faces.len()];
    let infinite = b.infinite_region();
    let all = regions(b, None).map_err(s)?;
    if all.first().copied() != infinite {
        return Err(REFUSED);
    }
    for (r, &region) in all.iter().enumerate() {
        let region = b.region(region).map_err(s)?;
        let region_is_material = region.is_void == 0;
        o.i(REGION_SHELL_COUNT, int(region.shells.len())?);
        // is_void: 0 material, 1 void; 2 (unsure) only before the nested-solid classification.
        o.i(
            REGION_TYPE,
            if r == 0 {
                1
            } else {
                match region.is_void {
                    0 => 0,
                    1 => 1,
                    _ => return Err(REFUSED),
                }
            },
        );
        for &sh in &region.shells {
            let shell = b.shell(sh).map_err(s)?;
            if shell.is_wire_shell() || shell.is_vertex_shell() {
                return Err(REFUSED);
            }
            let uses = shell.get_faceuses().map_err(s)?;
            o.i(SHELL_FACEUSE_COUNT, int(uses.len())?);
            o.i(SHELL_WIRE_EDGE_COUNT, 0);
            o.i(SHELL_POINT_TYPE, 0);
            for fu in uses {
                let fu = b.faceuse(fu).map_err(s)?;
                let face = fu.face.and_then(|f| face_at.get(&f)).ok_or(REFUSED)?;
                o.i(FACEUSE_FACE, int(*face)?);
                o.i(FACEUSE_ORIENTATION, i64::from(fu.orientation == OrientType::Same));
                let slot = material_sides.get_mut(*face).ok_or(REFUSED)?;
                *slot.get_mut(usize::from(fu.orientation != OrientType::Same)).ok_or(REFUSED)? = Some(r != 0 && region_is_material);
            }
        }
    }

    // Faces: surface, loops of the Same faceuse, edgeuse records.
    let mut records: Vec<Record> = Vec::new();
    let mut kinds = Kinds;
    for (face_index, &f) in faces.iter().enumerate() {
        let face = b.face(f).map_err(s)?;
        let surface = face.surface.as_ref().ok_or(REFUSED)?;
        let fu = b.faceuse(face.faceuse.ok_or(REFUSED)?).map_err(s)?;
        if fu.orientation != OrientType::Same {
            return Err(REFUSED);
        }
        let analytic = matches!(surface.kind(), SurfaceKind::Plane(_) | SurfaceKind::Cone(_) | SurfaceKind::Sphere(_) | SurfaceKind::Torus(_));
        let domain = if analytic { trimmed_domain(k, b, &fu.loopuses, &face.uv_domain)? } else { face.uv_domain };
        kinds.surface(k, &mut o, surface, &domain)?;
        o.i(FACE_TRIM_TYPE, i64::from(face.rectangular_trim == 1));
        o.i(FACE_LOOP_COUNT, int(fu.loopuses.len())?);
        for &lu in &fu.loopuses {
            match b.loopuse(lu).map_err(s)?.start {
                LoopuseStart::Vertexuse(Some(vu)) => {
                    let v = b.vertexuse(vu).map_err(s)?.vertex.and_then(|v| vertex_at.get(&v)).ok_or(REFUSED)?;
                    o.i(LOOP_EDGEUSE_COUNT, 0);
                    o.i(LOOP_VERTEX, int(*v)?);
                }
                LoopuseStart::Edgeuse(_) => {
                    let uses = loopuse_edgeuses(b, lu).map_err(s)?;
                    o.i(LOOP_EDGEUSE_COUNT, int(uses.len())?);
                    o.i(LOOP_VERTEX, -1);
                    for eu_id in uses {
                        let eu = b.edgeuse(eu_id).map_err(s)?;
                        let e = eu.edge.and_then(|e| edge_at.get(&e)).ok_or(REFUSED)?;
                        o.i(EDGEUSE_EDGE, int(*e)?);
                        o.i(EDGEUSE_ORIENTATION, i64::from(eu.orientation == OrientType::Same));
                        records.push(Record { edge: *e, eu: eu_id, forward: eu.orientation == OrientType::Same, face: face_index });
                        write_trim(k, b, &mut o, eu_id, surface)?;
                    }
                }
                _ => return Err(REFUSED),
            }
        }
    }

    radial_arrays(k, b, &mut o, &edges, &records, &material_sides)?;

    // Edges.
    for &e in &edges {
        let edge = b.edge(e).map_err(s)?;
        let curve = edge.curve.as_ref().ok_or(REFUSED)?;
        let (t0, t1) = (edge.interval.min(), edge.interval.max());
        // The range is in the curve's STEP parameter, which may run against the NURBS's: then
        // it comes out reversed, as the arrays carry "against the curve".
        let mut form = false;
        let (mut a, mut z) = (curve.convert_t_from_nurbs_to_step(k, t0).map_err(s)?, curve.convert_t_from_nurbs_to_step(k, t1).map_err(s)?);
        match curve.kind() {
            CurveKind::Line(l) => {
                let c = l.canonical();
                o.i(EDGE_CURVE_TYPE, i64::from(CURVE_LINE));
                o.r3(EDGE_LINE_ORIGIN, xyz(c.point));
                o.r3(EDGE_LINE_DIRECTION, vec3(c.vector));
            }
            CurveKind::Ellipse(el) => {
                let c = el.canonical();
                let f = frame(&c.position);
                if el.is_circle() {
                    // A circle goes with its NURBS form, its range and trims on that: SMLib's UV trims of the
                    // uses are on the NURBS parameter, not the angle.
                    (a, z) = (t0, t1);
                    form = true;
                    o.i(EDGE_CURVE_TYPE, i64::from(CURVE_CIRCLE));
                    o.frame([EDGE_CIRCLE_CENTER, EDGE_CIRCLE_AXIS, EDGE_CIRCLE_REF_DIRECTION], f);
                    o.r(EDGE_CIRCLE_RADIUS, c.radius_x);
                } else {
                    o.i(EDGE_CURVE_TYPE, i64::from(CURVE_ELLIPSE));
                    o.frame([EDGE_ELLIPSE_CENTER, EDGE_ELLIPSE_AXIS, EDGE_ELLIPSE_REF_DIRECTION], f);
                    o.r(EDGE_ELLIPSE_X_RADIUS, c.radius_x);
                    o.r(EDGE_ELLIPSE_Y_RADIUS, c.radius_y);
                    (a, z) = (t0, t1);
                    form = true;
                }
            }
            CurveKind::BSpline(c) => match straight(k, curve, t0, t1) {
                Some((p, d, length)) => {
                    o.i(EDGE_CURVE_TYPE, i64::from(CURVE_LINE));
                    o.r3(EDGE_LINE_ORIGIN, xyz(p));
                    o.r3(EDGE_LINE_DIRECTION, [d.x, d.y, d.z]);
                    (a, z) = (0.0, length);
                }
                None => nurbs_edge(&mut o, c)?,
            },
            CurveKind::Hyperbola(c) => nurbs_edge(&mut o, c.base())?,
            CurveKind::Parabola(c) => nurbs_edge(&mut o, c.base())?,
            _ => return Err(REFUSED),
        }
        o.i(EDGE_NURBS_FORM, i64::from(form));
        if form && let CurveKind::Ellipse(el) = curve.kind() {
            nurbs_block(&mut o, el.base())?;
        }
        o.r(EDGE_RANGE, a);
        o.r(EDGE_RANGE, z);
        let [start, end] = edge_vertices(b, e)?;
        for v in [start, end] {
            o.i(EDGE_VERTICES, int(*vertex_at.get(&v).ok_or(REFUSED)?)?);
        }
    }

    // Vertices, extent and tolerance.
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    let mut vertex_tolerances = Vec::with_capacity(vertices.len());
    for &v in &vertices {
        let v = b.vertex(v).map_err(s)?;
        let p = xyz(v.point);
        o.r3(VERTEX_POSITION, p);
        o.i(VERTEX_POINT_TYPE, 1);
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
        vertex_tolerances.push(v.tolerance);
    }
    let mut edge_tolerances = Vec::with_capacity(edges.len());
    for &e in &edges {
        edge_tolerances.push(b.edge(e).map_err(s)?.tolerance);
    }
    let mut tolerance = b.tolerance();
    for &f in &faces {
        tolerance = tolerance.max(b.face(f).map_err(s)?.tolerance);
    }
    for t in vertex_tolerances.iter().chain(&edge_tolerances) {
        tolerance = tolerance.max(*t);
    }
    o.r3(EXTENT, lo);
    o.r3(EXTENT, hi);
    // Each edge's and vertex's own tolerance, twice as the arrays carry it (as TOLERANCE below).
    // The STEP writer states the loosest ordinary one as the file's uncertainty.
    for t in &vertex_tolerances {
        o.r(VERTEX_TOLERANCE, 2.0 * t);
    }
    for t in &edge_tolerances {
        o.r(EDGE_TOLERANCE, 2.0 * t);
    }
    // unconfirmed: "twice the largest face, edge and vertex tolerance" over solidcore's zone
    // tolerances (and the body's), as the header words it.
    o.r(TOLERANCE, 2.0 * tolerance);
    o.i(SOURCE, 1);
    Ok(o.0)
}

/// Writes the surface arrays. Geometry goes per kind in order of appearance: the k-th plane face
/// uses entry k of the plane arrays.
struct Kinds;

impl Kinds {
    /// The face's surface kind and geometry, and its range in the surface's STEP parameters.
    fn surface(
        &mut self,
        k: &Kernel,
        o: &mut Out,
        surface: &solidcore::surface::surface::Surface,
        uv: &solidcore::geom::extent2d::Extent2d,
    ) -> Result<(), i32> {
        // Plane distances, angles in radians: the STEP parameters, as the arrays carry them.
        let (mut scale, mut angles) = ([1.0, 1.0], [false, false]);
        // An analytic face goes with the NURBS SMLib holds it as: its range and trims are on that.
        let mut form: Option<&solidcore::nurbs::b_spline_surface::BSplineSurface> = None;
        match surface.kind() {
            SurfaceKind::Plane(p) => {
                o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_PLANE));
                o.frame([FACE_PLANE_ORIGIN, FACE_PLANE_AXIS, FACE_PLANE_REF_DIRECTION], frame(p.position()));
                let su = p.uv_scale();
                scale = [su.x, su.y];
            }
            SurfaceKind::Cone(c) => {
                let cn = c.canonical();
                form = Some(c.rev().base());
                let f = frame(&cn.origin);
                angles = [true, false];
                if c.is_cylinder() || cn.semi_angle_deg == 0.0 {
                    o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_CYLINDER));
                    o.frame([FACE_CYLINDER_ORIGIN, FACE_CYLINDER_AXIS, FACE_CYLINDER_REF_DIRECTION], f);
                    o.r(FACE_CYLINDER_RADIUS, cn.radius);
                } else {
                    o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_CONE));
                    o.frame([FACE_CONE_ORIGIN, FACE_CONE_AXIS, FACE_CONE_REF_DIRECTION], f);
                    // unconfirmed: the radius at the origin (v = 0) and the signed semi-angle as
                    // `canonical` gives them, the angle in radians.
                    o.r(FACE_CONE_RADIUS, cn.radius);
                    o.r(FACE_CONE_SEMI_ANGLE, cn.semi_angle_deg.to_radians());
                }
            }
            SurfaceKind::Sphere(sp) => {
                form = Some(sp.base().base());
                o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_SPHERE));
                o.frame([FACE_SPHERE_CENTER, FACE_SPHERE_AXIS, FACE_SPHERE_REF_DIRECTION], frame(sp.base().position()));
                o.r(FACE_SPHERE_RADIUS, sp.radius());
                angles = [true, true];
            }
            SurfaceKind::Torus(t) => {
                form = Some(t.base().base());
                o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_TORUS));
                o.frame([FACE_TORUS_ORIGIN, FACE_TORUS_AXIS, FACE_TORUS_REF_DIRECTION], frame(t.base().position()));
                o.r(FACE_TORUS_MAJOR_RADIUS, t.major_radius());
                o.r(FACE_TORUS_MINOR_RADIUS, t.minor_radius());
                angles = [true, true];
            }
            SurfaceKind::BSpline(b) => nurbs_face(o, b)?,
            SurfaceKind::SurfOfRevolution(b) => nurbs_face(o, b.base())?,
            SurfaceKind::SurfOfExtrusion(b) => nurbs_face(o, b.base())?,
            _ => return Err(REFUSED),
        }
        o.i(FACE_NURBS_FORM, i64::from(form.is_some()));
        if let Some(n) = form {
            nurbs_face_block(o, n)?;
            // The domain as the trims cover it, but never past the NURBS's own knots.
            let nurb = n.gw_nurb().ok_or(REFUSED)?;
            let span = |knots: &[f64]| (knots.first().copied().unwrap_or(f64::MIN), knots.last().copied().unwrap_or(f64::MAX));
            let ((u0, u1), (v0, v1)) = (span(nurb.u_knots()), span(nurb.v_knots()));
            for x in [uv.u_min().max(u0), uv.v_min().max(v0), uv.u_max().min(u1), uv.v_max().min(v1)] {
                o.r(FACE_RANGE, x);
            }
            return Ok(());
        }
        if matches!(surface.kind(), SurfaceKind::BSpline(_) | SurfaceKind::SurfOfRevolution(_) | SurfaceKind::SurfOfExtrusion(_)) {
            // A NURBS face keeps the NURBS's own parameters: its domain as it is.
            for x in [uv.u_min(), uv.v_min(), uv.u_max(), uv.v_max()] {
                o.r(FACE_RANGE, x);
            }
            return Ok(());
        }
        // The face's domain is in the NURBS parameters: both corners go to the STEP ones.
        // unconfirmed: the range is the box of the two converted corners.
        let (na, nz) = (Point2::new(uv.u_min(), uv.v_min()), Point2::new(uv.u_max(), uv.v_max()));
        let a = torus_step(surface, na, surface.convert_uv_from_nurbs_to_step(k, na).map_err(s)?);
        let z = torus_step(surface, nz, surface.convert_uv_from_nurbs_to_step(k, nz).map_err(s)?);
        let side = |i: usize, a: f64, z: f64| {
            let f = if angles[i] { PI / 180.0 } else { scale[i] };
            let (a, z) = (a * f, z * f);
            (a.min(z), a.max(z))
        };
        let (u0, u1) = side(0, a.x, z.x);
        let (v0, v1) = side(1, a.y, z.y);
        for x in [u0, v0, u1, v1] {
            o.r(FACE_RANGE, x);
        }
        Ok(())
    }
}

/// An edge's start and end vertex along SMLib's direction (the NURBS parameter's): the start is
/// the vertex of the use that runs along the edge (`edge.vertices` is not kept up to date).
fn edge_vertices(b: &Brep, e: EdgeId) -> Result<[VertexId; 2], i32> {
    let primary = *b.edge(e).map_err(s)?.edgeuses.first().ok_or(REFUSED)?;
    let along = if b.edgeuse(primary).map_err(s)?.orientation == OrientType::Same { primary } else { mate(b, primary).map_err(s)?.ok_or(REFUSED)? };
    let vu = b.edgeuse(along).map_err(s)?.vertexuse.ok_or(REFUSED)?;
    let start = b.vertexuse(vu).map_err(s)?.vertex.ok_or(REFUSED)?;
    let end = other_vertex(b, e, Some(start)).map_err(s)?.ok_or(REFUSED)?;
    Ok([start, end])
}

/// A B-spline edge curve: its NURBS block (control points, full knots, weights; always given, 1
/// for a polynomial curve, since the consumer reads them together for every NURBS edge).
fn nurbs_edge(o: &mut Out, c: &solidcore::nurbs::b_spline_curve::BSplineCurve) -> Result<(), i32> {
    o.i(EDGE_CURVE_TYPE, i64::from(CURVE_NURBS));
    nurbs_block(o, c)
}

/// A B-spline edge that is a straight segment at a steady pace (a cylinder's seam): its start, unit
/// direction and length. SMLib's own seam is a line, which the arrays carry as one.
fn straight(k: &Kernel, curve: &solidcore::curve::curve::Curve, t0: f64, t1: f64) -> Option<(Point3, Vec3, f64)> {
    let at = |f: f64| curve.evaluate_point(k, t0 + (t1 - t0) * f).ok();
    let (p0, p1) = (at(0.0)?, at(1.0)?);
    let d = p1 - p0;
    let length = d.length();
    if !(length > 1e-9) {
        return None;
    }
    let unit = d / length;
    let slack = 1e-9 * length.max(1.0);
    [0.125, 0.25, 0.5, 0.75, 0.875]
        .iter()
        .all(|&f| at(f).is_some_and(|p| ((p - p0).dot(unit) - f * length).abs() <= slack && (p - p0 - unit * ((p - p0).dot(unit))).length() <= slack))
        .then_some((p0, unit, length))
}

/// The NURBS block of an edge: a NURBS edge's curve, or a circle's form.
fn nurbs_block(o: &mut Out, c: &solidcore::nurbs::b_spline_curve::BSplineCurve) -> Result<(), i32> {
    let nurb = c.gw_nurb().ok_or(REFUSED)?;
    let (points, mut weights) = c.control_polygon().map_err(s)?;
    if weights.is_empty() {
        weights = vec![1.0; points.len()];
    }
    o.i(EDGE_NURBS_VERTEX_COUNT, int(points.len())?);
    o.i(EDGE_NURBS_ORDER, int(nurb.degree() + 1)?);
    for p in points {
        o.r3(EDGE_NURBS_CVS, xyz(p));
    }
    for &k in nurb.knots() {
        o.r(EDGE_NURBS_KNOTS, k);
    }
    for w in weights {
        o.r(EDGE_NURBS_WEIGHTS, w);
    }
    Ok(())
}

/// A B-spline face: its NURBS block, control points u-major (point (i, j) at `i * v_count + j`).
fn nurbs_face(o: &mut Out, b: &solidcore::nurbs::b_spline_surface::BSplineSurface) -> Result<(), i32> {
    o.i(FACE_SURFACE_TYPE, i64::from(SURFACE_NURBS));
    nurbs_face_block(o, b)
}

/// The NURBS block of a face: a B-spline face's surface, or an analytic face's form.
fn nurbs_face_block(o: &mut Out, b: &solidcore::nurbs::b_spline_surface::BSplineSurface) -> Result<(), i32> {
    let nurb = b.gw_nurb().ok_or(REFUSED)?;
    let net = b.control_point_net().map_err(s)?;
    let n = net.points.len();
    o.i(FACE_NURBS_U_VERTEX_COUNT, int(net.u_count)?);
    o.i(FACE_NURBS_V_VERTEX_COUNT, int(net.v_count)?);
    o.i(FACE_NURBS_U_ORDER, int(nurb.p() + 1)?);
    o.i(FACE_NURBS_V_ORDER, int(nurb.q() + 1)?);
    for p in net.points {
        o.r3(FACE_NURBS_CVS, xyz(p));
    }
    for &k in nurb.u_knots() {
        o.r(FACE_NURBS_U_KNOTS, k);
    }
    for &k in nurb.v_knots() {
        o.r(FACE_NURBS_V_KNOTS, k);
    }
    let weights = if net.weights.is_empty() { vec![1.0; n] } else { net.weights };
    for w in weights {
        o.r(FACE_NURBS_WEIGHTS, w);
    }
    Ok(())
}

/// The face's domain in the NURBS parameters, as its UV trims cover it: a primitive's stored
/// `uv_domain` is not kept to the face (a torus keeps `[0, 1]²` of a `[0, 360]²` surface), so the
/// box of the trim curves' control points says where the face is; with no trims, the stored one.
fn trimmed_domain(k: &Kernel, b: &Brep, loopuses: &[solidcore::topology::LoopuseId], stored: &Extent2d) -> Result<Extent2d, i32> {
    let mut points: Vec<Point3> = Vec::new();
    for &lu in loopuses {
        if !matches!(b.loopuse(lu).map_err(s)?.start, LoopuseStart::Edgeuse(Some(_))) {
            continue;
        }
        for eu in loopuse_edgeuses(b, lu).map_err(s)? {
            let (Some(trim), _) = uv_trim_curve(b, eu).map_err(s)? else { continue };
            match trim.kind() {
                CurveKind::BSpline(c) => points.extend(c.control_polygon().map_err(s)?.0),
                _ => {
                    let ivl = trim.natural_interval();
                    for t in [ivl.min(), ivl.max()] {
                        points.push(trim.evaluate_point(k, t).map_err(s)?);
                    }
                }
            }
        }
    }
    if points.is_empty() {
        return Ok(*stored);
    }
    let fold = |f: fn(f64, f64) -> f64, g: fn(&Point3) -> f64, init: f64| points.iter().map(g).fold(init, f);
    Ok(Extent2d::from_bounds(
        fold(f64::min, |p| p.x, f64::MAX),
        fold(f64::min, |p| p.y, f64::MAX),
        fold(f64::max, |p| p.x, f64::MIN),
        fold(f64::max, |p| p.y, f64::MIN),
    ))
}

/// A torus's NURBS parameters span `[0, 1]` for its whole STEP domain, but its converter hands them
/// back unchanged (a face of `[0, 1]²` would be one degree square). Where it does, the STEP value
/// is the linear image of the parameter over the NURBS and STEP domains.
// unconfirmed: linear, exact only for the whole turn (a partial torus face's NURBS parameter is not
// linear in its angle).
pub(super) fn torus_step(surface: &solidcore::surface::surface::Surface, nurbs: Point2, converted: Point2) -> Point2 {
    let (SurfaceKind::Torus(t), natural) = (surface.kind(), surface.natural_uv_domain_if_built()) else { return converted };
    let step = t.base().step_uv_domain();
    let identity = converted.x == nurbs.x && converted.y == nurbs.y;
    let (lu, lv) = (natural.u_max() - natural.u_min(), natural.v_max() - natural.v_min());
    if !identity || !(lu > 0.0 && lv > 0.0) || (lu == step.u_max() - step.u_min() && lv == step.v_max() - step.v_min()) {
        return converted;
    }
    Point2::new(
        step.u_min() + (nurbs.x - natural.u_min()) / lu * (step.u_max() - step.u_min()),
        step.v_min() + (nurbs.y - natural.v_min()) / lv * (step.v_max() - step.v_min()),
    )
}

/// SMLib's own UV trim of the use (`EDGEUSE_NURBS_*`), where the face is a B-spline surface and the
/// edge a B-spline curve: along the use, its knots on the edge's interval, so the consumer takes
/// it as the use's pcurve instead of fitting one to the edge. SMLib keeps a trim on its own
/// parameter (its edge's, or its mate's, running either way), which is taken only where its lift
/// onto the surface meets the edge within the edge's tolerance at nine parameters along it, in
/// one direction or the other; else the use carries none (count 0) and the consumer fits one.
fn write_trim(
    k: &Kernel,
    b: &Brep,
    o: &mut Out,
    eu: solidcore::topology::EdgeuseId,
    surface: &solidcore::surface::surface::Surface,
) -> Result<(), i32> {
    let none = |o: &mut Out| -> Result<(), i32> {
        o.i(EDGEUSE_NURBS_VERTEX_COUNT, 0);
        o.i(EDGEUSE_NURBS_ORDER, 0);
        Ok(())
    };
    let record = b.edgeuse(eu).map_err(s)?;
    let Some(edge) = record.edge.and_then(|e| b.edge(e).ok()) else { return none(o) };
    let (Some(curve), true) = (
        edge.curve.as_ref(),
        matches!(
            surface.kind(),
            SurfaceKind::BSpline(_)
                | SurfaceKind::SurfOfRevolution(_)
                | SurfaceKind::SurfOfExtrusion(_)
                | SurfaceKind::Plane(_)
                | SurfaceKind::Cone(_)
                | SurfaceKind::Sphere(_)
                | SurfaceKind::Torus(_)
        ),
    ) else {
        return none(o);
    };
    let (Some(trim), _) = uv_trim_curve(b, eu).map_err(s)? else { return none(o) };
    let CurveKind::BSpline(c) = trim.kind() else { return none(o) };
    let Some(nurb) = c.gw_nurb() else { return none(o) };
    // The edge's range as the arrays carry it, which the trim's knots are moved onto: the NURBS
    // interval for a spline and a circle (on its form), the STEP parameter for a line. Other
    // curves have no trim here.
    let (t0, t1) = (edge.interval.min(), edge.interval.max());
    let (w0, w1) = match curve.kind() {
        CurveKind::BSpline(_) => match straight(k, curve, t0, t1) {
            Some((_, _, length)) => (0.0, length),
            None => (t0, t1),
        },
        CurveKind::Ellipse(_) => (t0, t1),
        CurveKind::Line(_) => (curve.convert_t_from_nurbs_to_step(k, t0).map_err(s)?, curve.convert_t_from_nurbs_to_step(k, t1).map_err(s)?),
        _ => return none(o),
    };
    let (points, mut weights) = c.control_polygon().map_err(s)?;
    if weights.is_empty() {
        weights = vec![1.0; points.len()];
    }
    let knots = nurb.knots();
    let (Some(&c0), Some(&c1)) = (knots.first(), knots.last()) else { return none(o) };
    let (same, tol) = (record.orientation == OrientType::Same, edge.tolerance.max(b.tolerance()));
    if !(t1 > t0 && c1 > c0) {
        return none(o);
    }
    for reversed in [false, true] {
        let lifts = (0..=8).all(|i| {
            let p = t0 + (t1 - t0) * f64::from(i) / 8.0;
            let f = (p - t0) / (t1 - t0);
            let at = if reversed { c1 - f * (c1 - c0) } else { c0 + f * (c1 - c0) };
            let along = if same { p } else { t0 + t1 - p };
            match (trim.evaluate_point(k, at), curve.evaluate_point(k, along)) {
                (Ok(uv), Ok(e)) => surface.evaluate_point(k, Point2::new(uv.x, uv.y)).is_ok_and(|q| q.distance(e) <= tol),
                _ => false,
            }
        });
        if !lifts {
            continue;
        }
        let onto = |t: f64| w0 + (t - c0) / (c1 - c0) * (w1 - w0);
        o.i(EDGEUSE_NURBS_VERTEX_COUNT, int(points.len())?);
        o.i(EDGEUSE_NURBS_ORDER, int(nurb.degree() + 1)?);
        let order: Vec<usize> = if reversed { (0..points.len()).rev().collect() } else { (0..points.len()).collect() };
        for &i in &order {
            // A plane's trims are written in its own (u, v), the STEP parameters; the surface's NURBS
            // ones are another affine map of them.
            let p = match surface.kind() {
                SurfaceKind::Plane(pl) => {
                    let q = surface.convert_uv_from_nurbs_to_step(k, Point2::new(points[i].x, points[i].y)).map_err(s)?;
                    let scale = pl.uv_scale();
                    Point2::new(q.x * scale.x, q.y * scale.y)
                }
                _ => Point2::new(points[i].x, points[i].y),
            };
            o.r(EDGEUSE_NURBS_CVS, p.x);
            o.r(EDGEUSE_NURBS_CVS, p.y);
        }
        if reversed {
            knots.iter().rev().for_each(|&t| o.r(EDGEUSE_NURBS_KNOTS, onto(c0 + c1 - t)));
        } else {
            knots.iter().for_each(|&t| o.r(EDGEUSE_NURBS_KNOTS, onto(t)));
        }
        for &i in &order {
            o.r(EDGEUSE_NURBS_WEIGHTS, weights[i]);
        }
        return Ok(());
    }
    none(o)
}

/// An edgeuse record as the arrays list it: its edge (position), its use on the face's `same`
/// faceuse, whether that use runs along the edge, and its face (position).
struct Record {
    edge: usize,
    eu: EdgeuseId,
    forward: bool,
    face: usize,
}

/// Rays closer than this (radians) lie on one ray: faces that touch along the edge.
const SAME_RAY: f64 = 1e-9;

/// The radial arrays: for each edgeuse record the next record round its edge by the right-hand
/// rule about the edge's direction, and whether the traversal enters it from its face's `same`
/// side. A record is the use on the face's `same` faceuse, whose loop runs counter-clockwise about
/// the outward normal `n`, so the face lies to the left of the use, `n x l` for the use's
/// direction `l` (the edge's when the use is forward). The traversal moves along `d x u` at a ray
/// `u` into a face, so it enters from the `same` side exactly when the use runs against the edge.
/// An edge with two records is a cycle of them; a crowded one has its records sorted by the angle
/// of `u` about the edge. Faces that touch (the same ray) cannot be told apart by angle: they are
/// put in the order where the material and the space alternate round the edge.
fn radial_arrays(k: &Kernel, b: &Brep, o: &mut Out, edges: &[EdgeId], records: &[Record], material: &[[Option<bool>; 2]]) -> Result<(), i32> {
    let mut by_edge: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    for (r, rec) in records.iter().enumerate() {
        by_edge.get_mut(rec.edge).ok_or(REFUSED)?.push(r);
    }
    // Whether the sector (after, before) a record (the traversal leaves it from the side it did not
    // enter) is material; none for a face with no material on one side only.
    let side = |r: usize| -> Option<(bool, bool)> {
        let rec = records.get(r)?;
        let [same, other] = *material.get(rec.face)?;
        let (same, other) = (same?, other?);
        let entered_same = !rec.forward;
        (same != other).then_some((same != entered_same, same == entered_same))
    };
    let mut next = vec![0usize; records.len()];
    // The normals are found on a scratch copy (the evaluation takes the body mutably).
    let mut scratch: Option<Brep> = None;
    for (e, list) in by_edge.iter().enumerate() {
        let mut order = list.clone();
        if order.len() > 2 {
            let [start, end] = edge_vertices(b, *edges.get(e).ok_or(REFUSED)?)?;
            let d = b.vertex(end).map_err(s)?.point - b.vertex(start).map_err(s)?.point;
            let d = d / d.length();
            let mut rays: Vec<(usize, Vec3)> = Vec::new();
            for &r in &order {
                let rec = records.get(r).ok_or(REFUSED)?;
                let work = scratch.get_or_insert_with(|| b.clone());
                let (n, _) = face_frame(k, work, rec.eu).ok_or(REFUSED)?;
                rays.push((r, n.cross(if rec.forward { d } else { -d })));
            }
            let first = rays.first().map(|(_, u)| *u).ok_or(REFUSED)?;
            let (e1, e2) = (first, d.cross(first));
            let mut angled: Vec<(f64, usize)> = rays.iter().map(|(r, u)| (u.dot(e2).atan2(u.dot(e1)), *r)).collect();
            angled.sort_by(|a, z| a.0.total_cmp(&z.0));
            // The angle -pi and pi are one ray: bring the first group's twin round to the front.
            while angled.len() > 1 && angled.first().zip(angled.last()).is_some_and(|(a, z)| a.0 + 2.0 * PI - z.0 < SAME_RAY) {
                let Some(last) = angled.pop() else { break };
                angled.insert(0, last);
            }
            let mut groups: Vec<Vec<usize>> = Vec::new();
            let mut at = f64::NAN;
            for (angle, r) in angled {
                let wrapped = (angle - at).abs() < SAME_RAY || (angle - at + 2.0 * PI).abs() < SAME_RAY;
                match groups.last_mut() {
                    Some(g) if wrapped => g.push(r),
                    _ => groups.push(vec![r]),
                }
                at = angle;
            }
            // Within a group of touching faces, each record must start the sector the last one ended.
            let mut done: Vec<usize> = Vec::new();
            for mut g in groups {
                let mut chain: Vec<usize> = Vec::new();
                let mut want = done.last().and_then(|&r| side(r)).map(|(after, _)| after);
                while !g.is_empty() {
                    let pick = g.iter().position(|&r| want.is_none() || side(r).is_none_or(|(_, before)| Some(before) == want)).unwrap_or(0);
                    let r = g.remove(pick);
                    want = side(r).map(|(after, _)| after);
                    chain.push(r);
                }
                done.extend(chain);
            }
            order = done;
        }
        for (i, &r) in order.iter().enumerate() {
            *next.get_mut(r).ok_or(REFUSED)? = *order.get((i + 1) % order.len()).ok_or(REFUSED)?;
        }
    }
    for (r, rec) in records.iter().enumerate() {
        o.i(EDGEUSE_NEXT_RADIAL, int(*next.get(r).ok_or(REFUSED)?)?);
        o.i(EDGEUSE_RADIAL_ENTRY, i64::from(!rec.forward));
    }
    Ok(())
}
