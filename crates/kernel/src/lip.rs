//! Raising a band of a planar face (a plastic part's lip): the band along one of the face's
//! boundary loops, a set width into the face, is pushed out along the face's normal. Built
//! directly on the body's schema 4 record (the band's walls continue the walls below, which a
//! boolean union can't join: their faces would coincide).

use solidcraft_geom::brep::{Brep, Curve, Frame, Surface};
use solidcraft_geom::{Loop2, Plane, Region2, Ring, Seg2, Vec3};

use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::{Change, FaceOrigin, FaceProv};
use crate::offset::{Surf, surf_of};
use crate::{KernelError, Result, guard};

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("raising a band: {m}"))
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A boundary edge of the face, as its ends and (for an arc) a point halfway.
#[derive(Clone, Copy)]
struct Piece {
    a: Vec3,
    b: Vec3,
    mid: Option<Vec3>,
}

/// The planar face of `body` at `at`: its index and outward normal.
fn face_at(body: &Body, rec: &Brep, at: Vec3) -> Result<(usize, Vec3)> {
    let size = body.size();
    let mesh = body.mesh(&crate::MeshTolerance::chord((size * 1e-3).max(1e-3)))?;
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
    match surf_of(rec, f) {
        Some(Surf::Plane { n, .. }) => Ok((f, n)),
        _ => Err(fail("the face must be planar")),
    }
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

/// The pieces of a loop (seen from outside the body), from its record edges: lines, and arcs
/// with their middle point.
fn pieces(asm: &Assembly, lp: &[AUse]) -> Result<Vec<Piece>> {
    lp.iter()
        .map(|u| {
            let e = asm.edges.get(u.edge as usize).ok_or_else(|| fail("edge"))?;
            let (s, t) = asm.ends(u).ok_or_else(|| fail("edge"))?;
            let (a, b) = (asm.point(s).ok_or_else(|| fail("vertex"))?, asm.point(t).ok_or_else(|| fail("vertex"))?);
            let [t0, t1] = e.interval;
            match e.curve {
                Curve::Line { .. } => Ok(Piece { a, b, mid: None }),
                Curve::Circle { .. } => Ok(Piece { a, b, mid: Some(asm.edge_point(u.edge, 0.5 * (t0 + t1)).ok_or_else(|| fail("arc"))?) }),
                _ => Err(fail("edges must be lines or arcs")),
            }
        })
        .collect()
}

/// Signed area ordering including arc midpoints: two semicircles have only two
/// vertices, whose polygon has zero area for both the inner and outer loops.
fn turn(pieces: &[Piece], n: Vec3) -> f64 {
    let ring: Ring<Vec3> = pieces.iter().flat_map(|p| [Some(p.a), p.mid]).flatten().collect();
    ring.area_vector().dot(n)
}

/// The face's two loops, outer then inner (a rim), as uses seen from outside.
fn rim_loops(asm: &Assembly, fi: usize, n: Vec3) -> Result<(Vec<AUse>, Vec<AUse>)> {
    let f = asm.faces.get(fi).and_then(Option::as_ref).ok_or_else(|| fail("face"))?;
    let [a, b] = f.loops.as_slice() else { return Err(fail("the face must have an outer and one inner loop (a rim)")) };
    let (ta, tb) = (turn(&pieces(asm, a)?, n), turn(&pieces(asm, b)?, n));
    Ok(if ta >= tb { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) })
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
    let rec = crate::native::brep_record(body)?;
    let (fi, n) = face_at(body, &rec, at)?;
    guard("rim band", || {
        let asm = Assembly::from_record(&rec)?;
        let (outer, inner) = rim_loops(&asm, fi, n)?;
        let along = if outside { outer } else { inner };
        let p = pieces(&asm, &along)?;
        let tol = (body.size() * 1e-5).max(1e-7);
        let plane = Plane::from_normal(at, n).ok_or_else(|| fail("rim plane"))?;
        let into = profile(&offset_loop(&p, n, width, tol)?, &plane)?;
        let away = profile(&offset_loop(&p, n, -free, tol)?, &plane)?;
        let (outer, inner) = if outside { (away, into) } else { (into, away) };
        Ok((plane, Region2 { outer: outer.ccw(), holes: vec![inner.ccw().reversed()] }))
    })
}

/// An edge from vertex `a` to vertex `b` along `p` (a line, or an arc about `n` through its
/// middle point): the use that runs from `a` to `b`.
fn piece_edge(asm: &mut Assembly, a: u32, b: u32, p: &Piece, n: Vec3) -> Result<AUse> {
    let Some(m) = p.mid else { return Ok(AUse { edge: asm.line(a, b)?, forward: true, given: None }) };
    let c = circle_centre(p.a, m, p.b).ok_or_else(|| fail("arc"))?;
    let r = c.dist(p.a);
    let x = (p.a - c).normalized().ok_or_else(|| fail("arc"))?;
    let y = n.cross(x);
    let at = |q: Vec3| (q - c).dot(y).atan2((q - c).dot(x)).rem_euclid(std::f64::consts::TAU);
    let (tm, tb) = (at(m), at(p.b));
    let frame = Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(y) };
    // Counter-clockwise about n from a (angle 0) through the middle to b, or the other way.
    let (interval, ends, forward) = if tm < tb { ([0.0, tb], (a, b), true) } else { ([tb, std::f64::consts::TAU], (b, a), false) };
    Ok(AUse { edge: asm.edge(Curve::Circle { frame, radius: r }, interval, ends.0, ends.1)?, forward, given: None })
}

/// The wall raised along `p` (from `bottom` to `top`, joined by `up_a` and `up_b` from its
/// ends), its outside on the right of `p` seen from `n` (`right`) or on its left.
fn wall(p: &Piece, n: Vec3, bottom: &AUse, top: &AUse, up_a: u32, up_b: u32, right: bool, shell: usize) -> Result<AFace> {
    let mut lp = vec![
        bottom.clone(),
        AUse { edge: up_b, forward: true, given: None },
        AUse { edge: top.edge, forward: !top.forward, given: None },
        AUse { edge: up_a, forward: false, given: None },
    ];
    let (surface, natural_out) = match p.mid {
        None => {
            let d = (p.b - p.a).normalized().ok_or_else(|| fail("zero edge"))?;
            // Natural normal d × n: the right of the piece.
            (Surface::Plane { origin: arr(p.a), u_axis: arr(d), v_axis: arr(n) }, right)
        }
        Some(m) => {
            let c = circle_centre(p.a, m, p.b).ok_or_else(|| fail("arc"))?;
            let x = (p.a - c).normalized().ok_or_else(|| fail("arc"))?;
            let surface = Surface::Cylinder { frame: Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(n.cross(x)) }, radius: c.dist(p.a) };
            // The piece's direction at its middle, and which side of it is away from the axis.
            let along = (p.b - p.a).normalized().ok_or_else(|| fail("arc"))?;
            let rightward = along.cross(n);
            let away = (m - c).dot(rightward) > 0.0;
            (surface, away == right)
        }
    };
    // The loop runs up the piece's right: the outside there. On its left it runs the other way.
    if !right {
        lp.reverse();
        for u in &mut lp {
            u.forward = !u.forward;
        }
    }
    Ok(AFace { surface, natural_out, loops: vec![lp], shell, domain: None })
}

/// Raise the band of the planar face at `at` that runs `width` wide along its outer loop
/// (`outside`) or its inner loop, by `height` along the face's normal.
pub fn raise_band(body: &Body, at: Vec3, width: f64, height: f64, outside: bool) -> Result<Body> {
    body.require_brep("a lip")?;
    if !(width > 1e-6 && height > 1e-6 && width.is_finite() && height.is_finite()) {
        return Err(KernelError::Invalid("the band's width and height must be positive".into()));
    }
    let rec = crate::native::brep_record(body)?;
    if rec.shells.len() != 1 {
        return Err(fail("a body in one piece"));
    }
    let size = body.size();
    let tol = (size * 1e-7).max(1e-9) * 100.0;
    let (fi, n) = face_at(body, &rec, at)?;
    guard("lip", || {
        let mut asm = Assembly::from_record(&rec)?;
        let (outer, inner) = rim_loops(&asm, fi, n)?;
        let (along, other) = if outside { (outer, inner) } else { (inner, outer) };
        let face = asm.faces.get_mut(fi).and_then(Option::take).ok_or_else(|| fail("face"))?;
        let below = pieces(&asm, &along)?;
        let moved = offset_loop(&below, n, width, tol)?;
        let k = moved.len();
        let up = n * height;
        // The band's other edge, on the face, and the band's top edges above both.
        let cverts: Vec<u32> = moved.iter().map(|p| asm.vertex(p.a)).collect::<Result<_>>()?;
        let averts: Vec<u32> = along.iter().map(|u| asm.ends(u).map(|e| e.0).ok_or_else(|| fail("edge"))).collect::<Result<_>>()?;
        let atop: Vec<u32> = below.iter().map(|p| asm.vertex(p.a + up)).collect::<Result<_>>()?;
        let ctop: Vec<u32> = moved.iter().map(|p| asm.vertex(p.a + up)).collect::<Result<_>>()?;
        let lift = |p: &Piece| Piece { a: p.a + up, b: p.b + up, mid: p.mid.map(|m| m + up) };
        let (mut cedges, mut aup, mut cup, mut atops, mut ctops) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..k {
            let j = (i + 1) % k;
            let (Some(&ca), Some(&cb), Some(&ta), Some(&tb), Some(&da), Some(&db), Some(&va)) =
                (cverts.get(i), cverts.get(j), atop.get(i), atop.get(j), ctop.get(i), ctop.get(j), averts.get(i))
            else {
                return Err(fail("vertex"));
            };
            let (Some(pb), Some(pc)) = (below.get(i), moved.get(i)) else { return Err(fail("piece")) };
            cedges.push(piece_edge(&mut asm, ca, cb, pc, n)?);
            atops.push(piece_edge(&mut asm, ta, tb, &lift(pb), n)?);
            ctops.push(piece_edge(&mut asm, da, db, &lift(pc), n)?);
            aup.push(asm.line(va, ta)?);
            cup.push(asm.line(ca, da)?);
        }
        // The rest of the face: its other loop, and the band's edge as a loop of its own.
        // (The band's edge runs the face's way round: a hole when the band follows the inner
        // loop, the outer loop when it follows the outer.)
        let rest_loops = if outside { vec![cedges.clone(), other] } else { vec![other, cedges.clone()] };
        let rest = AFace { surface: face.surface.clone(), natural_out: face.natural_out, loops: rest_loops, shell: face.shell, domain: face.domain };
        // Walls: along the face's loop (the band's outside continues the walls below, on the
        // loop's right) and along the band's edge (its outside on that edge's left).
        let mut walls = Vec::with_capacity(2 * k);
        for (right, (pieces, (bottoms, (tops, ups)))) in [(true, (&below, (&along, (&atops, &aup)))), (false, (&moved, (&cedges, (&ctops, &cup))))] {
            for i in 0..k {
                let j = (i + 1) % k;
                let (Some(p), Some(bottom), Some(top), Some(&ua), Some(&ub)) = (pieces.get(i), bottoms.get(i), tops.get(i), ups.get(i), ups.get(j))
                else {
                    return Err(fail("wall"));
                };
                let bottom = AUse { edge: bottom.edge, forward: bottom.forward, given: None };
                walls.push(wall(p, n, &bottom, top, ua, ub, right, face.shell)?);
            }
        }
        // The band's top: above the face's loop as it runs, above the band's edge the other way.
        let reversed = |l: &[AUse]| -> Vec<AUse> { l.iter().rev().map(|u| AUse { edge: u.edge, forward: !u.forward, given: None }).collect() };
        let top_loops = if outside { vec![atops.clone(), reversed(&ctops)] } else { vec![reversed(&ctops), atops.clone()] };
        let origin = below.first().map(|p| p.a + up).ok_or_else(|| fail("piece"))?;
        let u = n.any_perp();
        let top = AFace {
            surface: Surface::Plane { origin: arr(origin), u_axis: arr(u), v_axis: arr(n.cross(u)) },
            natural_out: true,
            loops: top_loops,
            shell: face.shell,
            domain: None,
        };
        // History (one operand, the body), face by face as gathered below: the other faces
        // kept, the face trimmed to what the band leaves, each wall made along the loop edge it
        // rises from (the band's inner wall along the edge it runs beside), and the band's top
        // an offset of the face.
        let loop_edges: Vec<Option<usize>> = along.iter().map(|u| asm.edges.get(u.edge as usize).and_then(|e| e.source)).collect();
        let mut history: Vec<FaceProv> = crate::record::kept_except(rec.faces.len(), fi).collect();
        history.push(crate::record::from_face(0, fi, Change::Modified));
        for _ in 0..2 {
            for e in &loop_edges {
                let e = e.ok_or_else(|| fail("a loop edge the body doesn't have"))?;
                history.push(crate::record::generated(vec![FaceOrigin::Edge { operand: 0, edge: e }]));
            }
        }
        history.push(crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: fi }]));
        asm.face(rest);
        for w in walls {
            asm.face(w);
        }
        asm.face(top);
        let finished = asm.finish()?;
        let b = crate::assemble::body_with_history(finished, &asm, body, history)?;
        let problems = b.validity();
        if !problems.is_empty() {
            return Err(fail(&problems.join("; ")));
        }
        Ok(b)
    })
}
