//! Analytic boundary recovery from explicit incident supports and owned display normals.
//! No source dimensions, face numbering or shortest-arc assumptions are used.

use super::{cross, dot, sub};
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, HyperbolaCurve, ParabolaCurve};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    CadIr,
    geometry::{CurveGeometry, SolvedCurveGeometry as C, SolvedSurfaceGeometry as S, SurfaceGeometry},
    topology::{FaceLoops, Sense},
};
use std::collections::{BTreeMap, BTreeSet};

type V = [f64; 3];
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn mul(a: V, k: f64) -> V {
    a.map(|x| x * k)
}
fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> Result<V, String> {
    let n = len(a);
    if !n.is_finite() || n < 1e-12 {
        return Err("SLDPRT: degenerate analytic direction".into());
    }
    Ok(mul(a, 1. / n))
}
fn point(p: Point3) -> V {
    [p.x, p.y, p.z]
}
fn vector(p: Vector3) -> V {
    [p.x, p.y, p.z]
}
fn xyz(p: V) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}
fn vec(p: V) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}
fn flip(s: Sense) -> Sense {
    match s {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}
fn sign(s: Sense) -> f64 {
    if s == Sense::Forward { 1. } else { -1. }
}

#[derive(Clone)]
pub(in crate::native_cad) enum Support {
    Plane { p: V, n: V, u: V },
    Rotation { p: V, z: V, r: f64, angle: f64 },
    Torus { p: V, z: V, major: f64, minor: f64 },
    Sphere { p: V, radius: f64 },
}
impl Support {
    fn read(s: &SurfaceGeometry) -> Option<Self> {
        match s {
            SurfaceGeometry::Solved(S::Plane(g)) => {
                Some(Self::Plane { p: point(g.origin().get()), n: vector(*g.frame().axis().as_raw()), u: vector(*g.frame().reference().as_raw()) })
            }
            SurfaceGeometry::Solved(S::Cylinder(g)) => {
                Some(Self::Rotation { p: point(g.origin().get()), z: vector(*g.frame().axis().as_raw()), r: g.radius().get(), angle: 0. })
            }
            SurfaceGeometry::Solved(S::Cone(g)) if (g.ratio().get() - 1.).abs() < 1e-10 => Some(Self::Rotation {
                p: point(g.origin().get()),
                z: vector(*g.frame().axis().as_raw()),
                r: g.radius().get(),
                angle: g.half_angle().get(),
            }),
            SurfaceGeometry::Solved(S::Torus(g)) if g.major_radius().get() > g.minor_radius().get().abs() => Some(Self::Torus {
                p: point(g.center().get()),
                z: vector(*g.frame().axis().as_raw()),
                major: g.major_radius().get(),
                minor: g.minor_radius().get().abs(),
            }),
            SurfaceGeometry::Solved(S::Sphere(g)) => Some(Self::Sphere { p: point(g.center().get()), radius: g.radius().get().abs() }),
            _ => None,
        }
    }
    pub(in crate::native_cad) fn distance(&self, q: V) -> f64 {
        match self {
            Self::Sphere { p, radius } => (len(sub(q, *p)) - radius).abs(),
            Self::Torus { p, z, major, minor } => {
                let d = sub(q, *p);
                let h = dot(d, *z);
                ((len(sub(d, mul(*z, h))) - major).hypot(h) - minor).abs()
            }
            Self::Plane { p, n, .. } => dot(sub(q, *p), *n).abs(),
            Self::Rotation { p, z, r, angle } => {
                let q = sub(q, *p);
                let h = dot(q, *z);
                (len(sub(q, mul(*z, h))) - (r + h * angle.tan())).abs()
            }
        }
    }
    pub(in crate::native_cad) fn normal(&self, q: V) -> Result<V, String> {
        match self {
            Self::Sphere { p, .. } => unit(sub(q, *p)),
            Self::Torus { p, z, major, .. } => {
                let d = sub(q, *p);
                let radial = unit(sub(d, mul(*z, dot(d, *z))))?;
                unit(sub(d, mul(radial, *major)))
            }
            Self::Plane { n, .. } => Ok(*n),
            Self::Rotation { p, z, angle, .. } => {
                let q = sub(q, *p);
                Ok(sub(mul(unit(sub(q, mul(*z, dot(q, *z))))?, angle.cos()), mul(*z, angle.sin())))
            }
        }
    }
}

/// Intersect a plane with a circular cone/cylinder. Only finite centered conics are
/// handled; tangencies, parabolas and an endpoint on the opposite branch are refused.
pub(in crate::native_cad) fn conic(plane: &Support, quad: &Support, a: V, b: V) -> Result<Option<C>, String> {
    if [a, b].into_iter().flatten().any(|x| !x.is_finite() || x.abs() > 1e12) {
        return Err("SLDPRT: invalid conic endpoint".into());
    }
    if let Support::Plane { p, n, u } = plane {
        let circular = match quad {
            Support::Sphere { p: o, radius } => {
                let height = dot(sub(*p, *o), *n);
                let squared = radius * radius - height * height;
                (squared > 1e-12).then(|| (add(*o, mul(*n, height)), squared.sqrt()))
            }
            Support::Torus { p: o, z, major, minor } if dot(*n, *z).abs() > 1. - 1e-10 => {
                let height = dot(sub(*p, *o), *z);
                let squared = minor * minor - height * height;
                if squared > 1e-12 {
                    let center = add(*o, mul(*z, height));
                    let radial = len(sub(a, center));
                    let offset = squared.sqrt();
                    let radius = if (radial - (major + offset)).abs() < (radial - (major - offset)).abs() { major + offset } else { major - offset };
                    Some((center, radius))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((center, radius)) = circular {
            if [a, b].into_iter().all(|q| plane.distance(q) < 1e-5 && quad.distance(q) < 1e-5 && (len(sub(q, center)) - radius).abs() < 1e-5) {
                return Ok(Some(C::Circle(CircleCurve::try_new(xyz(center), vec(*n), vec(*u), radius)?)));
            }
            return Ok(None);
        }
    }
    let (Support::Plane { p, n, u }, Support::Rotation { p: o, z, r, angle }) = (plane, quad) else { return Ok(None) };
    if [a, b].into_iter().any(|p| plane.distance(p) > 1e-5 || quad.distance(p) > 1e-5) {
        return Ok(None);
    }
    let t = angle.tan();
    if !t.is_finite() {
        return Ok(None);
    }
    let cone = t.abs() > 1e-12;
    let o = if cone { sub(*o, mul(*z, r / t)) } else { *o };
    let k = 1. + if cone { t * t } else { 0. };
    let q = |a: V| sub(a, mul(*z, k * dot(*z, a)));
    let v = cross(*n, *u);
    let w = sub(*p, o);
    let aa = dot(*u, q(*u));
    let bb = dot(*u, q(v));
    let cc = dot(v, q(v));
    let l = [dot(*u, q(w)), dot(v, q(w))];
    let f = dot(w, q(w)) - if cone { 0. } else { r * r };
    let det = aa * cc - bb * bb;
    if !det.is_finite() {
        return Ok(None);
    }
    if det.abs() < 1e-8 {
        let theta = 0.5 * (2. * bb).atan2(aa - cc);
        let (sn, cs) = theta.sin_cos();
        let mut ew = add(mul(*u, cs), mul(v, sn));
        let mut et = add(mul(*u, -sn), mul(v, cs));
        let mut lam = aa * cs * cs + 2. * bb * sn * cs + cc * sn * sn;
        if lam.abs() < 1e-8 {
            std::mem::swap(&mut ew, &mut et);
            lam = aa + cc - lam;
        }
        let lw = dot(ew, q(w));
        let lt = dot(et, q(w));
        if lam.abs() < 1e-8 || lt.abs() < 1e-10 {
            return Ok(None);
        }
        let wc = -lw / lam;
        let tc = -(f - lw * lw / lam) / (2. * lt);
        let origin = add(*p, add(mul(ew, wc), mul(et, tc)));
        let focal = (lt / (2. * lam)).abs();
        let axis = mul(et, (-lam / lt).signum());
        let normal = unit(cross(axis, ew))?;
        return Ok(Some(C::Parabola(ParabolaCurve::try_new(xyz(origin), vec(normal), vec(axis), focal)?)));
    }
    let cx = (-cc * l[0] + bb * l[1]) / det;
    let cy = (bb * l[0] - aa * l[1]) / det;
    let center = add(*p, add(mul(*u, cx), mul(v, cy)));
    let f0 = f + l[0] * cx + l[1] * cy;
    let theta = 0.5 * (2. * bb).atan2(aa - cc);
    let (sn, cs) = theta.sin_cos();
    let mut e1 = add(mul(*u, cs), mul(v, sn));
    let mut e2 = add(mul(*u, -sn), mul(v, cs));
    let mut lam1 = aa * cs * cs + 2. * bb * sn * cs + cc * sn * sn;
    let mut lam2 = aa + cc - lam1;
    if [lam1, lam2, f0].iter().any(|v| !v.is_finite() || v.abs() < 1e-10) {
        return Ok(None);
    }
    let (ra, rb, hyperbola) = if lam1 * lam2 > 0. {
        if -f0 / lam1 <= 0. || -f0 / lam2 <= 0. {
            return Ok(None);
        }
        let (mut a, mut b) = ((-f0 / lam1).sqrt(), (-f0 / lam2).sqrt());
        if a < b {
            std::mem::swap(&mut a, &mut b);
            let old = e1;
            e1 = e2;
            e2 = mul(old, -1.);
        }
        (a, b, false)
    } else {
        if -f0 / lam1 < 0. {
            std::mem::swap(&mut lam1, &mut lam2);
            let old = e1;
            e1 = e2;
            e2 = mul(old, -1.);
        }
        if dot(sub(a, center), e1) < 0. {
            e1 = mul(e1, -1.);
            e2 = mul(e2, -1.);
        }
        if dot(sub(b, center), e1) <= 0. {
            return Ok(None);
        }
        ((-f0 / lam1).sqrt(), (f0 / lam2).sqrt(), true)
    };
    let z = unit(cross(e1, e2))?;
    let curve = if hyperbola {
        C::Hyperbola(HyperbolaCurve::try_new(xyz(center), vec(z), vec(e1), ra, rb)?)
    } else {
        C::Ellipse(EllipseCurve::try_new(xyz(center), vec(z), vec(e1), ra, rb)?)
    };
    Ok(Some(curve))
}

struct Edge {
    start: String,
    end: String,
    curve: String,
    faces: BTreeSet<String>,
}
struct Face {
    surface: Support,
    loops: Vec<String>,
    sense: Sense,
}
struct Model {
    vertices: BTreeMap<String, V>,
    curves: BTreeMap<String, C>,
    edges: BTreeMap<String, Edge>,
    coedges: BTreeMap<String, (String, Sense)>,
    loops: BTreeMap<String, Vec<String>>,
    faces: BTreeMap<String, Face>,
    samples: BTreeMap<String, Vec<(V, V)>>,
    all: Vec<(V, V)>,
    triangles: Vec<(V, V)>,
    work: usize,
}
impl Model {
    fn charge(&mut self, n: usize) -> Result<(), String> {
        self.work = self.work.checked_add(n).filter(|x| *x <= 10_000_000).ok_or("SLDPRT: analytic recovery work limit exceeded")?;
        Ok(())
    }
    fn new(ir: &CadIr) -> Result<Self, String> {
        let m = &ir.model;
        if m.faces.len() > 512
            || m.edges.len() > 4096
            || m.curves.len() > 4096
            || m.surfaces.len() > 512
            || [m.points.len(), m.vertices.len(), m.coedges.len(), m.loops.len()].iter().any(|x| *x > 8192)
        {
            return Err("SLDPRT: analytic recovery size limit exceeded".into());
        }
        if m.bodies.is_empty()
            || m.bodies.iter().any(|b| b.kind != cadmpeg_ir::topology::BodyKind::Solid)
            || m.edges.iter().any(|e| {
                e.param_range().is_some()
                    && !e.curve().is_some_and(|c| {
                        m.curves.iter().find(|g| g.id == *c).is_some_and(|g| matches!(g.geometry, CurveGeometry::Solved(C::Line(_))))
                    })
            })
        {
            return Err("SLDPRT: analytic recovery requires solid bodies and unresolved edge ranges".into());
        }
        let points: BTreeMap<_, _> = m.points.iter().map(|p| (p.id.to_string(), point(p.position().get()))).collect();
        let vertices = m
            .vertices
            .iter()
            .map(|v| Ok((v.id.to_string(), *points.get(v.point.as_str()).ok_or("SLDPRT: missing vertex point")?)))
            .collect::<Result<_, String>>()?;
        let curves = m
            .curves
            .iter()
            .map(|c| match &c.geometry {
                CurveGeometry::Solved(g) => Ok((c.id.to_string(), g.clone())),
                _ => Err("SLDPRT: unresolved analytic boundary".into()),
            })
            .collect::<Result<_, String>>()?;
        let edges = m
            .edges
            .iter()
            .map(|e| {
                Ok((
                    e.id.to_string(),
                    Edge {
                        start: e.start.to_string(),
                        end: e.end.to_string(),
                        curve: e.curve().ok_or("SLDPRT: missing edge curve")?.to_string(),
                        faces: BTreeSet::new(),
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        let coedges = m.coedges.iter().map(|c| (c.id.to_string(), (c.edge.to_string(), c.sense))).collect();
        let loops = m.loops.iter().map(|l| (l.id.to_string(), l.coedges().iter().map(ToString::to_string).collect())).collect();
        let faces = m
            .faces
            .iter()
            .map(|f| {
                Ok((
                    f.id.to_string(),
                    Face {
                        surface: m
                            .surfaces
                            .iter()
                            .find(|s| s.id == f.surface)
                            .and_then(|s| Support::read(&s.geometry))
                            .ok_or("SLDPRT: unsupported analytic face")?,
                        loops: f.loops.iter().map(ToString::to_string).collect(),
                        sense: f.sense,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        let mut model =
            Self { vertices, curves, edges, coedges, loops, faces, samples: BTreeMap::new(), all: Vec::new(), triangles: Vec::new(), work: 0 };
        for l in &m.loops {
            for c in l.coedges() {
                let e = &model.coedges.get(c.as_str()).ok_or("SLDPRT: missing coedge")?.0;
                model.edges.get_mut(e).ok_or("SLDPRT: missing edge")?.faces.insert(l.face.to_string());
            }
        }
        for t in &m.tessellations {
            let positions = t.vertices();
            let normals = t.vertex_normals();
            model.charge(positions.len())?;
            if positions.len() != normals.len() {
                return Err("SLDPRT: display normals are missing".into());
            }
            let samples: Vec<_> = positions.iter().zip(&normals).map(|(p, n)| (point(p.get()), vector(n.get()))).collect();
            if model.all.len().checked_add(samples.len()).is_none_or(|n| n > 100_000) {
                return Err("SLDPRT: display sample limit exceeded".into());
            }
            model.charge(samples.len().saturating_mul(t.faces.len()))?;
            for f in &t.faces {
                model.samples.entry(f.to_string()).or_default().extend_from_slice(&samples);
            }
            let triangles = t.triangles();
            model.charge(triangles.len())?;
            if model.triangles.len().checked_add(triangles.len()).is_none_or(|n| n > 200_000) {
                return Err("SLDPRT: display triangle limit exceeded".into());
            }
            for tri in triangles {
                let mut p = [[0.; 3]; 3];
                for (slot, index) in p.iter_mut().zip(tri) {
                    *slot = point(positions.get(index as usize).ok_or("SLDPRT: invalid display index")?.get());
                }
                let normal = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                if len(normal) > 1e-12 {
                    model.triangles.push((mul(add(add(p[0], p[1]), p[2]), 1. / 3.), unit(normal)?));
                }
            }
            model.all.extend(samples);
        }
        Ok(model)
    }
    fn edge_points(&self, e: &Edge, n: usize) -> Result<Vec<V>, String> {
        let a = *self.vertices.get(&e.start).ok_or("SLDPRT: missing edge start")?;
        let b = *self.vertices.get(&e.end).ok_or("SLDPRT: missing edge end")?;
        let g = self.curves.get(&e.curve).ok_or("SLDPRT: missing curve")?;
        let (o, z, x, ra, rb, hyperbola) = match g {
            C::Circle(g) => (
                point(g.center().get()),
                vector(*g.frame().axis().as_raw()),
                vector(*g.frame().reference().as_raw()),
                g.radius().get(),
                g.radius().get(),
                false,
            ),
            C::Ellipse(g) => (
                point(g.center().get()),
                vector(*g.frame().axis().as_raw()),
                vector(*g.frame().reference().as_raw()),
                g.major_radius().get(),
                g.minor_radius().get(),
                false,
            ),
            C::Hyperbola(g) => (
                point(g.center().get()),
                vector(*g.frame().axis().as_raw()),
                vector(*g.frame().reference().as_raw()),
                g.major_radius().get(),
                g.minor_radius().get(),
                true,
            ),
            C::Line(_) => return Ok(vec![a, b]),
            _ => return Err("SLDPRT: analytic boundary is not reconstructed".into()),
        };
        let y = cross(z, x);
        let angle = |p: V| {
            let q = sub(p, o);
            if hyperbola { (dot(q, y) / rb).asinh() } else { (dot(q, y) / rb).atan2(dot(q, x) / ra) }
        };
        let start = angle(a);
        let span = if hyperbola {
            angle(b) - start
        } else if len(sub(a, b)) < 1e-7 {
            std::f64::consts::TAU
        } else {
            (angle(b) - start).rem_euclid(std::f64::consts::TAU)
        };
        Ok((0..=n)
            .map(|i| {
                let t = start + span * (i as f64) / (n as f64);
                let (s, c) = if hyperbola { (t.sinh(), t.cosh()) } else { t.sin_cos() };
                add(o, add(mul(x, ra * c), mul(y, rb * s)))
            })
            .collect())
    }
    fn polygon(&self, id: &str) -> Result<Vec<V>, String> {
        let mut points = Vec::new();
        for c in self.loops.get(id).ok_or("SLDPRT: missing face loop")? {
            let (e, s) = self.coedges.get(c).ok_or("SLDPRT: missing boundary use")?;
            let mut q = self.edge_points(self.edges.get(e).ok_or("SLDPRT: missing boundary edge")?, 32)?;
            if *s == Sense::Reversed {
                q.reverse();
            }
            q.pop();
            points.extend(q);
        }
        Ok(points)
    }
}
fn area(p: &[V]) -> V {
    let mut a = [0.; 3];
    let Some(o) = p.first() else { return a };
    for (p, q) in p.iter().zip(p.iter().cycle().skip(1)).take(p.len()) {
        a = add(a, cross(sub(*p, *o), sub(*q, *o)));
    }
    mul(a, 0.5)
}

fn inside(poly: &[V], p: V) -> bool {
    let normal = area(poly);
    let axis = if normal[0].abs() > normal[1].abs() && normal[0].abs() > normal[2].abs() {
        0
    } else if normal[1].abs() > normal[2].abs() {
        1
    } else {
        2
    };
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let mut inside = false;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len()) {
        if (a[v] > p[v]) != (b[v] > p[v]) && p[u] < (b[u] - a[u]) * (p[v] - a[v]) / (b[v] - a[v]) + a[u] {
            inside = !inside;
        }
    }
    inside
}

/// Normalize the analytic subset, committing changes only after all evidence is coherent.
/// Closedness, source bounds and volume are independently checked after STEP conversion.
pub(super) fn normalize(ir: &mut CadIr) -> Result<(), String> {
    normalize_with_senses(ir, None)
}

pub(super) fn normalize_with_senses(ir: &mut CadIr, native: Option<&BTreeMap<String, Sense>>) -> Result<(), String> {
    let mut m = Model::new(ir)?;
    let mut replacements = BTreeMap::new();
    for e in m.edges.values() {
        if !matches!(m.curves.get(&e.curve), Some(C::Nurbs(_))) {
            continue;
        }
        let supports = e.faces.iter().filter_map(|f| m.faces.get(f).map(|f| &f.surface)).collect::<Vec<_>>();
        if supports.len() != 2 {
            continue;
        }
        let a = *m.vertices.get(&e.start).ok_or("SLDPRT: missing edge vertex")?;
        let b = *m.vertices.get(&e.end).ok_or("SLDPRT: missing edge vertex")?;
        let a_support = supports.first().ok_or("SLDPRT: missing edge support")?;
        let b_support = supports.get(1).ok_or("SLDPRT: missing edge support")?;
        if let Some(c) = conic(a_support, b_support, a, b)?.or(conic(b_support, a_support, a, b)?)
            && let Some(previous) = replacements.insert(e.curve.clone(), c.clone())
            && previous != c
        {
            return Err("SLDPRT: inconsistent shared intersection curve".into());
        }
    }
    m.curves.extend(replacements);
    m.charge(m.edges.len().saturating_mul(m.all.len()))?;
    let mut reversed = BTreeSet::new();
    for (id, e) in &m.edges {
        let Some(C::Circle(c)) = m.curves.get(&e.curve) else { continue };
        if e.start == e.end {
            continue;
        }
        let samples: Vec<_> = e.faces.iter().filter_map(|f| m.samples.get(f)).flatten().map(|p| p.0).collect();
        let samples = if samples.is_empty() { m.all.iter().map(|p| p.0).collect() } else { samples };
        if super::arc_reversed(
            point(c.center().get()),
            vector(*c.frame().reference().as_raw()),
            vector(*c.frame().axis().as_raw()),
            c.radius().get(),
            *m.vertices.get(&e.start).ok_or("SLDPRT: missing circle start")?,
            *m.vertices.get(&e.end).ok_or("SLDPRT: missing circle end")?,
            &samples,
        )
        .map_err(|error| format!("{error} for {id} ({})", e.curve))?
        {
            reversed.insert(id.clone());
        }
    }
    for id in &reversed {
        let e = m.edges.get_mut(id).ok_or("SLDPRT: missing reversed edge")?;
        std::mem::swap(&mut e.start, &mut e.end);
    }
    for (e, s) in m.coedges.values_mut() {
        if reversed.contains(e) {
            *s = flip(*s);
        }
    }
    m.charge(m.faces.len().saturating_mul(m.all.len().saturating_add(m.triangles.len())))?;
    let mut outer = BTreeMap::new();
    let mut source_signs = BTreeMap::new();
    let mut votes = [0., 0.];
    for (id, f) in &m.faces {
        let polygons = f.loops.iter().map(|l| m.polygon(l)).collect::<Result<Vec<_>, _>>()?;
        let areas = polygons.iter().map(|p| len(area(p))).collect::<Vec<_>>();
        let index = if matches!(f.surface, Support::Plane { .. }) || f.loops.len() == 1 {
            super::outer_loop(&areas).map_err(|e| format!("{e} for {id}: {areas:?}"))?
        } else {
            0
        };
        let loop_id = f.loops.get(index).ok_or("SLDPRT: missing outer loop")?.clone();
        outer.insert(id.clone(), loop_id);
        let mut samples = m.samples.get(id).cloned().unwrap_or_default();
        samples.retain(|(p, _)| f.surface.distance(*p) < 1e-4);
        if samples.is_empty() {
            let bounds = polygons.iter().flatten().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
                (lo, hi)
            });
            let fallback = if matches!(f.surface, Support::Plane { .. }) { &m.triangles } else { &m.all };
            samples = fallback
                .iter()
                .filter(|(p, _)| {
                    (0..3).all(|i| p[i] >= bounds.0[i] - 0.01 && p[i] <= bounds.1[i] + 0.01)
                        && f.surface.distance(*p) < 1e-4
                        && (!matches!(f.surface, Support::Plane { .. })
                            || polygons.iter().enumerate().filter(|(i, _)| *i == index).all(|(_, poly)| inside(poly, *p))
                                && polygons.iter().enumerate().filter(|(i, _)| *i != index).all(|(_, poly)| !inside(poly, *p)))
                })
                .copied()
                .collect();
        }
        let signs = samples.iter().map(|(p, n)| f.surface.normal(*p).map(|q| dot(q, *n))).collect::<Result<Vec<_>, _>>()?;
        let sum = signs.iter().sum::<f64>();
        let native_sign = native.and_then(|s| s.get(id)).copied().map(sign);
        let sg = if let Some(sg) = native_sign {
            if !signs.is_empty() && sum * sg < 0.9 * (signs.len() as f64) {
                return Err(format!("SLDPRT: source and display surface senses disagree for {id}"));
            }
            sg
        } else {
            if signs.is_empty() || sum.abs() < 0.9 * (signs.len() as f64) {
                return Err(format!("SLDPRT: physical normal is unresolved for {id}"));
            }
            if sum > 0. { 1. } else { -1. }
        };
        source_signs.insert(id.clone(), sg);
        if let Support::Plane { n, .. } = f.surface {
            let a = area(polygons.get(index).ok_or("SLDPRT: missing planar boundary")?);
            let weight = len(a);
            votes[usize::from(dot(a, n) * sg < 0.)] += weight;
        }
    }
    if votes[0].min(votes[1]) > 1e-5 * votes[0].max(votes[1]) || votes[0].max(votes[1]) < 1e-9 {
        return Err(format!("SLDPRT: inconsistent shell winding {votes:?}"));
    }
    let shell_sign = if votes[0] > votes[1] { 1. } else { -1. };
    for (id, f) in &mut m.faces {
        f.sense = if source_signs.get(id).ok_or("SLDPRT: missing face normal")? * shell_sign > 0. { Sense::Forward } else { Sense::Reversed };
    }
    let mut requests: BTreeMap<String, bool> = BTreeMap::new();
    for (id, f) in &m.faces {
        let Support::Rotation { p, z, .. } = f.surface else { continue };
        let uses = f.loops.iter().filter_map(|l| m.loops.get(l)).flatten().filter_map(|c| m.coedges.get(c)).collect::<Vec<_>>();
        let mut heights = Vec::new();
        for (e, _) in &uses {
            for q in m.edge_points(m.edges.get(e).ok_or("SLDPRT: missing rotational boundary")?, 8)? {
                heights.push(dot(sub(q, p), z));
            }
        }
        if let Some(samples) = m.samples.get(id) {
            heights.extend(samples.iter().map(|(q, _)| dot(sub(*q, p), z)));
        }
        let lo = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for (e, s) in uses {
            let edge = m.edges.get(e).ok_or("SLDPRT: missing circular boundary")?;
            if edge.start != edge.end {
                continue;
            }
            let Some(C::Circle(c)) = m.curves.get(&edge.curve) else { continue };
            let h = dot(sub(point(c.center().get()), p), z);
            if hi - lo < 1e-7 || (h - lo).abs().min((h - hi).abs()) > 1e-3 {
                return Err("SLDPRT: closed ring has no axial boundary evidence".into());
            }
            let wanted = if (h - lo).abs() < (h - hi).abs() { 1. } else { -1. };
            let reverse = wanted * sign(f.sense) * dot(vector(*c.frame().axis().as_raw()), z) * sign(*s) < 0.;
            if requests.insert(edge.curve.clone(), reverse).is_some_and(|old| old != reverse) {
                return Err("SLDPRT: conflicting full-circle directions".into());
            }
        }
    }
    for (id, reverse) in requests {
        if reverse {
            let Some(C::Circle(c)) = m.curves.get(&id) else { return Err("SLDPRT: missing full circle".into()) };
            let next = CircleCurve::try_new(
                c.center().get(),
                vec(mul(vector(*c.frame().axis().as_raw()), -1.)),
                *c.frame().reference().as_raw(),
                c.radius().get(),
            )?;
            m.curves.insert(id, C::Circle(next));
        }
    }
    // Commit only a fully resolved model. Curves are exact analytic supports, never display triangles.
    for c in &mut ir.model.curves {
        c.geometry = CurveGeometry::Solved(m.curves.remove(c.id.as_str()).ok_or("SLDPRT: missing normalized curve")?);
    }
    for e in &mut ir.model.edges {
        if reversed.contains(e.id.as_str()) {
            std::mem::swap(&mut e.start, &mut e.end);
        }
    }
    for c in &mut ir.model.coedges {
        if reversed.contains(c.edge.as_str()) {
            c.sense = flip(c.sense);
        }
    }
    for f in &mut ir.model.faces {
        let normal = m.faces.get(f.id.as_str()).ok_or("SLDPRT: missing normalized face")?;
        f.sense = normal.sense;
        if !matches!(normal.surface, Support::Plane { .. }) && normal.loops.len() > 1 {
            continue;
        }
        let selected = outer.get(f.id.as_str()).ok_or("SLDPRT: missing classified boundary")?;
        let outer = f.loops.iter().find(|l| l.as_str() == selected).ok_or("SLDPRT: invalid exterior")?.clone();
        let inner = f.loops.iter().filter(|l| **l != outer).cloned().collect();
        f.loops = FaceLoops::Classified { outer, inner };
    }
    Ok(())
}
