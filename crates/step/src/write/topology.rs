//! A schema 4 record ([`solidcraft_geom::brep`]) as ISO 10303-42 topology, the inverse of
//! [`crate::topology`]. Each bounded material region becomes a MANIFOLD_SOLID_BREP, or a
//! BREP_WITH_VOIDS when voids lie inside it (each void's CLOSED_SHELL through an
//! ORIENTED_CLOSED_SHELL); the open shells become one SHELL_BASED_SURFACE_MODEL. Faces are ADVANCED_FACEs on their
//! exact surface, bounded by a FACE_OUTER_BOUND (the loop that runs counter-clockwise in (u, v))
//! and FACE_BOUNDs of EDGE_LOOPs of ORIENTED_EDGEs, or of VERTEX_LOOPs. Each EDGE_CURVE runs on
//! its exact curve from the record edge's start vertex (interval min) to its end (max), its
//! same_sense .T.; an edge one or two faces use is a SURFACE_CURVE (SEAM_CURVE when one face
//! uses it twice) carrying the PCURVEs of its uses: the record's exact pcurves, in the STEP
//! surface's parameters. Nothing is approximated: what STEP cannot state exactly is refused
//! ("not supported yet: …").
//!
//! Parameters: every record curve keeps its parameter in STEP (a line's runs from its start to
//! its end as the record's does). A record surface's (u, v) is a linear image of its STEP
//! surface's ([`Map`]): a plane is written on the orthonormal frame of its axes, a cone that
//! narrows along its axis (negative half angle) with its axis turned over, a revolution with the
//! angle first; pcurves go through the same map. Where the map reverses orientation, the
//! record's natural normal is STEP's reversed, and the faces' same_sense says so.
//!
//! Sides: every face's normal points out of the material, as the record's shells have them (a
//! void's faces into the void, its ORIENTED_CLOSED_SHELL orientation .T.). A face is written with
//! the side its first use needs (its same_sense, and its loops run counter-clockwise about that
//! side's normal); a later use of the other side refers to it through an ORIENTED_FACE.

use std::sync::Arc;

use solidcraft_geom::brep::{Brep, Curve as BCurve, Curve2, Edge, Face, Frame, Loop, Shell, Surface as BSurface};

use super::{GeomWriter, boolean, list, r, s};
use crate::geom::{
    Associated, Axis1, BSplineCurve, BSplineSurface, Curve, CurveForm, KnotSpec, Logical, P3, Pcurve, Placement, PreferredRep, Surface,
    SurfaceCurveKind, SurfaceForm, cross, dot, norm, scale, sub,
};
use crate::p21::Param;
use crate::read::R;

/// Points of each use's pcurve the loops' (u, v) areas are taken over.
const AREA_SAMPLES: usize = 16;
/// How far (relative to its scales) a revolution's placement may be from one that keeps it a
/// revolution, or from a similarity.
const SIMILAR: f64 = 1e-9;

fn unsupported(what: impl std::fmt::Display) -> String {
    format!("not supported yet: STEP export of {what}")
}

/// The record's surface parameters as the STEP surface's: `(u', v') = m · (u, v)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Map {
    pub m: [[f64; 2]; 2],
}

impl Map {
    pub const IDENTITY: Map = Map { m: [[1.0, 0.0], [0.0, 1.0]] };

    pub fn apply(&self, [u, v]: [f64; 2]) -> [f64; 2] {
        let [a, b] = self.m;
        [a[0] * u + a[1] * v, b[0] * u + b[1] * v]
    }

    /// The record's natural normal is the STEP surface's (both are ∂S/∂u × ∂S/∂v, so they differ
    /// by the sign of the map's determinant).
    pub fn keeps_normal(&self) -> bool {
        let [a, b] = self.m;
        a[0] * b[1] - a[1] * b[0] > 0.0
    }
}

fn placement(f: &Frame) -> Placement {
    Placement { origin: f.origin, axis: cross(f.x_axis, f.y_axis), ref_direction: f.x_axis }
}

fn unit(a: P3, what: &str) -> R<P3> {
    let n = norm(a);
    if n.is_finite() && n > 0.0 { Ok(scale(a, 1.0 / n)) } else { Err(format!("a degenerate {what}")) }
}

/// Full knots (each repeated by its multiplicity) as STEP's distinct knots and multiplicities.
fn knots_of(full: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut knots, mut mults) = (Vec::new(), Vec::<usize>::new());
    for &k in full {
        match (knots.last(), mults.last_mut()) {
            (Some(last), Some(m)) if *last == k => *m += 1,
            _ => {
                knots.push(k);
                mults.push(1);
            }
        }
    }
    (knots, mults)
}

fn degree(knots: &[f64], count: usize) -> R<usize> {
    knots.len().checked_sub(count + 1).filter(|d| *d >= 1).ok_or_else(|| "a B-spline whose knots do not fit its points".to_string())
}

fn bspline_curve(full: &[f64], points: Vec<P3>, weights: Option<Vec<f64>>) -> R<Curve> {
    let degree = degree(full, points.len())?;
    let (knots, multiplicities) = knots_of(full);
    Ok(Curve::BSpline(Box::new(BSplineCurve {
        degree,
        points,
        weights,
        knots,
        multiplicities,
        form: CurveForm::Unspecified,
        closed: Logical::Unknown,
        self_intersect: Logical::Unknown,
        knot_spec: KnotSpec::Unspecified,
    })))
}

/// Homogeneous `[x w, y w, z w, w]` as the point and its weight.
fn rational(h: &[f64; 4]) -> R<(P3, f64)> {
    let w = h[3];
    if w > 0.0 { Ok(([h[0] / w, h[1] / w, h[2] / w], w)) } else { Err("a non-positive rational weight".into()) }
}

/// A record curve as the STEP curve with the same parameter.
pub fn curve(c: &BCurve) -> R<Curve> {
    Ok(match c {
        BCurve::Line { start, end } => {
            let d = sub(*end, *start);
            Curve::Line { origin: *start, direction: unit(d, "line")?, magnitude: norm(d) }
        }
        BCurve::Circle { frame, radius } => Curve::Circle { position: placement(frame), radius: *radius },
        BCurve::Ellipse { frame, radii } => Curve::Ellipse { position: placement(frame), semi_axis_1: radii[0], semi_axis_2: radii[1] },
        BCurve::Polyline { points } => Curve::Polyline(points.clone()),
        BCurve::Spline { knots, points } => bspline_curve(knots, points.clone(), None)?,
        BCurve::RationalSpline { knots, homogeneous } => {
            let (points, weights): (Vec<P3>, Vec<f64>) = homogeneous.iter().map(rational).collect::<R<Vec<_>>>()?.into_iter().unzip();
            bspline_curve(knots, points, Some(weights))?
        }
        BCurve::SurfaceIntersection { .. } => return Err(unsupported("a traced intersection curve")),
    })
}

fn bspline_surface(u_knots: &[f64], v_knots: &[f64], points: Vec<Vec<P3>>, weights: Option<Vec<Vec<f64>>>) -> R<Surface> {
    let (u_degree, v_degree) = (degree(u_knots, points.len())?, degree(v_knots, points.first().map_or(0, Vec::len))?);
    let ((u_knots, u_multiplicities), (v_knots, v_multiplicities)) = (knots_of(u_knots), knots_of(v_knots));
    Ok(Surface::BSpline(Box::new(BSplineSurface {
        u_degree,
        v_degree,
        points,
        weights,
        u_knots,
        u_multiplicities,
        v_knots,
        v_multiplicities,
        form: SurfaceForm::Unspecified,
        u_closed: Logical::Unknown,
        v_closed: Logical::Unknown,
        self_intersect: Logical::Unknown,
        knot_spec: KnotSpec::Unspecified,
    })))
}

/// The plane `origin + u a + v b` on the orthonormal frame of `a` and the normal `a × b`: the
/// record's (u, v) map linearly onto STEP's, keeping the normal.
fn plane(origin: P3, a: P3, b: P3) -> R<(Surface, Map)> {
    let x = unit(a, "plane")?;
    let z = unit(cross(a, b), "plane")?;
    let y = cross(z, x);
    let m = [[dot(a, x), dot(b, x)], [dot(a, y), dot(b, y)]];
    Ok((Surface::Plane { position: Placement { origin, axis: z, ref_direction: x } }, Map { m }))
}

/// A column-major affine matrix as its linear part (columns) and translation.
type Affine = ([P3; 3], P3);

fn affine(m: &[[f64; 4]; 4]) -> Affine {
    let col = |c: usize| [m[c][0], m[c][1], m[c][2]];
    ([col(0), col(1), col(2)], col(3))
}

fn linear(l: &[P3; 3], p: P3) -> P3 {
    let [a, b, c] = l;
    [0, 1, 2].map(|i| a[i] * p[0] + b[i] * p[1] + c[i] * p[2])
}

fn moved((l, t): &Affine, p: P3) -> P3 {
    let q = linear(l, p);
    [q[0] + t[0], q[1] + t[1], q[2] + t[2]]
}

/// A record curve mapped by an affine map: the same kind of curve on the same parameter. A
/// circle or ellipse stays one only under a similarity (`similarity`: its scale).
fn placed_curve(c: &BCurve, a: &Affine, similarity: Option<f64>) -> R<BCurve> {
    let (l, _) = a;
    let conic = |f: &Frame| -> R<(Frame, f64)> {
        let k = similarity.ok_or_else(|| unsupported("a revolution of a conic under a non-uniform placement"))?;
        Ok((Frame { origin: moved(a, f.origin), x_axis: unit(linear(l, f.x_axis), "frame")?, y_axis: unit(linear(l, f.y_axis), "frame")? }, k))
    };
    Ok(match c {
        BCurve::Line { start, end } => BCurve::Line { start: moved(a, *start), end: moved(a, *end) },
        BCurve::Circle { frame, radius } => {
            let (frame, k) = conic(frame)?;
            BCurve::Circle { frame, radius: radius * k }
        }
        BCurve::Ellipse { frame, radii } => {
            let (frame, k) = conic(frame)?;
            BCurve::Ellipse { frame, radii: radii.map(|r| r * k) }
        }
        BCurve::Polyline { points } => BCurve::Polyline { points: points.iter().map(|p| moved(a, *p)).collect() },
        BCurve::Spline { knots, points } => BCurve::Spline { knots: knots.clone(), points: points.iter().map(|p| moved(a, *p)).collect() },
        BCurve::RationalSpline { knots, homogeneous } => BCurve::RationalSpline {
            knots: knots.clone(),
            homogeneous: homogeneous
                .iter()
                .map(|h| {
                    let (p, w) = rational(h)?;
                    let q = moved(a, p);
                    Ok([q[0] * w, q[1] * w, q[2] * w, w])
                })
                .collect::<R<_>>()?,
        },
        BCurve::SurfaceIntersection { .. } => return Err(unsupported("a revolution of a traced intersection curve")),
    })
}

/// `profile` revolved about `origin` and `axis`, then placed by `matrix`, as a
/// SURFACE_OF_REVOLUTION. Exact when the placement maps revolutions about the axis onto
/// revolutions: a rotation (or reflection, which turns the angle back) after one scale across
/// the axis and another along it. STEP's u is the angle, v the profile's parameter.
fn revolution(profile: &BCurve, origin: P3, axis: P3, matrix: &[[f64; 4]; 4], forward: bool) -> R<(Surface, Map)> {
    let a = affine(matrix);
    let l = &a.0;
    let k = unit(axis, "revolution axis")?;
    // An orthonormal frame (e1, e2, k): the placement scales e1 and e2 alike, keeps them
    // orthogonal to each other and to the image of k.
    let e1 = unit(if k[0].abs() < 0.9 { cross(k, [1.0, 0.0, 0.0]) } else { cross(k, [0.0, 1.0, 0.0]) }, "revolution axis")?;
    let e2 = cross(k, e1);
    let (le1, le2, lk) = (linear(l, e1), linear(l, e2), linear(l, k));
    let (across, along) = (norm(le1), norm(lk));
    let near = |x: f64, y: f64| (x - y).abs() <= SIMILAR * across.max(along);
    let keeps_revolutions = across > 0.0
        && along > 0.0
        && near(norm(le2), across)
        && [dot(le1, le2), dot(le1, lk), dot(le2, lk)].iter().all(|d| near(*d / across.max(along), 0.0));
    if !keeps_revolutions {
        return Err(unsupported("a revolution under a placement that does not keep it one"));
    }
    let turn = if dot(cross(l[0], l[1]), l[2]) > 0.0 { 1.0 } else { -1.0 };
    let similarity = near(across, along).then_some(across);
    let profile = curve(&placed_curve(profile, &a, similarity)?)?;
    let axis = Axis1 { origin: moved(&a, origin), direction: scale(lk, 1.0 / along) };
    // Record (u, v) is (profile, angle) when `forward`, else (angle, profile).
    let m = if forward { [[0.0, turn], [1.0, 0.0]] } else { [[turn, 0.0], [0.0, 1.0]] };
    Ok((Surface::Revolution { profile: Arc::new(profile), axis }, Map { m }))
}

/// A record surface as a STEP surface, and how the record's (u, v) map onto STEP's.
pub fn surface(s: &BSurface) -> R<(Surface, Map)> {
    Ok(match s {
        BSurface::Plane { origin, u_axis, v_axis } => plane(*origin, *u_axis, *v_axis)?,
        BSurface::PlaneThrough { origin, u_point, v_point } => plane(*origin, sub(*u_point, *origin), sub(*v_point, *origin))?,
        BSurface::Cylinder { frame, radius } => (Surface::Cylinder { position: placement(frame), radius: *radius }, Map::IDENTITY),
        BSurface::Cone { frame, radius_at_origin, half_angle } => {
            // STEP's semi-angle is positive: a cone narrowing along z is the one widening along −z,
            // its frame turned over about x, (u, v) → (−u, −v).
            let k = half_angle.signum();
            let mut position = placement(frame);
            position.axis = scale(position.axis, k);
            (Surface::Cone { position, radius: *radius_at_origin, semi_angle: half_angle.abs() }, Map { m: [[k, 0.0], [0.0, k]] })
        }
        BSurface::Sphere { frame, radius } => (Surface::Sphere { position: placement(frame), radius: *radius }, Map::IDENTITY),
        BSurface::Torus { frame, major_radius, minor_radius } => {
            (Surface::Torus { position: placement(frame), major_radius: *major_radius, minor_radius: *minor_radius }, Map::IDENTITY)
        }
        BSurface::Spline { u_knots, v_knots, points } => (bspline_surface(u_knots, v_knots, points.clone(), None)?, Map::IDENTITY),
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => {
            let mut points = Vec::with_capacity(homogeneous.len());
            let mut weights = Vec::with_capacity(homogeneous.len());
            for row in homogeneous {
                let (p, w): (Vec<P3>, Vec<f64>) = row.iter().map(rational).collect::<R<Vec<_>>>()?.into_iter().unzip();
                points.push(p);
                weights.push(w);
            }
            (bspline_surface(u_knots, v_knots, points, Some(weights))?, Map::IDENTITY)
        }
        BSurface::PlacedRevolution { profile, origin, axis, matrix, forward } => revolution(profile, *origin, *axis, matrix, *forward)?,
    })
}

/// A record pcurve in the STEP surface's parameters: its control points through `map` (a linear
/// map, exact on a B-spline and on a NURBS's homogeneous coefficients).
pub fn pcurve(c: &Curve2, map: Map) -> R<Curve> {
    let lift = |p: [f64; 2]| {
        let [u, v] = map.apply(p);
        [u, v, 0.0]
    };
    match c {
        Curve2::Spline { knots, points } => bspline_curve(knots, points.iter().map(|p| lift(*p)).collect(), None),
        Curve2::RationalSpline { knots, homogeneous } => {
            let mut points = Vec::with_capacity(homogeneous.len());
            let mut weights = Vec::with_capacity(homogeneous.len());
            for h in homogeneous {
                let w = h[2];
                if !(w > 0.0) {
                    return Err("a pcurve with a non-positive rational weight".into());
                }
                points.push(lift([h[0] / w, h[1] / w]));
                weights.push(w);
            }
            bspline_curve(knots, points, Some(weights))
        }
    }
}

/// The STEP items a record became.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Written {
    /// The MANIFOLD_SOLID_BREP or BREP_WITH_VOIDS of each material region, in region order.
    pub solids: Vec<u64>,
    /// The SHELL_BASED_SURFACE_MODEL of the open shells, if there are any.
    pub sheets: Option<u64>,
    /// The ADVANCED_FACE of each record face, in the record's face order (0 for a face no shell
    /// lists, which a valid record has none of).
    pub faces: Vec<u64>,
}

/// Write `b`'s solids and sheets, each item named `name`.
pub fn brep(g: &mut GeomWriter<'_>, b: &Brep, name: &str) -> R<Written> {
    let mut w = BrepWriter {
        g,
        b,
        surfaces: vec![None; b.faces.len()],
        vertices: vec![None; b.vertices.len()],
        edges: vec![None; b.edges.len()],
        faces: vec![None; b.faces.len()],
    };
    // The bounded region each closed shell is the outer shell of.
    let mut outer_of: Vec<Option<usize>> = vec![None; b.shells.len()];
    for (ri, region) in b.regions.iter().enumerate().skip(1) {
        if let Some(slot) = region.shells.first().and_then(|s| outer_of.get_mut(*s as usize)) {
            *slot = Some(ri);
        }
    }
    let item_name = Param::Str(name.to_string());
    let mut out = Written::default();
    for region in b.regions.iter().skip(1).filter(|r| !r.void) {
        let (Some(&outer), inner) = (region.shells.first(), region.shells.get(1..).unwrap_or(&[])) else { continue };
        let shell = w.shell(outer, true)?;
        let mut voids = Vec::new();
        for &inner_shell in inner {
            match outer_of.get(inner_shell as usize).copied().flatten() {
                Some(v) if b.regions.get(v).is_some_and(|r| r.void) => {
                    let void = w.shell(inner_shell, true)?;
                    voids.push(r(w.g.w.add_distinct("ORIENTED_CLOSED_SHELL", &[s(), Param::Derived, r(void), boolean(true)])));
                }
                Some(_) => return Err(unsupported("material regions that meet across a shell")),
                // An open shell inside the material: written with the sheets.
                None => {}
            }
        }
        out.solids.push(if voids.is_empty() {
            w.g.w.add_distinct("MANIFOLD_SOLID_BREP", &[item_name.clone(), r(shell)])
        } else {
            w.g.w.add_distinct("BREP_WITH_VOIDS", &[item_name.clone(), r(shell), list(voids)])
        });
    }
    let mut sheets = Vec::new();
    for (si, shell) in b.shells.iter().enumerate() {
        match shell {
            Shell::Faces { .. } if outer_of.get(si).copied().flatten().is_none() => sheets.push(r(w.shell(index(si)?, false)?)),
            Shell::Faces { .. } => {}
            Shell::Wire { .. } | Shell::Vertex { .. } => return Err(unsupported("wire and vertex shells")),
        }
    }
    if !sheets.is_empty() {
        out.sheets = Some(w.g.w.add_distinct("SHELL_BASED_SURFACE_MODEL", &[item_name, list(sheets)]));
    }
    out.faces = w.faces.iter().map(|f| f.map_or(0, |(id, _)| id)).collect();
    Ok(out)
}

fn index(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| "too many items".to_string())
}

struct BrepWriter<'g, 'w, 'b> {
    g: &'g mut GeomWriter<'w>,
    b: &'b Brep,
    /// Each face's STEP surface and parameter map, once worked out.
    surfaces: Vec<Option<(Arc<Surface>, Map)>>,
    vertices: Vec<Option<u64>>,
    edges: Vec<Option<u64>>,
    /// Each face's ADVANCED_FACE and the side it was written with (true: the natural normal).
    faces: Vec<Option<(u64, bool)>>,
}

fn face_of(b: &Brep, f: u32) -> R<&Face> {
    b.faces.get(f as usize).ok_or_else(|| format!("face {f}: no such face"))
}

fn edge_of(b: &Brep, e: u32) -> R<&Edge> {
    b.edges.get(e as usize).ok_or_else(|| format!("edge {e}: no such edge"))
}

impl BrepWriter<'_, '_, '_> {
    fn surface(&mut self, f: u32) -> R<(Arc<Surface>, Map)> {
        if let Some(Some(s)) = self.surfaces.get(f as usize) {
            return Ok(s.clone());
        }
        let (s, map) = surface(&face_of(self.b, f)?.surface).map_err(|m| format!("face {f}: {m}"))?;
        let s = (Arc::new(s), map);
        if let Some(slot) = self.surfaces.get_mut(f as usize) {
            *slot = Some(s.clone());
        }
        Ok(s)
    }

    fn vertex(&mut self, v: u32) -> R<u64> {
        if let Some(Some(id)) = self.vertices.get(v as usize) {
            return Ok(*id);
        }
        let point = self.b.vertices.get(v as usize).ok_or_else(|| format!("vertex {v}: no such vertex"))?.point;
        let p = self.g.point(point);
        let id = self.g.w.add_distinct("VERTEX_POINT", &[s(), r(p)]);
        if let Some(slot) = self.vertices.get_mut(v as usize) {
            *slot = Some(id);
        }
        Ok(id)
    }

    fn edge(&mut self, e: u32) -> R<u64> {
        if let Some(Some(id)) = self.edges.get(e as usize) {
            return Ok(*id);
        }
        let edge = edge_of(self.b, e)?;
        let (start, end) = (self.vertex(edge.vertices[0])?, self.vertex(edge.vertices[1])?);
        let geometry = self.edge_geometry(edge).map_err(|m| format!("edge {e}: {m}"))?;
        let id = self.g.w.add_distinct("EDGE_CURVE", &[s(), r(start), r(end), r(geometry), boolean(true)]);
        if let Some(slot) = self.edges.get_mut(e as usize) {
            *slot = Some(id);
        }
        Ok(id)
    }

    /// The edge's curve, with the pcurves of its uses when it has one or two (a SURFACE_CURVE
    /// holds at most two).
    fn edge_geometry(&mut self, edge: &Edge) -> R<u64> {
        let curve_3d = curve(&edge.curve)?;
        if !(1..=2).contains(&edge.uses.len()) {
            return Ok(self.g.curve(&curve_3d));
        }
        let b = self.b;
        let mut associated = Vec::with_capacity(edge.uses.len());
        for u in &edge.uses {
            let Some(Loop::Edges { edges }) = face_of(b, u.face)?.loops.get(u.r#loop as usize) else {
                return Err(format!("face {}: loop {} is no edge loop", u.face, u.r#loop));
            };
            let used = edges.get(u.position as usize).ok_or_else(|| format!("face {}: no use at {}", u.face, u.position))?;
            let (surface, map) = self.surface(u.face)?;
            associated.push(Associated::Pcurve(Pcurve { surface, curve: Arc::new(pcurve(&used.pcurve, map)?) }));
        }
        let seam = matches!(edge.uses.as_slice(), [a, b] if a.face == b.face);
        let kind = if seam { SurfaceCurveKind::Seam } else { SurfaceCurveKind::Surface };
        Ok(self.g.curve(&Curve::SurfaceCurve { kind, curve_3d: Arc::new(curve_3d), associated, master: PreferredRep::Curve3d }))
    }

    /// The face as a shell uses it, `side` true where its normal there is its surface's natural
    /// normal: its ADVANCED_FACE, or an ORIENTED_FACE of the other side of it.
    fn face(&mut self, f: u32, side: bool) -> R<u64> {
        if let Some(Some((id, written))) = self.faces.get(f as usize).copied() {
            return Ok(if written == side { id } else { self.g.w.add_distinct("ORIENTED_FACE", &[s(), Param::Derived, r(id), boolean(false)]) });
        }
        let face = face_of(self.b, f)?;
        let (surface, map) = self.surface(f)?;
        let geometry = self.g.surface(&surface);
        let outer = self.outer_loop(face);
        let mut bounds = Vec::with_capacity(face.loops.len());
        for (li, l) in face.loops.iter().enumerate() {
            let lp = match l {
                Loop::Edges { edges } => {
                    // Counter-clockwise about the side's normal: the record's order on the natural
                    // side, reversed on the other.
                    let mut oriented = Vec::with_capacity(edges.len());
                    let uses: Box<dyn Iterator<Item = _>> = if side { Box::new(edges.iter()) } else { Box::new(edges.iter().rev()) };
                    for u in uses {
                        let e = self.edge(u.edge)?;
                        let along = boolean(u.forward == side);
                        oriented.push(r(self.g.w.add_distinct("ORIENTED_EDGE", &[s(), Param::Derived, Param::Derived, r(e), along])));
                    }
                    self.g.w.add_distinct("EDGE_LOOP", &[s(), list(oriented)])
                }
                Loop::Vertex { vertex } => {
                    let v = self.vertex(*vertex)?;
                    self.g.w.add_distinct("VERTEX_LOOP", &[s(), r(v)])
                }
            };
            let kind = if outer == Some(li) { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
            bounds.push(r(self.g.w.add_distinct(kind, &[s(), r(lp), boolean(true)])));
        }
        let same_sense = side == map.keeps_normal();
        let id = self.g.w.add_distinct("ADVANCED_FACE", &[s(), list(bounds), r(geometry), boolean(same_sense)]);
        if let Some(slot) = self.faces.get_mut(f as usize) {
            *slot = Some((id, side));
        }
        Ok(id)
    }

    /// The face's outer loop: the one edge loop whose pcurves run counter-clockwise in (u, v)
    /// about the natural normal (the record's loop convention); `None` when no single loop does.
    fn outer_loop(&self, face: &Face) -> Option<usize> {
        let mut ccw = face.loops.iter().enumerate().filter_map(|(li, l)| match l {
            Loop::Edges { edges } => {
                let mut ring = Vec::with_capacity(edges.len() * AREA_SAMPLES);
                for u in edges {
                    let [t0, t1] = edge_of(self.b, u.edge).ok()?.interval;
                    for k in 0..AREA_SAMPLES {
                        let f = k as f64 / AREA_SAMPLES as f64;
                        ring.push(u.pcurve.eval(if u.forward { t0 + (t1 - t0) * f } else { t1 - (t1 - t0) * f })?);
                    }
                }
                (signed_area(&ring) > 0.0).then_some(li)
            }
            Loop::Vertex { .. } => None,
        });
        let first = ccw.next();
        if ccw.next().is_some() { None } else { first }
    }

    /// A face shell: CLOSED_SHELL when it bounds a region, else OPEN_SHELL.
    fn shell(&mut self, si: u32, closed: bool) -> R<u64> {
        let b = self.b;
        let Some(Shell::Faces { faces, wires }) = b.shells.get(si as usize) else {
            return Err(format!("shell {si}: not a face shell"));
        };
        if !wires.is_empty() {
            return Err(unsupported("wire edges inside a face shell"));
        }
        let mut entries = Vec::with_capacity(faces.len());
        for of in faces {
            entries.push(r(self.face(of.face, of.forward)?));
        }
        let kind = if closed { "CLOSED_SHELL" } else { "OPEN_SHELL" };
        Ok(self.g.w.add_distinct(kind, &[s(), list(entries)]))
    }
}

/// Signed area of a closed polygon in (u, v): positive counter-clockwise.
fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    0.5 * (0..n).filter_map(|i| Some((ring.get(i)?, ring.get((i + 1) % n)?))).map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum::<f64>()
}
