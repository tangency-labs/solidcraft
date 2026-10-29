//! Convert to Sheet Metal through the skin (#6748): a body's faces as a skin of panels, rolls
//! among them, laid out flat by `solidcraft-develop`. No face is fixed: every face of the sheet
//! is a panel like any other, and the flat is drawn in the largest one's plane.
//!
//! The sheet is every face of a solid, or of one side of a shell or a plate (the side its largest
//! face is on, reached across every edge but a rim's), less the faces the user skips. A cylinder
//! or a cone is a roll that keeps its modeled radius, joined by seams where the sheet runs on
//! smoothly. Every sharp edge between two faces of the sheet is cut open, half the rule's gap
//! from the plane halfway between the two faces as `sheet.convert` cuts it, unless the user picks
//! it as a bend. A ring of bends and seams is refused, naming its edges. A solid's material lies
//! inside its faces, or outside them; a shell or a plate keeps its own.
//!
//! Not converted yet: a free-form face (the kernel does not hand over its spline yet), a sphere
//! or a torus (neither is developable), a cylinder or cone that closes round its axis, a bend on
//! the edge of a roll, and a shell's face that meets the sheet at a sharp edge but is not a plane
//! (it is left out).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use solidcraft_develop::{Anchor, Bend, Datum, Development, Joint, JointKind, Panel, Skin, Surface, Tail, ZoneEnds, develop};
use solidcraft_geom::{Loop2, Mesh, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, FaceSurface, face_surface};

use crate::sheet::{outline_loop, round_wall};
use crate::{DocError, ModelBody, Result};

/// Normals closer than this (radians) meet smoothly: the sheet runs on across their edge.
const SMOOTH: f64 = 1e-6;

/// The rule's values Convert works with.
#[derive(Clone, Copy, Debug)]
pub struct SkinRule {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    pub gap: f64,
}

/// A body converted through its skin: the skin and its development, the body's face each panel
/// is, a point on the edge of each joint, and the sheet's thickness (a shell's own).
#[derive(Clone, Debug)]
pub struct SkinConversion {
    pub skin: Skin,
    pub development: Development,
    pub faces: Vec<usize>,
    pub edges: Vec<Vec3>,
    pub thickness: f64,
}

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

fn at(p: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)
}

/// What a face of the body is as a panel, and how its points map to the surface's parameters,
/// which run with `u x v` toward the material (opposite the face's outward normal).
#[derive(Clone, Copy, Debug)]
enum Shape {
    Plane { plane: Plane, out: Vec3 },
    Cylinder { origin: Vec3, axis: Vec3, e1: Vec3, e2: Vec3, radius: f64, inward: bool },
    Cone { apex: Vec3, axis: Vec3, e1: Vec3, e2: Vec3, half: f64, inward: bool },
}

impl Shape {
    /// Face `f`'s shape. A cylinder's or cone's material lies toward its axis when the face's
    /// outward normal (from the mesh's winding) points away from it; `flip` puts the material
    /// outside the body instead.
    fn of(b: &Body, mesh: &Mesh, f: usize, flip: bool) -> std::result::Result<Shape, &'static str> {
        let frame = |axis: Vec3| {
            let pick = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let e1 = (pick - axis * pick.dot(axis)).normalized().unwrap_or(Vec3::X);
            (e1, axis.cross(e1))
        };
        let outward = |from_axis: &dyn Fn(Vec3) -> Vec3| -> bool {
            let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]);
            let tri = mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t));
            tri.max_by(|x, y| area(x).len().total_cmp(&area(y).len()))
                .is_some_and(|t| area(&t).dot(from_axis((t[0] + t[1] + t[2]) * (1.0 / 3.0))) > 0.0)
        };
        match face_surface(b, f) {
            Some(FaceSurface::Plane { normal, point }) => {
                let out = if flip { -*normal } else { *normal };
                Plane::from_normal(point, -out).map(|plane| Shape::Plane { plane, out }).ok_or("a face of the body has no plane")
            }
            Some(FaceSurface::Cylinder { origin, axis, radius }) => {
                let axis = *axis;
                let (e1, e2) = frame(axis);
                let inward = outward(&|p| (p - origin) - axis * (p - origin).dot(axis)) != flip;
                Ok(Shape::Cylinder { origin, axis, e1, e2, radius, inward })
            }
            Some(FaceSurface::Cone { apex, axis, half_angle }) => {
                let axis = *axis;
                let (e1, e2) = frame(axis);
                let inward = outward(&|p| (p - apex) - axis * (p - apex).dot(axis)) != flip;
                Ok(Shape::Cone { apex, axis, e1, e2, half: half_angle, inward })
            }
            Some(FaceSurface::Sphere { .. } | FaceSurface::Torus { .. }) => Err("a sphere or a torus is not developable"),
            None => Err("not supported yet: a free-form face (the kernel does not hand over its spline yet)"),
        }
    }

    fn surface(&self) -> Surface {
        match *self {
            Shape::Plane { .. } => Surface::Plane,
            // The skin's radius is the face's; with the material outward the face is the inside.
            Shape::Cylinder { radius, inward, .. } => Surface::Cylinder { radius, material_inward: inward },
            Shape::Cone { half, inward, .. } => Surface::Cone { half_angle: half, material_inward: inward },
        }
    }

    /// The parameters of a point of the face, its angle not yet unwrapped.
    fn params(&self, p: Vec3) -> Vec2 {
        let turn = |w: Vec3, e1: Vec3, e2: Vec3, inward: bool| w.dot(e2).atan2(w.dot(e1)) * if inward { 1.0 } else { -1.0 };
        match *self {
            Shape::Plane { plane, .. } => plane.to_local(p),
            Shape::Cylinder { origin, axis, e1, e2, inward, .. } => Vec2::new((p - origin).dot(axis), turn(p - origin, e1, e2, inward)),
            Shape::Cone { apex, e1, e2, inward, .. } => Vec2::new((p - apex).len(), turn(p - apex, e1, e2, inward)),
        }
    }

    fn angular(&self) -> bool {
        !matches!(self, Shape::Plane { .. })
    }

    /// The outward unit normal at a point of the face.
    fn out(&self, p: Vec3) -> Vec3 {
        let side = |inward: bool| if inward { 1.0 } else { -1.0 };
        match *self {
            Shape::Plane { out, .. } => out,
            Shape::Cylinder { origin, axis, inward, .. } => {
                ((p - origin) - axis * (p - origin).dot(axis)).normalized().unwrap_or(axis) * side(inward)
            }
            Shape::Cone { apex, axis, half, inward, .. } => {
                let c = ((p - apex) - axis * (p - apex).dot(axis)).normalized().unwrap_or(axis);
                (c * half.cos() - axis * half.sin()) * side(inward)
            }
        }
    }
}

/// Face `f`'s boundary loops from the mesh's edges, chained head to tail: each point with the
/// face across the loop from it to the next.
fn face_loops(mesh: &Mesh, f: usize, tol: f64) -> Result<Vec<Vec<(Vec3, Option<usize>)>>> {
    let mut edges: Vec<(&[Vec3], Option<usize>)> = mesh
        .edges
        .iter()
        .zip(&mesh.edge_faces)
        .filter(|(line, faces)| line.len() >= 2 && faces.iter().any(|g| *g as usize == f))
        .map(|(line, faces)| (line.as_slice(), faces.iter().map(|g| *g as usize).find(|g| *g != f)))
        .collect();
    let mut loops = Vec::new();
    while let Some((line, across)) = edges.pop() {
        let (Some(&start), Some(&last)) = (line.first(), line.last()) else { continue };
        let mut lp: Vec<(Vec3, Option<usize>)> = line.iter().take(line.len().saturating_sub(1)).map(|p| (*p, across)).collect();
        let mut end = last;
        while end.dist(start) > tol {
            let meets = |p: Option<&Vec3>| p.is_some_and(|p| p.dist(end) <= tol);
            let Some(k) = edges.iter().position(|(l, _)| meets(l.first()) || meets(l.last())) else {
                return err("the outline of a face of the body does not close");
            };
            let (l, g) = edges.swap_remove(k);
            let run: Vec<Vec3> = if meets(l.first()) { l.to_vec() } else { l.iter().rev().copied().collect() };
            lp.extend(run.iter().take(run.len().saturating_sub(1)).map(|p| (*p, g)));
            end = run.last().copied().unwrap_or(start);
        }
        loops.push(lp);
    }
    Ok(loops)
}

/// A face's region in its surface's parameters. A plane's loops are read exactly, an arc where
/// one runs along a round wall standing square on it; a roll's are straight in its parameters
/// along its rulings and its circles, and kept as the mesh's points elsewhere.
fn region(b: &Body, shape: &Shape, loops: &[Vec<(Vec3, Option<usize>)>], tol: f64) -> Result<Region2> {
    let mut shaped: Vec<Loop2> = Vec::new();
    for lp in loops {
        let mut pts: Vec<Vec2> = lp.iter().map(|(p, _)| shape.params(*p)).collect();
        if shape.angular() {
            // The angle unwrapped along the loop; a loop that comes back a turn away closes
            // round the axis.
            for k in 1..pts.len() {
                let (Some(prev), Some(cur)) = (pts.get(k - 1).copied(), pts.get_mut(k)) else { continue };
                cur.y += ((prev.y - cur.y) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            }
            if let (Some(first), Some(last)) = (pts.first(), pts.last())
                && (last.y - first.y).abs() > std::f64::consts::PI
            {
                return err("not supported yet: a cylinder or cone that closes round its axis (split it, or pick its seam as a rip)");
            }
        }
        shaped.push(match shape {
            Shape::Plane { plane, out } => {
                let tagged: Vec<(Vec2, Option<u32>)> = pts.iter().zip(lp).map(|(p, (_, g))| (*p, g.and_then(|g| u32::try_from(g).ok()))).collect();
                let mut wall = |g: u32| round_wall(b, plane, *out, g);
                outline_loop(&tagged, &mut wall, tol * 10.0)
            }
            _ => Loop2 { segs: straight_runs(&pts, tol) },
        });
    }
    let area = |l: &Loop2| l.signed_area().abs();
    shaped.sort_by(|x, y| area(y).total_cmp(&area(x)));
    let mut it = shaped.into_iter();
    let Some(outer) = it.next() else { return err("a face of the body has no edges") };
    let mut holes: Vec<Loop2> = it.collect();
    if shape.angular() {
        // A hole is unwrapped on its own: bring it to the turn the outline is on.
        let mean = |l: &Loop2| l.segs.iter().map(|s| s.start().y).sum::<f64>() / l.segs.len().max(1) as f64;
        let m = mean(&outer);
        for h in &mut holes {
            let shift = ((m - mean(h)) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            if shift != 0.0 {
                let mv = solidcraft_develop::Rigid2 { cos: 1.0, sin: 0.0, shift: Vec2::new(0.0, shift) };
                *h = mv.loop2(h);
            }
        }
    }
    Ok(Region2 { outer: outer.ccw(), holes: holes.iter().map(|h| h.ccw().reversed()).collect() })
}

/// A closed polyline as lines, each run of points along one straight line joined into one.
fn straight_runs(pts: &[Vec2], tol: f64) -> Vec<Seg2> {
    let n = pts.len();
    let straight = |a: Vec2, p: Vec2, c: Vec2| (c - a).normalized().is_some_and(|d| (p - a).cross(d).abs() <= tol * 1e-3);
    // Corners: points where the outline turns.
    let corners: Vec<Vec2> = (0..n)
        .filter_map(|i| {
            let (a, p, c) = (pts.get((i + n - 1) % n)?, pts.get(i)?, pts.get((i + 1) % n)?);
            (!straight(*a, *p, *c)).then_some(*p)
        })
        .collect();
    let m = corners.len();
    (0..m).filter_map(|i| Some(Seg2::Line { a: *corners.get(i)?, b: *corners.get((i + 1) % m)? })).collect()
}

/// The normal of the plane halfway between panels `x` and `y` through the edge between them,
/// toward `x`'s side. `e` runs along the edge with `x` on its left seen from outside; normals are
/// outward.
fn rip_plane(nx: Vec3, ny: Vec3, e: Vec3) -> Option<Vec3> {
    let (wx, wy) = (nx.cross(e), e.cross(ny));
    (wx - wy).normalized().filter(|m| wx.dot(*m) > 1e-9)
}

/// How far a panel `x` is cut back from an edge between it and panel `y` so that its end stays
/// half the gap from the plane halfway between them, as `sheet.convert` cuts a rip.
fn rip_cut(nx: Vec3, ny: Vec3, e: Vec3, t: f64, gap: f64) -> Result<f64> {
    let Some(mid) = rip_plane(nx, ny, e) else {
        return err("two faces of the sheet lie on each other along the edge between them");
    };
    let lean = -nx.dot(mid) * t;
    Ok((gap / 2.0 - lean.min(0.0)) / nx.cross(e).dot(mid))
}

/// How far a bend is cut back square along its edge from the corner `v` so that none of its
/// metal comes nearer than half the gap to a rip's plane through `v` (normal `m`, toward the side
/// the bend keeps), as the rip cuts the panels beside it. `d` runs along the edge from `v`; `na`
/// and `nb` are the two panels' normals away from their material. The bend's metal is the ring
/// sector between radii `r` and `r + t` round its axis, which lies `r + t` inside both faces
/// where it turns toward the material (the faces are its outside) and `r` outside them where it
/// turns away (they are its inside). None where running along the edge leads no further from the
/// plane.
fn bend_cut(v: Vec3, d: Vec3, m: Vec3, (na, nb): (Vec3, Vec3), toward: bool, r: f64, t: f64, gap: f64) -> Option<f64> {
    let away = d.dot(m);
    let cos = na.dot(nb).clamp(-1.0, 1.0);
    if away <= 1e-9 || 1.0 + cos <= 1e-9 {
        return None;
    }
    let depth = if toward { -(r + t) } else { r };
    let axis = v + (na + nb) * (depth / (1.0 + cos));
    // The sector runs from the radius toward `a`'s face to the one toward `b`'s.
    let (ua, ub) = if toward { (na, nb) } else { (-na, -nb) };
    let across = (ub - ua * ub.dot(ua)).normalized()?;
    let theta = cos.acos();
    let (cu, cv) = (ua.dot(m), across.dot(m));
    let lowest = (-cv).atan2(-cu).rem_euclid(std::f64::consts::TAU);
    let turns = [0.0, theta, if lowest <= theta { lowest } else { 0.0 }];
    let nearest =
        turns.iter().flat_map(|a| [r, r + t].map(|radius| (axis - v).dot(m) + radius * (a.cos() * cu + a.sin() * cv))).fold(f64::INFINITY, f64::min);
    Some(((gap / 2.0 - nearest) / away).max(0.0))
}

/// What an edge between two faces of the sheet is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Seam,
    Bend,
    Rip,
}

/// Converts `mb` through its skin, less the faces at the `skip` points, bending at the edges at
/// the `bends` points and cutting every other sharp edge between two faces of the sheet open.
/// A solid's material lies inside its faces, or outside them when `outward`.
pub fn convert_skin(mb: &ModelBody, rule: &SkinRule, bends: &[Vec3], skip: &[Vec3], outward: bool) -> Result<SkinConversion> {
    let (b, mesh) = (&mb.body, mb.mesh());
    let size = b.size();
    if !(size.is_finite() && size > 0.0) {
        return err("the body has no size");
    }
    let (tol, near) = ((size * 1e-6).max(1e-6), (size * 2e-3).max(1e-3));
    let count = mesh.tri_face.iter().map(|g| *g as usize + 1).max().unwrap_or(0);
    let area: Vec<f64> = (0..count).map(|f| face_area(&mesh, f)).collect();
    let area_of = |f: usize| area.get(f).copied().unwrap_or(0.0);
    let on_edge = |p: Vec3, line: &[Vec3]| line.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min) <= near;
    let picked = |picks: &[Vec3], line: &[Vec3]| picks.iter().any(|p| on_edge(*p, line));
    if let Some(p) = bends.iter().find(|p| !mesh.edges.iter().any(|l| on_edge(**p, l))) {
        return err(format!("no edge of the body at the bend point {}", at(*p)));
    }
    let mut skipped = BTreeSet::new();
    for p in skip {
        let Some((_, g)) = crate::appearance::nearest_face(mb, *p).filter(|(d, _)| *d <= near) else {
            return err(format!("no face of the body at the skipped face point {}", at(*p)));
        };
        skipped.insert(g);
    }

    // The walk starts on the largest plane. A shell or a plate: a parallel face the same small
    // distance behind it. It keeps its thickness and its material; a solid takes the rule's.
    let planes: Vec<(usize, Vec3)> =
        (0..count).filter_map(|f| if let Ok(Shape::Plane { out, .. }) = Shape::of(b, &mesh, f, false) { Some((f, out)) } else { None }).collect();
    let Some(&(start, n0)) = planes.iter().max_by(|x, y| area_of(x.0).total_cmp(&area_of(y.0))) else {
        return err("not supported yet: a body with no flat face");
    };
    let sheet = behind(&mesh, start, n0).filter(|d| *d > 0.0 && d * d * 16.0 <= area_of(start));
    let t = sheet.map_or(rule.thickness, |d| (d * 1e9).round() / 1e9);
    let shapes: Vec<std::result::Result<Shape, &'static str>> = (0..count).map(|f| Shape::of(b, &mesh, f, outward && sheet.is_none())).collect();
    let shape = |f: usize| shapes.get(f).copied().unwrap_or(Err("a face the mesh does not have"));
    let smooth = |line: &[Vec3], x: &Shape, y: &Shape| -> bool {
        let m = middle(line).unwrap_or_default();
        x.out(m).dot(y.out(m)).clamp(-1.0, 1.0).acos() <= SMOOTH
    };

    // The faces of the sheet: a solid's all, a shell's or a plate's side the start is on. That
    // side runs on smoothly, or across a sharp edge between two planes that is no rim: a face's
    // rims (its thickness faces) touch the face behind it.
    let touches = |g: usize, h: usize| mesh.edge_faces.iter().any(|fs| fs.contains(&(g as u32)) && fs.contains(&(h as u32)));
    let mut behind_of: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    let mut sheet_faces = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(f) = queue.pop_front() {
        for (line, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
            let (&[x, y], true) = (faces.as_slice(), line.len() >= 2) else { continue };
            let (x, y) = (x as usize, y as usize);
            let g = if x == f {
                y
            } else if y == f {
                x
            } else {
                continue;
            };
            if g == f || sheet_faces.contains(&g) {
                continue;
            }
            let crosses = sheet.is_none() || {
                let rim = behind_of.entry(f).or_insert_with(|| behind_face(&mesh, f)).is_some_and(|h| touches(g, h));
                match (shape(f), shape(g)) {
                    (Ok(sf), Ok(sg)) => !rim && (smooth(line, &sf, &sg) || matches!((sf, sg), (Shape::Plane { .. }, Shape::Plane { .. }))),
                    _ => false,
                }
            };
            if crosses {
                sheet_faces.insert(g);
                queue.push_back(g);
            }
        }
    }
    sheet_faces.retain(|f| !skipped.contains(f));
    let mut shaped: BTreeMap<usize, Shape> = BTreeMap::new();
    for &f in &sheet_faces {
        match shape(f) {
            Ok(s) => shaped.insert(f, s),
            Err(why) => return err(format!("the face at {} can't be a panel: {why}. Skip it to leave it out", at(face_point(&mesh, f)))),
        };
    }

    // Each edge between two faces of the sheet: a seam where it runs on smoothly, a bend where
    // the user picked it, and cut open everywhere else.
    let mut roles: Vec<(usize, usize, usize, Role)> = Vec::new();
    for (e, (line, faces)) in mesh.edges.iter().zip(&mesh.edge_faces).enumerate() {
        let (&[x, y], true) = (faces.as_slice(), line.len() >= 2) else { continue };
        let (x, y) = (x as usize, y as usize);
        let (Some(sx), Some(sy)) = (shaped.get(&x), shaped.get(&y)) else { continue };
        if x == y {
            continue;
        }
        let role = if smooth(line, sx, sy) {
            Role::Seam
        } else if picked(bends, line) {
            Role::Bend
        } else {
            Role::Rip
        };
        roles.push((e, x, y, role));
    }
    let between = |p: &Vec3| roles.iter().any(|(e, ..)| mesh.edges.get(*e).is_some_and(|l| on_edge(*p, l)));
    if let Some(p) = bends.iter().find(|p| !between(p)) {
        return err(format!("the bend at {} is not between two faces of the sheet", at(*p)));
    }

    // The panels, each piece from its largest face (a plane first) across its bends and seams;
    // the first piece's start is the flat's frame.
    let largest = |left: &BTreeSet<usize>| {
        left.iter().copied().max_by(|x, y| {
            let plane = |f: &usize| matches!(shaped.get(f), Some(Shape::Plane { .. }));
            plane(x).cmp(&plane(y)).then(area_of(*x).total_cmp(&area_of(*y))).then(y.cmp(x))
        })
    };
    let mut left = sheet_faces.clone();
    let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
    let mut order = Vec::with_capacity(left.len());
    while let Some(root) = largest(&left) {
        left.remove(&root);
        order.push(root);
        let mut queue = VecDeque::from([root]);
        while let Some(f) = queue.pop_front() {
            for &(_, x, y, role) in &roles {
                let g = if x == f {
                    y
                } else if y == f {
                    x
                } else {
                    continue;
                };
                if role != Role::Rip && left.remove(&g) {
                    parent.insert(g, f);
                    order.push(g);
                    queue.push_back(g);
                }
            }
        }
    }
    if order.is_empty() {
        return err("every face of the body is skipped");
    }
    let index: BTreeMap<usize, usize> = order.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let mut panels = Vec::with_capacity(order.len());
    for &f in &order {
        let Some(shape) = shaped.get(&f) else { continue };
        let loops = face_loops(&mesh, f, tol)?;
        let region = region(b, shape, &loops, tol).map_err(|e| {
            DocError::Invalid(format!("the face at {} can't be a panel: {e}", at(loops.first().and_then(|l| l.first()).map_or(Vec3::ZERO, |p| p.0))))
        })?;
        panels.push(Panel { name: format!("face {f}"), surface: shape.surface(), region });
    }

    // The joints: every edge between two faces of the sheet. Each rip's plane, by its ends and
    // toward its `a` face, and each bend's edge, for the bends' ends below.
    let (mut joints, mut edges) = (Vec::new(), Vec::new());
    let mut rips: Vec<([Vec3; 2], (usize, usize), Vec3)> = Vec::new();
    let mut bent: Vec<(usize, [Vec3; 2], (usize, usize), (Vec3, Vec3), bool, f64)> = Vec::new();
    for &(e, x, y, role) in &roles {
        let Some(line) = mesh.edges.get(e) else { continue };
        let (Some(&p0), Some(&p1)) = (line.first(), line.last()) else { continue };
        let (Some(&ix), Some(&iy), Some(sx), Some(sy)) = (index.get(&x), index.get(&y), shaped.get(&x), shaped.get(&y)) else { continue };
        // `a` is the face the other was reached from, where there is one.
        let ((ia, sa), (ib, sb)) = if parent.get(&x) == Some(&y) { ((iy, sy), (ix, sx)) } else { ((ix, sx), (iy, sy)) };
        let faces = if parent.get(&x) == Some(&y) { (y, x) } else { (x, y) };
        let mid = middle(line).unwrap_or(p0);
        // The edge's ends in each panel's parameters, run so that `a`'s material is on the left.
        let (Some(ra), Some(rb)) = (panels.get(ia).map(|p: &Panel| p.region.clone()), panels.get(ib).map(|p: &Panel| p.region.clone())) else {
            continue;
        };
        let ends = |s: &Shape, from: Vec3, to: Vec3, near_to: &Region2| -> Option<(Vec2, Vec2)> {
            let (mut u0, mut u1) = (s.params(from), s.params(to));
            // On a roll, the angle on the side the panel is.
            if s.angular() {
                let c = near_to.outer.segs.first().map_or(u0, Seg2::start);
                for u in [&mut u0, &mut u1] {
                    u.y += ((c.y - u.y) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                }
            }
            Some((u0, u1))
        };
        let Some((mut a0, mut a1)) = ends(sa, p0, p1, &ra) else { continue };
        let probe = |r: &Region2, u0: Vec2, u1: Vec2, left: bool| -> bool {
            let Some(d) = (u1 - u0).normalized() else { return false };
            let extent = r.outer.polyline(1e-3).bounds().size().len().max(1e-9);
            let side = if left { d.perp() } else { -d.perp() };
            r.contains((u0 + u1) * 0.5 + side * (extent * 1e-4))
        };
        let (from, to) = if probe(&ra, a0, a1, true) { (p0, p1) } else { (p1, p0) };
        if from != p0 {
            std::mem::swap(&mut a0, &mut a1);
        }
        let Some((b0, b1)) = ends(sb, from, to, &rb) else { continue };
        let (Some(da), Some(db)) = ((a1 - a0).normalized(), (b1 - b0).normalized()) else { continue };
        let kind = if role == Role::Bend {
            let (Shape::Plane { out: na, .. }, Shape::Plane { out: nb, .. }) = (sa, sb) else {
                return err(format!("not supported yet: a bend on the edge of a roll, at {}", at(mid)));
            };
            let theta = na.dot(*nb).clamp(-1.0, 1.0).acos();
            // The bend turns toward the material where `b` rises behind `a`'s face.
            let lift = panels.get(ib).map_or(0.0, |p| {
                let pts = p.region.outer.polyline(1e-3);
                let Shape::Plane { plane, .. } = sb else { return 0.0 };
                (&pts).into_iter().map(|q| (plane.to_world(*q) - from).dot(*na)).fold(0.0, |m: f64, h| if h.abs() > m.abs() { h } else { m })
            });
            let len = a0.dist(a1);
            bent.push((joints.len(), [from, to], faces, (*na, *nb), lift < 0.0, len));
            JointKind::Bend(Bend {
                angle: theta,
                radius: rule.bend_radius,
                toward_material: lift < 0.0,
                datum: Datum::Sharp,
                ends: ZoneEnds { near: (0.0, len), far: (0.0, len) },
                tails: (Tail::Keep, Tail::Trim),
                relieved: (false, false),
            })
        } else if role == Role::Seam {
            JointKind::Seam
        } else {
            let e = (to - from).normalized().unwrap_or(Vec3::X);
            let cut = |s: &Shape, o: &Shape, e: Vec3| -> Result<f64> {
                match (s, o) {
                    (Shape::Plane { out: nx, .. }, Shape::Plane { out: ny, .. }) => rip_cut(*nx, *ny, e, t, rule.gap),
                    _ => Ok(rule.gap / 2.0),
                }
            };
            if let (Shape::Plane { out: na, .. }, Shape::Plane { out: nb, .. }) = (sa, sb)
                && let Some(m) = rip_plane(*na, *nb, -e)
            {
                rips.push(([from, to], faces, m));
            }
            JointKind::Rip { cut: (cut(sa, sb, -e)?, cut(sb, sa, e)?) }
        };
        joints.push(Joint { a: Anchor { panel: ia, origin: a0, dir: da }, b: Anchor { panel: ib, origin: b0, dir: db }, kind });
        edges.push(mid);
    }
    // A bend ends where its metal does. At a corner where a rip cuts a panel the bend joins, the
    // rip's plane cuts the bend square too, so that one sheet drawn from its outside, or from its
    // inside with its material out, unfolds alike.
    for (j, [from, to], (fa, fb), normals, toward, len) in bent {
        let cut_at = |v: Vec3, d: Vec3| -> Option<f64> {
            let at_corner = rips.iter().filter(|(ends, ..)| ends.iter().any(|p| p.dist(v) <= tol * 10.0));
            let planes = at_corner.filter_map(|(_, (x, y), m)| {
                if *x == fa || *x == fb {
                    Some(*m)
                } else if *y == fa || *y == fb {
                    Some(-*m)
                } else {
                    None
                }
            });
            planes.filter_map(|m| bend_cut(v, d, m, normals, toward, rule.bend_radius, t, rule.gap)).reduce(f64::max)
        };
        let Some(d) = (to - from).normalized() else { continue };
        let (first, last) = (cut_at(from, d), cut_at(to, -d));
        let (start, end) = (first.unwrap_or(0.0), len - last.unwrap_or(0.0));
        if start >= end {
            return err(format!("the bend at {} is cut away by the rips at its ends", at(from.lerp(to, 0.5))));
        }
        // Past a cut end, the panels stop at their tangent lines: the corner's relief.
        if let Some(Joint { kind: JointKind::Bend(b), .. }) = joints.get_mut(j) {
            b.ends = ZoneEnds { near: (start, end), far: (start, end) };
            b.relieved = (first.is_some(), last.is_some());
        }
    }
    let skin = Skin { thickness: t, k_factor: rule.k_factor, panels, joints };
    let development = develop(&skin, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
    if !development.loops.is_empty() {
        let names: Vec<String> = development.loops.iter().filter_map(|m| edges.get(m.joint)).map(|p| at(*p)).collect();
        return err(format!(
            "the edge at {} closes a ring of bends and seams: leave one of the ring's edges out of the bends to cut it open",
            names.join(" and the one at ")
        ));
    }
    Ok(SkinConversion { skin, development, faces: order, edges, thickness: t })
}

/// The area of face `f`'s triangles.
fn face_area(mesh: &Mesh, f: usize) -> f64 {
    let tris = mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t));
    tris.map(|t| (t[1] - t[0]).cross(t[2] - t[0]).len() / 2.0).sum()
}

/// A point in face `f`: the middle of its largest triangle.
fn face_point(mesh: &Mesh, f: usize) -> Vec3 {
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).len();
    let tri = mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t));
    tri.max_by(|x, y| area(x).total_cmp(&area(y))).map_or(Vec3::ZERO, |t| (t[0] + t[1] + t[2]) * (1.0 / 3.0))
}

/// The face straight through the material behind face `f`, from a ray through its largest
/// triangle along its inward normal.
fn behind_face(mesh: &Mesh, f: usize) -> Option<usize> {
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]);
    let tri = mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t));
    let t = tri.max_by(|x, y| area(x).len().total_cmp(&area(y).len()))?;
    let n = area(&t).normalized()?;
    let (_, hit) = mesh.raycast((t[0] + t[1] + t[2]) * (1.0 / 3.0), n * -1.0)?;
    mesh.tri_face.get(hit).map(|g| *g as usize).filter(|g| *g != f)
}

/// How far behind planar face `f` (outward normal `n`) a parallel face facing the other way
/// lies, from rays through its largest triangles.
fn behind(mesh: &Mesh, f: usize, n: Vec3) -> Option<f64> {
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).len();
    let mut tris: Vec<[Vec3; 3]> =
        mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t)).collect();
    tris.sort_by(|x, y| area(y).total_cmp(&area(x)));
    let middle = tris.first().map(|t| (t[0] + t[1] + t[2]) * (1.0 / 3.0))?;
    let (d, hit) = mesh.raycast(middle, n * -1.0)?;
    let g = *mesh.tri_face.get(hit)? as usize;
    let back = mesh.triangles.get(hit).and_then(|t| mesh.tri(t))?;
    let nb = (back[1] - back[0]).cross(back[2] - back[0]).normalized()?;
    (g != f && nb.dot(n) < -1.0 + 1e-9).then_some(d)
}

impl SkinConversion {
    /// The flat pattern: each placed panel less its trims, and the bend zones, joined.
    pub fn flat(&self) -> Result<Vec<Region2>> {
        crate::sheet_skin::join(&self.development, &mut crate::sheet::FlatWork::new())
    }
}

/// The point halfway along a polyline.
fn middle(line: &[Vec3]) -> Option<Vec3> {
    let total: f64 = line.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total / 2.0;
    for w in line.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            return Some(w[0].lerp(w[1], left / l));
        }
        left -= l;
    }
    line.first().copied()
}

/// A body converted through its skin, as the model keeps it: its flat pattern, which
/// `sheet.flat_pattern` shows. The body itself stays the solid as modeled.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkinFlat {
    pub body: String,
    pub rule: String,
    pub thickness: f64,
    pub k_factor: f64,
    /// The flat material, in the plane of the sheet's largest face.
    pub regions: Vec<Region2>,
    pub bends: Vec<SkinBendLine>,
    /// The largest strain any panel's flat leaves: none but on a free-form panel.
    pub stretch: f64,
    /// How many of the panels are rolls.
    pub rolls: usize,
}

/// A bend in a skin's flat: its zone, its center line, how far it turns (radians), its inside
/// radius, whether it turns toward the material, and the zone's width.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkinBendLine {
    pub zone: Region2,
    pub start: Vec2,
    pub end: Vec2,
    pub angle: f64,
    pub radius: f64,
    pub up: bool,
    pub allowance: f64,
}

impl SkinConversion {
    /// The flat pattern as the model keeps it, for the body `body` under the rule `rule`.
    pub fn record(&self, body: &str, rule: &str) -> Result<SkinFlat> {
        let bends = self
            .development
            .zones
            .iter()
            .filter_map(|z| {
                let JointKind::Bend(b) = &self.skin.joints.get(z.joint)?.kind else { return None };
                Some(SkinBendLine {
                    zone: z.region.clone(),
                    start: (z.near[0] + z.far[0]) * 0.5,
                    end: (z.near[1] + z.far[1]) * 0.5,
                    angle: b.angle,
                    radius: b.radius,
                    up: b.toward_material,
                    allowance: b.allowance(self.skin.thickness, self.skin.k_factor),
                })
            })
            .collect();
        Ok(SkinFlat {
            body: body.into(),
            rule: rule.into(),
            thickness: self.thickness,
            k_factor: self.skin.k_factor,
            regions: self.flat()?,
            bends,
            stretch: self.development.panels.iter().flatten().map(|p| p.stretch).fold(0.0, f64::max),
            rolls: self.skin.panels.iter().filter(|p| p.surface != Surface::Plane).count(),
        })
    }
}
