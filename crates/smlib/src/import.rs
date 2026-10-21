//! Schema 4 records (`solidcraft_geom::brep::Brep`) into SMLib: the record is laid out as the C
//! API's flat arrays (`capi/include/smlib.h`, "Flat B-rep arrays"), which `smlib_brep_import`
//! builds into an `SmBrep`.
//!
//! The two models differ in four places, and the mapping takes care of each:
//! - A closed shell is one record listed by the two regions it separates; SMLib keeps one shell
//!   per region, the face used on opposite sides. A sheet's shell holds both sides of its faces.
//! - The infinite region is never a void here; SMLib marks it as one.
//! - Pcurves run along the edge (interval min → max) here; SMLib's UV trims run along their
//!   edgeuse. They are carried on B-spline faces only: on analytic faces SMLib keeps its trims in
//!   its own parameterisation and rebuilds them from the 3D edges.
//! - Every end value (edge intervals, face domains) is already in the flat arrays' units, radians
//!   for angles; the C side converts them to SMLib's own pointwise.

use std::collections::{BTreeMap, HashMap};

use solidcraft_geom::brep as b;

use crate::flat;

/// Why a record cannot go to SMLib.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// The record failed `Brep::validate`.
    Invalid(&'static str),
    /// A part of schema 4 this mapping does not carry yet.
    NotYet(&'static str),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsupported::Invalid(w) => write!(f, "invalid B-rep record: {w}"),
            Unsupported::NotYet(w) => write!(f, "not supported yet: {w} in SMLib"),
        }
    }
}

type R<T> = std::result::Result<T, Unsupported>;

/// The flat arrays of one B-rep, by array id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arrays {
    pub ints: BTreeMap<i32, Vec<i64>>,
    pub reals: BTreeMap<i32, Vec<f64>>,
}

impl Arrays {
    fn i(&mut self, id: i32) -> &mut Vec<i64> {
        self.ints.entry(id).or_default()
    }
    fn r(&mut self, id: i32) -> &mut Vec<f64> {
        self.reals.entry(id).or_default()
    }
    fn r3(&mut self, id: i32, p: [f64; 3]) {
        self.r(id).extend(p);
    }
}

fn int(n: usize) -> R<i64> {
    i64::try_from(n).map_err(|_| Unsupported::Invalid("count"))
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn z_axis(f: &b::Frame) -> [f64; 3] {
    cross(f.x_axis, f.y_axis)
}

/// The record as flat arrays. `trusted`: the record comes from a modeller that keeps SMLib's
/// rules (SMLib then skips its healer); otherwise SMLib heals it on import.
pub fn arrays(brep: &b::Brep, trusted: bool) -> R<Arrays> {
    brep.validate().map_err(|e| Unsupported::Invalid(e.0))?;
    let brep = analytic::analytic(brep)?;
    laid_out(brep.as_ref(), trusted, true)
}

/// [`arrays`] with every surface as the record holds it: a round face truck holds as a rational
/// spline (an extruded circle) stays one, and a plane off unit orthogonal axes goes as its
/// bilinear patch, to compare what SMLib makes of splines with what it makes of its own analytic
/// surfaces. A revolution is still refused (SMLib has no such surface).
pub fn arrays_keeping_splines(brep: &b::Brep, trusted: bool) -> R<Arrays> {
    brep.validate().map_err(|e| Unsupported::Invalid(e.0))?;
    laid_out(brep, trusted, false)
}

/// The flat arrays; `analytic`: a plane on any axes goes as SMLib's plane (else, off unit
/// orthogonal axes, as the bilinear patch it is).
fn laid_out(brep: &b::Brep, trusted: bool, analytic: bool) -> R<Arrays> {
    let mut a = Arrays::default();

    // Regions and shells. Each region gets its own copy of each shell it lists.
    let enclosed_by: HashMap<u32, usize> =
        brep.regions.iter().enumerate().skip(1).filter_map(|(r, region)| region.shells.first().map(|&s| (s, r))).collect();
    let listed = |s: u32| brep.regions.iter().filter(|r| r.shells.contains(&s)).count();
    for (r, region) in brep.regions.iter().enumerate() {
        a.i(flat::REGION_SHELL_COUNT).push(int(region.shells.len())?);
        // SMLib marks the infinite region as a void.
        a.i(flat::REGION_TYPE).push(i64::from(r == 0 || region.void));
        for (k, &s) in region.shells.iter().enumerate() {
            let Some(b::Shell::Faces { faces, wires }) = brep.shells.get(s as usize) else {
                return Err(Unsupported::NotYet("wire and vertex shells"));
            };
            if !wires.is_empty() {
                return Err(Unsupported::NotYet("wire edges embedded in a shell"));
            }
            let uses: Vec<(u32, bool)> = if listed(s) == 2 {
                // A closed shell: the outward normals (out of the material) point into the
                // region that is not material. As the outer shell of region r (k == 0) the shell
                // encloses r; as an inner shell r lies outside it.
                let inside = enclosed_by.get(&s).copied().ok_or(Unsupported::Invalid("closed shell encloses no region"))?;
                let inside_is_void = brep.regions.get(inside).is_some_and(|e| e.void);
                let normals_into_this = if k == 0 && r > 0 { inside_is_void } else { !inside_is_void };
                faces.iter().map(|f| (f.face, f.forward == normals_into_this)).collect()
            } else {
                // An open shell lies in one region and holds both sides of each face.
                faces.iter().flat_map(|f| [(f.face, true), (f.face, false)]).collect()
            };
            a.i(flat::SHELL_FACEUSE_COUNT).push(int(uses.len())?);
            a.i(flat::SHELL_WIRE_EDGE_COUNT).push(0);
            a.i(flat::SHELL_POINT_TYPE).push(0);
            for (face, same) in uses {
                a.i(flat::FACEUSE_FACE).push(i64::from(face));
                a.i(flat::FACEUSE_ORIENTATION).push(i64::from(same));
            }
        }
    }

    // Edgeuses are numbered face → loop → position.
    let mut eu_index: HashMap<(u32, u32, u32), i64> = HashMap::new();
    let mut n = 0i64;
    for (fi, f) in brep.faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            if let b::Loop::Edges { edges } = l {
                for p in 0..edges.len() {
                    eu_index.insert((idx(fi)?, idx(li)?, idx(p)?), n);
                    n += 1;
                }
            }
        }
    }
    let mut next = vec![-1i64; usize::try_from(n).unwrap_or(0)];
    let mut entry = vec![0i64; next.len()];
    for e in &brep.edges {
        let at = |u: &b::EdgeUseRef| eu_index.get(&(u.face, u.r#loop, u.position)).copied().ok_or(Unsupported::Invalid("edge use"));
        for (k, u) in e.uses.iter().enumerate() {
            let here = at(u)?;
            let after = e.uses.get((k + 1) % e.uses.len()).ok_or(Unsupported::Invalid("edge use"))?;
            let slot = usize::try_from(here).map_err(|_| Unsupported::Invalid("edge use"))?;
            *next.get_mut(slot).ok_or(Unsupported::Invalid("edge use"))? = at(after)?;
            *entry.get_mut(slot).ok_or(Unsupported::Invalid("edge use"))? = i64::from(u.upper);
        }
    }

    // Faces, loops and edgeuses, with the surfaces and, on B-spline faces, the trims.
    let any_spline_face = brep.faces.iter().any(|f| matches!(f.surface, b::Surface::Spline { .. } | b::Surface::RationalSpline { .. }));
    for f in &brep.faces {
        a.i(flat::FACE_LOOP_COUNT).push(int(f.loops.len())?);
        a.i(flat::FACE_TRIM_TYPE).push(0);
        let spline = surface(&mut a, &f.surface, f.domain, analytic)?;
        for l in &f.loops {
            match l {
                b::Loop::Vertex { vertex } => {
                    a.i(flat::LOOP_EDGEUSE_COUNT).push(0);
                    a.i(flat::LOOP_VERTEX).push(i64::from(*vertex));
                }
                b::Loop::Edges { edges } => {
                    a.i(flat::LOOP_EDGEUSE_COUNT).push(int(edges.len())?);
                    a.i(flat::LOOP_VERTEX).push(-1);
                    for u in edges {
                        a.i(flat::EDGEUSE_EDGE).push(i64::from(u.edge));
                        a.i(flat::EDGEUSE_ORIENTATION).push(i64::from(u.forward));
                        if any_spline_face {
                            if spline {
                                trim(&mut a, &u.pcurve, u.forward)?;
                            } else {
                                a.i(flat::EDGEUSE_NURBS_VERTEX_COUNT).push(0);
                                a.i(flat::EDGEUSE_NURBS_ORDER).push(0);
                            }
                        }
                    }
                }
            }
        }
    }
    a.i(flat::EDGEUSE_NEXT_RADIAL).extend(next);
    a.i(flat::EDGEUSE_RADIAL_ENTRY).extend(entry);

    for e in &brep.edges {
        curve(&mut a, &e.curve)?;
        a.i(flat::EDGE_VERTICES).extend([i64::from(e.vertices[0]), i64::from(e.vertices[1])]);
        a.r(flat::EDGE_RANGE).extend(e.interval);
    }

    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in &brep.vertices {
        a.i(flat::VERTEX_POINT_TYPE).push(1);
        a.r3(flat::VERTEX_POSITION, v.point);
        for k in 0..3 {
            lo[k] = lo[k].min(v.point[k]);
            hi[k] = hi[k].max(v.point[k]);
        }
    }
    if brep.vertices.is_empty() {
        (lo, hi) = ([0.0; 3], [0.0; 3]);
    }
    a.r(flat::EXTENT).extend(lo.into_iter().chain(hi));
    a.r(flat::TOLERANCE).push(brep.tolerance);
    a.i(flat::SOURCE).push(i64::from(trusted));
    Ok(a)
}

fn idx(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| Unsupported::Invalid("index"))
}

/// A NURBS net's control points and weights, cartesian, U-major.
fn net(a: &mut Arrays, rows: impl Iterator<Item = ([f64; 3], f64)>) {
    for (p, w) in rows {
        a.r3(flat::FACE_NURBS_CVS, p);
        a.r(flat::FACE_NURBS_WEIGHTS).push(w);
    }
}

fn nurbs_face(a: &mut Arrays, u_knots: &[f64], v_knots: &[f64], nu: usize, nv: usize) -> R<()> {
    let order = |k: &[f64], n: usize| k.len().checked_sub(n).filter(|o| *o >= 2).ok_or(Unsupported::Invalid("spline order"));
    a.i(flat::FACE_SURFACE_TYPE).push(i64::from(flat::SURFACE_NURBS));
    a.i(flat::FACE_NURBS_U_VERTEX_COUNT).push(int(nu)?);
    a.i(flat::FACE_NURBS_V_VERTEX_COUNT).push(int(nv)?);
    a.i(flat::FACE_NURBS_U_ORDER).push(int(order(u_knots, nu)?)?);
    a.i(flat::FACE_NURBS_V_ORDER).push(int(order(v_knots, nv)?)?);
    a.r(flat::FACE_NURBS_U_KNOTS).extend_from_slice(u_knots);
    a.r(flat::FACE_NURBS_V_KNOTS).extend_from_slice(v_knots);
    Ok(())
}

/// The face's surface and range; true for a B-spline surface (which carries trims).
fn surface(a: &mut Arrays, s: &b::Surface, domain: [[f64; 2]; 2], analytic_planes: bool) -> R<bool> {
    let [[u0, u1], [v0, v1]] = domain;
    let analytic = |a: &mut Arrays, kind: u8, ids: [i32; 3], f: &b::Frame| {
        a.i(flat::FACE_SURFACE_TYPE).push(i64::from(kind));
        a.r3(ids[0], f.origin);
        a.r3(ids[1], z_axis(f));
        a.r3(ids[2], f.x_axis);
        a.r(flat::FACE_RANGE).extend([u0, v0, u1, v1]);
    };
    match s {
        b::Surface::Plane { origin, u_axis, v_axis } => plane(a, *origin, *u_axis, *v_axis, domain, analytic_planes)?,
        b::Surface::PlaneThrough { origin, u_point, v_point } => {
            plane(a, *origin, sub(*u_point, *origin), sub(*v_point, *origin), domain, analytic_planes)?
        }
        b::Surface::Cylinder { frame, radius } => {
            analytic(a, flat::SURFACE_CYLINDER, [flat::FACE_CYLINDER_ORIGIN, flat::FACE_CYLINDER_AXIS, flat::FACE_CYLINDER_REF_DIRECTION], frame);
            a.r(flat::FACE_CYLINDER_RADIUS).push(*radius);
        }
        b::Surface::Cone { frame, radius_at_origin, half_angle } => {
            analytic(a, flat::SURFACE_CONE, [flat::FACE_CONE_ORIGIN, flat::FACE_CONE_AXIS, flat::FACE_CONE_REF_DIRECTION], frame);
            a.r(flat::FACE_CONE_RADIUS).push(*radius_at_origin);
            a.r(flat::FACE_CONE_SEMI_ANGLE).push(*half_angle);
        }
        b::Surface::Sphere { frame, radius } => {
            analytic(a, flat::SURFACE_SPHERE, [flat::FACE_SPHERE_CENTER, flat::FACE_SPHERE_AXIS, flat::FACE_SPHERE_REF_DIRECTION], frame);
            a.r(flat::FACE_SPHERE_RADIUS).push(*radius);
        }
        b::Surface::Torus { frame, major_radius, minor_radius } => {
            analytic(a, flat::SURFACE_TORUS, [flat::FACE_TORUS_ORIGIN, flat::FACE_TORUS_AXIS, flat::FACE_TORUS_REF_DIRECTION], frame);
            a.r(flat::FACE_TORUS_MAJOR_RADIUS).push(*major_radius);
            a.r(flat::FACE_TORUS_MINOR_RADIUS).push(*minor_radius);
        }
        b::Surface::Spline { u_knots, v_knots, points } => {
            let nv = points.first().map_or(0, Vec::len);
            nurbs_face(a, u_knots, v_knots, points.len(), nv)?;
            net(a, points.iter().flatten().map(|p| (*p, 1.0)));
            a.r(flat::FACE_RANGE).extend([u0, v0, u1, v1]);
            return Ok(true);
        }
        b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            let nv = homogeneous.first().map_or(0, Vec::len);
            nurbs_face(a, u_knots, v_knots, homogeneous.len(), nv)?;
            net(a, homogeneous.iter().flatten().map(|h| ([h[0] / h[3], h[1] / h[3], h[2] / h[3]], h[3])));
            a.r(flat::FACE_RANGE).extend([u0, v0, u1, v1]);
            return Ok(true);
        }
        b::Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => revolution(a, profile, *origin, *axis, matrix, *forward, domain)?,
    }
    Ok(false)
}

/// The quarter-circle arc's rational parameter `s` in [0, 1] at angle `phi` in [0, π/2]: the
/// circle NURBS below is not linear in the angle, so end values are converted pointwise.
fn quarter_param(phi: f64) -> f64 {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let angle = |s: f64| {
        let (b0, b1, b2) = ((1.0 - s) * (1.0 - s), 2.0 * s * (1.0 - s) * c, s * s);
        (b1 + b2).atan2(b0 + b1)
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        if angle(mid) < phi {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The circle NURBS's parameter (knots 0..4, one quarter turn per span) at `angle` after its start.
fn circle_param(angle: f64) -> f64 {
    let q = std::f64::consts::FRAC_PI_2;
    let a = angle.clamp(0.0, 4.0 * q);
    let k = (a / q).floor().min(3.0);
    k + quarter_param(a - k * q)
}

/// A revolution, exactly, as a rational B-spline surface: each profile control point swept round
/// the axis by a full circle of four rational quadratic quarters starting at the face's first
/// angle, then mapped by the placement. The circle's parameter is not the angle, so the face's
/// angle range is converted pointwise and its pcurves are not carried (SMLib rebuilds the trims).
/// Where the placement mirrors, the record's natural normal is the opposite of the map's
/// (`brep::eval`), so the B-spline takes the two directions the other way round.
fn revolution(
    a: &mut Arrays,
    profile: &b::Curve,
    origin: [f64; 3],
    axis: [f64; 3],
    m: &[[f64; 4]; 4],
    forward: bool,
    domain: [[f64; 2]; 2],
) -> R<()> {
    let (knots, pts): (Vec<f64>, Vec<([f64; 3], f64)>) = match profile {
        b::Curve::Line { start, end } => (vec![0.0, 0.0, 1.0, 1.0], vec![(*start, 1.0), (*end, 1.0)]),
        b::Curve::Spline { knots, points } => (knots.clone(), points.iter().map(|p| (*p, 1.0)).collect()),
        b::Curve::RationalSpline { knots, homogeneous } => {
            (knots.clone(), homogeneous.iter().map(|h| ([h[0] / h[3], h[1] / h[3], h[2] / h[3]], h[3])).collect())
        }
        _ => return Err(Unsupported::NotYet("revolutions of conic, polyline or intersection profiles")),
    };
    let len = dot(axis, axis).sqrt();
    if !(len > 0.0) {
        return Err(Unsupported::Invalid("revolution axis"));
    }
    let z = scale(axis, 1.0 / len);
    // The record's u and v: the profile's parameter and the angle, or the other way round.
    let ([p0, p1], [a0, a1]) = if forward { (domain[0], domain[1]) } else { (domain[1], domain[0]) };
    let t1 = circle_param(a1 - a0);
    // Column-major placement: point' = M · (p, 1).
    let place = |p: [f64; 3]| [0, 1, 2].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
    let det = {
        let col = |c: usize| [m[c][0], m[c][1], m[c][2]];
        dot(cross(col(0), col(1)), col(2))
    };
    let c = std::f64::consts::FRAC_1_SQRT_2;
    // net[i][k]: profile point i at circle control point k.
    let mut net: Vec<Vec<([f64; 3], f64)>> = Vec::with_capacity(pts.len());
    for (p, w) in &pts {
        let r = sub(*p, origin);
        let centre = add(origin, scale(z, dot(r, z)));
        let x0 = sub(*p, centre);
        let y0 = cross(z, x0);
        // Start at the face's first angle.
        let (s0, c0) = a0.sin_cos();
        let (x, y) = (add(scale(x0, c0), scale(y0, s0)), sub(scale(y0, c0), scale(x0, s0)));
        let row = (0..9)
            .map(|k| {
                let (sk, ck) = (k as f64 * std::f64::consts::FRAC_PI_4).sin_cos();
                let (f, wk) = if k % 2 == 1 { (1.0 / c, c) } else { (1.0, 1.0) };
                (place(add(centre, scale(add(scale(x, ck), scale(y, sk)), f))), w * wk)
            })
            .collect();
        net.push(row);
    }
    let circle_knots = [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
    // Profile along the B-spline's first direction when the record's u is the profile and the
    // placement keeps orientation, or the angle is u and it mirrors; else the angle goes first.
    let profile_first = forward == (det > 0.0);
    if profile_first {
        nurbs_face(a, &knots, &circle_knots, pts.len(), 9)?;
        net_rows(a, net.iter().flatten().copied());
        a.r(flat::FACE_RANGE).extend([p0, 0.0, p1, t1]);
    } else {
        nurbs_face(a, &circle_knots, &knots, 9, pts.len())?;
        net_rows(a, (0..9).flat_map(|k| net.iter().filter_map(move |row| row.get(k).copied())));
        a.r(flat::FACE_RANGE).extend([0.0, p0, t1, p1]);
    }
    Ok(())
}

fn net_rows(a: &mut Arrays, cps: impl Iterator<Item = ([f64; 3], f64)>) {
    net(a, cps);
}

/// A plane `origin + u·U + v·V` as SMLib's plane, whose (u, v) are distances along unit,
/// orthogonal axes: U's direction, and the one at a right angle to it in the plane, the normal
/// U × V as the record's. The range is the record's domain mapped onto those axes (its bounding
/// box: a face's range need not be tight, and SMLib rebuilds an analytic face's trims from its
/// edges). Truck's planes run through three points, their axes the face's own sides.
/// Off unit orthogonal axes and not `analytic`, it goes as the exact bilinear patch over the
/// domain instead.
fn plane(a: &mut Arrays, origin: [f64; 3], u: [f64; 3], v: [f64; 3], domain: [[f64; 2]; 2], analytic: bool) -> R<()> {
    let [[u0, u1], [v0, v1]] = domain;
    let unit = |x: [f64; 3]| (dot(x, x).sqrt() - 1.0).abs() <= 1e-12;
    if !analytic && !(unit(u) && unit(v) && dot(u, v).abs() <= 1e-12) {
        nurbs_face(a, &[u0, u0, u1, u1], &[v0, v0, v1, v1], 2, 2)?;
        let at = |s: f64, t: f64| add(origin, add(scale(u, s), scale(v, t)));
        net(a, [at(u0, v0), at(u0, v1), at(u1, v0), at(u1, v1)].into_iter().map(|p| (p, 1.0)));
        a.r(flat::FACE_RANGE).extend([u0, v0, u1, v1]);
        return Ok(());
    }
    let n = cross(u, v);
    let (lu, ln) = (dot(u, u).sqrt(), dot(n, n).sqrt());
    if !(lu > 0.0 && ln > 0.0 && lu.is_finite() && ln.is_finite()) {
        return Err(Unsupported::Invalid("plane axes"));
    }
    let (x, z) = (scale(u, 1.0 / lu), scale(n, 1.0 / ln));
    let y = cross(z, x);
    let corners = [[u0, v0], [u0, v1], [u1, v0], [u1, v1]].map(|[s, t]| {
        let d = add(scale(u, s), scale(v, t));
        [dot(d, x), dot(d, y)]
    });
    let lo = |k: usize| corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min);
    let hi = |k: usize| corners.iter().map(|c| c[k]).fold(f64::NEG_INFINITY, f64::max);
    a.i(flat::FACE_SURFACE_TYPE).push(i64::from(flat::SURFACE_PLANE));
    a.r3(flat::FACE_PLANE_ORIGIN, origin);
    a.r3(flat::FACE_PLANE_AXIS, z);
    a.r3(flat::FACE_PLANE_REF_DIRECTION, x);
    a.r(flat::FACE_RANGE).extend([lo(0), lo(1), hi(0), hi(1)]);
    Ok(())
}

/// A pcurve as the edgeuse's UV trim: reversed when the loop runs against the edge.
fn trim(a: &mut Arrays, c: &b::Curve2, forward: bool) -> R<()> {
    let (knots, mut pts): (&[f64], Vec<([f64; 2], f64)>) = match c {
        b::Curve2::Spline { knots, points } => (knots, points.iter().map(|p| (*p, 1.0)).collect()),
        b::Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.iter().map(|h| ([h[0] / h[2], h[1] / h[2]], h[2])).collect()),
    };
    let order = knots.len().checked_sub(pts.len()).filter(|o| *o >= 2).ok_or(Unsupported::Invalid("pcurve order"))?;
    let mut k = knots.to_vec();
    if !forward {
        pts.reverse();
        k = k.iter().rev().map(|t| -t).collect();
    }
    a.i(flat::EDGEUSE_NURBS_VERTEX_COUNT).push(int(pts.len())?);
    a.i(flat::EDGEUSE_NURBS_ORDER).push(int(order)?);
    for (p, w) in pts {
        a.r(flat::EDGEUSE_NURBS_CVS).extend(p);
        a.r(flat::EDGEUSE_NURBS_WEIGHTS).push(w);
    }
    a.r(flat::EDGEUSE_NURBS_KNOTS).extend(k);
    Ok(())
}

fn nurbs_curve(a: &mut Arrays, knots: &[f64], pts: impl ExactSizeIterator<Item = ([f64; 3], f64)>) -> R<()> {
    let n = pts.len();
    let order = knots.len().checked_sub(n).filter(|o| *o >= 2).ok_or(Unsupported::Invalid("spline order"))?;
    a.i(flat::EDGE_CURVE_TYPE).push(i64::from(flat::CURVE_NURBS));
    a.i(flat::EDGE_NURBS_VERTEX_COUNT).push(int(n)?);
    a.i(flat::EDGE_NURBS_ORDER).push(int(order)?);
    for (p, w) in pts {
        a.r3(flat::EDGE_NURBS_CVS, p);
        a.r(flat::EDGE_NURBS_WEIGHTS).push(w);
    }
    a.r(flat::EDGE_NURBS_KNOTS).extend_from_slice(knots);
    Ok(())
}

fn curve(a: &mut Arrays, c: &b::Curve) -> R<()> {
    match c {
        b::Curve::Line { start, end } => {
            a.i(flat::EDGE_CURVE_TYPE).push(i64::from(flat::CURVE_LINE));
            a.r3(flat::EDGE_LINE_ORIGIN, *start);
            a.r3(flat::EDGE_LINE_DIRECTION, sub(*end, *start));
        }
        b::Curve::Circle { frame, radius } => {
            a.i(flat::EDGE_CURVE_TYPE).push(i64::from(flat::CURVE_CIRCLE));
            a.r3(flat::EDGE_CIRCLE_CENTER, frame.origin);
            a.r3(flat::EDGE_CIRCLE_AXIS, z_axis(frame));
            a.r3(flat::EDGE_CIRCLE_REF_DIRECTION, frame.x_axis);
            a.r(flat::EDGE_CIRCLE_RADIUS).push(*radius);
        }
        b::Curve::Ellipse { frame, radii } => {
            a.i(flat::EDGE_CURVE_TYPE).push(i64::from(flat::CURVE_ELLIPSE));
            a.r3(flat::EDGE_ELLIPSE_CENTER, frame.origin);
            a.r3(flat::EDGE_ELLIPSE_AXIS, z_axis(frame));
            a.r3(flat::EDGE_ELLIPSE_REF_DIRECTION, frame.x_axis);
            a.r(flat::EDGE_ELLIPSE_X_RADIUS).push(radii[0]);
            a.r(flat::EDGE_ELLIPSE_Y_RADIUS).push(radii[1]);
        }
        b::Curve::Polyline { points } => {
            // Segment i spans [i, i + 1]: a degree-1 B-spline with those knots.
            let n = points.len();
            let mut k = vec![0.0];
            k.extend((0..n).map(|i| i as f64));
            k.push(n.saturating_sub(1) as f64);
            nurbs_curve(a, &k, points.iter().map(|p| (*p, 1.0)))?;
        }
        b::Curve::Spline { knots, points } => nurbs_curve(a, knots, points.iter().map(|p| (*p, 1.0)))?,
        b::Curve::RationalSpline { knots, homogeneous } => {
            nurbs_curve(a, knots, homogeneous.iter().map(|h| ([h[0] / h[3], h[1] / h[3], h[2] / h[3]], h[3])))?
        }
        b::Curve::SurfaceIntersection { .. } => return Err(Unsupported::NotYet("traced intersection curves")),
    }
    Ok(())
}

mod analytic;

#[cfg(test)]
mod tests;

impl crate::Brep {
    /// The SMLib B-rep of a schema 4 record. `trusted` as in [`arrays`].
    pub fn from_record(brep: &b::Brep, trusted: bool) -> crate::Result<crate::Brep> {
        let a = arrays(brep, trusted).map_err(crate::Error::Record)?;
        crate::flat::Flat::from_arrays(&a)?.import()
    }

    /// [`crate::Brep::from_record`] through [`arrays_keeping_splines`].
    pub fn from_record_keeping_splines(brep: &b::Brep, trusted: bool) -> crate::Result<crate::Brep> {
        let a = arrays_keeping_splines(brep, trusted).map_err(crate::Error::Record)?;
        crate::flat::Flat::from_arrays(&a)?.import()
    }
}
