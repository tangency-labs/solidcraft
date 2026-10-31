//! Raising a band of a planar face (a plastic part's lip): the band along one of the face's
//! boundary loops, a set width into the face, is pushed out along the face's normal. Built
//! directly from the body's faces (the band's walls continue the walls below, which a boolean
//! union can't join: their faces would coincide).

use solidcraft_geom::{Loop2, Plane, Region2, Ring, Seg2, Vec3};
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::history::{Change, FaceOrigin, FaceProv};
use crate::record::Operand;
use crate::{KernelError, Result, guard};

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("raising a band: {m}"))
}

/// A boundary edge of the face, as its ends and (for an arc) a point halfway.
#[derive(Clone, Copy)]
struct Piece {
    a: Vec3,
    b: Vec3,
    mid: Option<Vec3>,
}

/// The planar face of `body` at `at`: its index and outward normal.
fn face_at(body: &Body, at: Vec3) -> Result<(usize, Vec3)> {
    let size = body.size();
    let mesh = body.tessellate((size * 1e-3).max(1e-3))?;
    let near = mesh
        .triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter_map(|(t, f)| {
            let [a, b, c] = mesh.tri(t)?;
            let n = (b - a).cross(c - a).normalized()?;
            let q = at - n * (at - a).dot(n);
            let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-9);
            inside.then_some(((at - a).dot(n).abs(), *f as usize))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0));
    let (d, f) = near.ok_or_else(|| fail("no face there"))?;
    if d > size * 1e-3 {
        return Err(fail("no face there"));
    }
    guard("rim face", || {
        let face = body.solid.face_iter().nth(f).ok_or_else(|| fail("face"))?;
        let mt::Surface::Plane(pl) = face.oriented_surface() else { return Err(fail("the face must be planar")) };
        let n = pl.normal();
        Ok((f, Vec3::new(n.x, n.y, n.z).normalized().ok_or_else(|| fail("normal"))?))
    })
}

/// The face's loop moved `w` into the face (to the left of its edges, as a face's boundary has
/// its inside on the left), corners mitred where the offsets don't already meet.
fn offset_loop(pieces: &[Piece], n: Vec3, w: f64, tol: f64) -> Result<Vec<Piece>> {
    let left = |a: Vec3, b: Vec3| n.cross(b - a).normalized();
    let mut out: Vec<Piece> = Vec::new();
    for p in pieces {
        out.push(match p.mid {
            None => {
                let l = left(p.a, p.b).ok_or_else(|| fail("zero edge"))?;
                Piece { a: p.a + l * w, b: p.b + l * w, mid: None }
            }
            Some(m) => {
                // Each point moves square to the arc (along its radius), to the left.
                let lm = left(p.a, p.b).ok_or_else(|| fail("arc"))?;
                let c = circle_centre(p.a, m, p.b).ok_or_else(|| fail("arc"))?;
                let r = m.dist(c);
                let shift = if (m - c).dot(lm) >= 0.0 { w } else { -w };
                if r + shift <= tol {
                    return Err(fail("the offset closes an arc"));
                }
                // Use one radial scale for the whole arc. At a semicircle's ends the
                // radial is perpendicular to the chord's left, so its dot product cannot
                // reliably choose the side independently at each end.
                let moved = |q: Vec3| c + (q - c) * ((r + shift) / r);
                Piece { a: moved(p.a), b: moved(p.b), mid: Some(moved(m)) }
            }
        });
    }
    // Joints: where the offsets don't meet (sharp corners of lines), the lines are extended or
    // cut back to their crossing.
    let k = out.len();
    for i in 0..k {
        let j = (i + 1) % k;
        let (Some(pi), Some(pj)) = (out.get(i).copied(), out.get(j).copied()) else { continue };
        if pi.b.dist(pj.a) < tol {
            if let Some(x) = out.get_mut(j) {
                x.a = pi.b;
            }
            continue;
        }
        match (pi.mid, pj.mid) {
            (None, None) => {
                let (d1, d2) = (pi.b - pi.a, pj.b - pj.a);
                let w0 = pj.a - pi.a;
                let cr = d1.cross(d2);
                let den = cr.len2();
                if den < 1e-24 {
                    return Err(fail("parallel edges at a corner"));
                }
                let t = w0.cross(d2).dot(cr) / den;
                let x = pi.a + d1 * t;
                if let Some(e) = out.get_mut(i) {
                    e.b = x;
                }
                if let Some(e) = out.get_mut(j) {
                    e.a = x;
                }
            }
            _ => return Err(fail("an arc meeting another edge at a corner")),
        }
    }
    Ok(out)
}

fn circle_centre(a: Vec3, m: Vec3, b: Vec3) -> Option<Vec3> {
    let (u, v) = (b - a, m - a);
    let w = u.cross(v);
    let d = 2.0 * w.len2();
    if d < 1e-300 {
        return None;
    }
    Some(a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / d))
}

fn pieces(wire: &mt::Wire, size: f64) -> Result<Vec<Piece>> {
    wire.edge_iter()
        .map(|e| {
            let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
            match crate::offset::arc_mid(e, size * 1e-6) {
                Some(mid) => Ok(Piece { a, b, mid }),
                None => Err(fail("edges must be lines or arcs")),
            }
        })
        .collect()
}

/// Signed area ordering including arc midpoints: two semicircles have only two
/// vertices, whose polygon has zero area for both the inner and outer loops.
fn turn(wire: &mt::Wire, n: Vec3) -> f64 {
    use mt::{BoundedCurve, ParametricCurve};
    let ring: Ring<Vec3> = wire
        .edge_iter()
        .flat_map(|e| {
            let c = e.oriented_curve();
            let (a, b) = c.range_tuple();
            [from_p3(e.front().point()), from_p3(c.subs((a + b) * 0.5))]
        })
        .collect();
    ring.area_vector().dot(n)
}

fn rim_loops(wires: &[mt::Wire], n: Vec3) -> Result<(&mt::Wire, &mt::Wire)> {
    let [a, b] = wires else { return Err(fail("the face must have an outer and one inner loop (a rim)")) };
    Ok(if turn(a, n) >= turn(b, n) { (a, b) } else { (b, a) })
}

fn profile(pieces: &[Piece], plane: &Plane) -> Result<Loop2> {
    use std::f64::consts::TAU;
    let segs = pieces
        .iter()
        .map(|p| {
            let (a, b) = (plane.to_local(p.a), plane.to_local(p.b));
            Ok(match p.mid {
                None => Seg2::Line { a, b },
                Some(m) => {
                    let center = plane.to_local(circle_centre(p.a, m, p.b).ok_or_else(|| fail("arc"))?);
                    let mid = plane.to_local(m);
                    let start = (a - center).angle();
                    let end = (b - center).angle();
                    let sweep = (end - start).rem_euclid(TAU);
                    let sweep = if ((mid - center).angle() - start).rem_euclid(TAU) <= sweep { sweep } else { sweep - TAU };
                    Seg2::Arc { center, radius: a.dist(center), start, sweep }
                }
            })
        })
        .collect::<Result<_>>()?;
    Ok(Loop2 { segs })
}

/// Exact profile of a band along a planar rim's chosen edge. `width` reaches into
/// the rim; `free` reaches into its free side so a groove cutter crosses the wall.
/// Lines and circular arcs stay analytic, independent of display tessellation.
pub fn rim_band(body: &Body, at: Vec3, width: f64, free: f64, outside: bool) -> Result<(Plane, Region2)> {
    body.require_brep("a rim band")?;
    if !(width.is_finite() && width > 0.0 && free.is_finite() && free >= 0.0) {
        return Err(KernelError::Invalid("invalid rim band width".into()));
    }
    let (fi, n) = face_at(body, at)?;
    guard("rim band", || {
        let face = body.solid.face_iter().nth(fi).ok_or_else(|| fail("face"))?;
        let wires = face.boundaries();
        let (outer, inner) = rim_loops(&wires, n)?;
        let along = if outside { outer } else { inner };
        let p = pieces(along, body.size())?;
        let tol = (body.size() * 1e-5).max(1e-7);
        let plane = Plane::from_normal(at, n).ok_or_else(|| fail("rim plane"))?;
        let into = profile(&offset_loop(&p, n, width, tol)?, &plane)?;
        let away = profile(&offset_loop(&p, n, -free, tol)?, &plane)?;
        let (outer, inner) = if outside { (away, into) } else { (into, away) };
        Ok((plane, Region2 { outer: outer.ccw(), holes: vec![inner.ccw().reversed()] }))
    })
}

/// Raise the band of the planar face at `at` that runs `width` wide along its outer loop
/// (`outside`) or its inner loop, by `height` along the face's normal.
pub fn raise_band(body: &Body, at: Vec3, width: f64, height: f64, outside: bool) -> Result<Body> {
    body.require_brep("a lip")?;
    if !(width > 1e-6 && height > 1e-6 && width.is_finite() && height.is_finite()) {
        return Err(KernelError::Invalid("the band's width and height must be positive".into()));
    }
    if body.solid.boundaries().len() != 1 {
        return Err(fail("a body in one piece"));
    }
    let size = body.size();
    let tol = (size * 1e-7).max(1e-9) * 100.0;
    let (fi, n) = face_at(body, at)?;
    guard("lip", || {
        let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
        let f = faces.get(fi).ok_or_else(|| fail("face"))?;
        let wires = f.boundaries();
        let (outer, inner) = rim_loops(&wires, n)?;
        let (along, other) = if outside { (outer, inner) } else { (inner, outer) };
        let pieces = pieces(along, size)?;
        let moved = offset_loop(&pieces, n, width, tol)?;
        // The band's other edge, on the face.
        let verts: Vec<mt::Vertex> = moved.iter().map(|p| builder::vertex(p3(p.a))).collect();
        let k = verts.len();
        let mut cedges: Vec<mt::Edge> = Vec::with_capacity(k);
        for (i, p) in moved.iter().enumerate() {
            let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % k)) else { return Err(fail("vertex")) };
            cedges.push(match p.mid {
                None => builder::line(a, b),
                Some(m) => builder::circle_arc(a, b, p3(m)),
            });
        }
        // The loop's edges and the band's edge beside each, by the loop edge's index in the body.
        let operand = Operand::new(body);
        let mut wall_edge: std::collections::HashMap<mt::EdgeID, usize> = std::collections::HashMap::new();
        for (e, ce) in along.edge_iter().zip(&cedges) {
            if let Some(i) = operand.edges.get(&e.id()) {
                wall_edge.insert(e.id(), *i);
                wall_edge.insert(ce.id(), *i);
            }
        }
        let c: mt::Wire = cedges.into();
        // The rest of the face: its other loop, and the band's edge as a loop of its own.
        // (The band's edge runs the face's way round: a hole when the band follows the inner
        // loop, the outer loop when it follows the outer.)
        let rest = mt::Face::try_new(vec![other.clone(), c.clone()], f.oriented_surface()).map_err(|e| fail(&e.to_string()))?;
        // Walls: the face's loop and the band's edge swept out; the band's top.
        let up = v3(n * height);
        let wall_along: mt::Shell = builder::tsweep(along, up);
        let wall_c: mt::Shell = builder::tsweep(&c, up);
        let top_of = |sh: &mt::Shell| -> Vec<mt::Wire> {
            sh.extract_boundaries().into_iter().filter(|w| w.vertex_iter().all(|v| (from_p3(v.point()) - at).dot(n) > height * 0.5)).collect()
        };
        let flipped = |sh: &mt::Shell, yes: bool| -> mt::Shell {
            if yes { sh.face_iter().map(|x| x.inverse()).collect::<Vec<_>>().into() } else { sh.clone() }
        };
        // History (one operand, the body), face by face as gathered below: the other faces
        // kept, the face trimmed to what the band leaves, each wall made along the loop
        // edge it rises from (the band's inner wall along the edge it runs beside), and the
        // band's top an offset of the face. (Flipping a face keeps its edges.)
        let history: Option<Vec<FaceProv>> = crate::record::kept_except(faces.len(), fi)
            .map(Some)
            .chain([Some(crate::record::from_face(0, fi, Change::Modified))])
            .chain(wall_along.face_iter().chain(wall_c.face_iter()).map(|w| {
                let edge = w.absolute_boundaries().iter().flatten().find_map(|e| wall_edge.get(&e.id()))?;
                Some(crate::record::generated(vec![FaceOrigin::Edge { operand: 0, edge: *edge }]))
            }))
            .chain([Some(crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: fi }]))])
            .collect();
        let mut last = String::new();
        for flip in 0..8u32 {
            let (wa, wc) = (flipped(&wall_along, flip & 1 == 1), flipped(&wall_c, flip & 2 == 2));
            let mut tops = top_of(&wa);
            tops.extend(top_of(&wc));
            if tops.len() != 2 {
                return Err(fail("the band's top"));
            }
            // The top closes both walls: its loops run against theirs.
            let tops: Vec<mt::Wire> = tops.iter().map(|w| w.inverse()).collect();
            crate::build::refuse_intersection_curves(&tops, "the lip's top")?;
            let Ok(top) = builder::try_attach_plane(&tops) else { continue };
            let top = if flip & 4 == 4 { top.inverse() } else { top };
            let mut all: Vec<mt::Face> = faces.iter().enumerate().filter(|(i, _)| *i != fi).map(|(_, x)| x.clone()).collect();
            all.push(rest.clone());
            all.extend(wa.face_iter().cloned());
            all.extend(wc.face_iter().cloned());
            all.push(top);
            match Solid::try_new(vec![all.into()]) {
                Ok(s) => {
                    let b = Body::new(s)?;
                    if b.validity().is_empty() {
                        return crate::record::attach(b, std::slice::from_ref(&operand), history.clone(), Default::default());
                    }
                    last = b.validity().join("; ");
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(fail(&last))
    })
}
