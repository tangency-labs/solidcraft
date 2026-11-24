//! Raising a band of a planar face (a plastic part's lip): the band along one of the face's
//! boundary loops, a set width into the face, is pushed out along the face's normal. Built
//! directly on the body's schema 4 record (the band's walls continue the walls below, which a
//! boolean union can't join: their faces would coincide).

use solidcore_ops::prism::{LoopPlacement, loop_placement};
use solidcore_record::brep::{Brep, Curve, Frame, Surface};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec3};
use solidcraft_geom::Ring;
use solidcraft_geom::prelude::*;

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

/// A boundary edge of the face, as its ends and, for an arc, its circle: the centre and the
/// angle it turns through about the face's normal from `a` to `b` (positive counter-clockwise;
/// ±2π for a whole circle, whose ends are one point).
#[derive(Clone, Copy)]
struct Piece {
    a: Vec3,
    b: Vec3,
    arc: Option<Arc>,
}

#[derive(Clone, Copy)]
struct Arc {
    centre: Vec3,
    sweep: f64,
}

impl Piece {
    /// The point a fraction `f` of the way along the piece (an arc turning about `n`).
    fn at(&self, n: Vec3, f: f64) -> Vec3 {
        match self.arc {
            None => self.a + (self.b - self.a) * f,
            Some(Arc { centre, sweep }) => {
                let x = self.a - centre;
                let (s, c) = (f * sweep).sin_cos();
                centre + x * c + n.cross(x) * s
            }
        }
    }

    /// The piece moved by `d`.
    fn moved(&self, d: Vec3) -> Piece {
        Piece { a: self.a + d, b: self.b + d, arc: self.arc.map(|x| Arc { centre: x.centre + d, ..x }) }
    }
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
        out.push(match p.arc {
            None => {
                let l = left(p.a, p.b).ok_or_else(|| fail("zero edge"))?;
                p.moved(l * w)
            }
            Some(arc) => {
                // Each point moves along its radius, to the left: towards the centre of an arc
                // turning counter-clockwise, away from that of one turning clockwise.
                let r = p.a.dist(arc.centre);
                let shift = if arc.sweep > 0.0 { -w } else { w };
                if r + shift <= tol {
                    return Err(fail("the offset closes an arc"));
                }
                let moved = |q: Vec3| arc.centre + (q - arc.centre) * ((r + shift) / r);
                Piece { a: moved(p.a), b: moved(p.b), arc: Some(arc) }
            }
        });
    }
    // Joints: where the offsets don't meet (sharp corners), the pieces are extended or cut back to
    // their crossing.
    let k = out.len();
    let mut turned = vec![false; k];
    for i in 0..k {
        let j = (i + 1) % k;
        let (Some(pi), Some(pj)) = (out.get(i).copied(), out.get(j).copied()) else { continue };
        if pi.b.dist(pj.a) < tol {
            if let Some(x) = out.get_mut(j) {
                x.a = pi.b;
            }
            continue;
        }
        match (pi.arc, pj.arc) {
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
            _ => {
                // An arc at the corner: the moved line and circle (or the two moved circles)
                // cross nearest the old corner, and both pieces end there.
                let corner = pieces.get(j).map(|p| p.a).ok_or_else(|| fail("piece"))?;
                let x = carriers_cross(&pi, &pj, n, corner)
                    .ok_or_else(|| fail("an arc meeting another edge at a corner the band's edge can't go round"))?;
                if let Some(e) = out.get_mut(i) {
                    e.b = x;
                }
                if let Some(e) = out.get_mut(j) {
                    e.a = x;
                }
                for m in [i, j] {
                    if let Some(t) = turned.get_mut(m) {
                        *t = true;
                    }
                }
            }
        }
    }
    // An arc cut back or carried on at a corner turns through a new angle, the same way round.
    for (p, _) in out.iter_mut().zip(&turned).filter(|(_, t)| **t) {
        let Some(arc) = p.arc.as_mut() else { continue };
        let (x, y) = (p.a - arc.centre, p.b - arc.centre);
        let tau = std::f64::consts::TAU;
        let mut sweep = n.dot(x.cross(y)).atan2(x.dot(y));
        sweep += ((arc.sweep - sweep) / tau).round() * tau;
        if sweep * arc.sweep <= 0.0 {
            return Err(fail("the offset swallows an arc"));
        }
        arc.sweep = sweep;
    }
    Ok(out)
}

/// Where the carriers of two moved pieces cross (a line's line, an arc's circle, all in the plane
/// with normal `n`), the crossing nearest `near`; `None` when they don't cross.
fn carriers_cross(p: &Piece, q: &Piece, n: Vec3, near: Vec3) -> Option<Vec3> {
    let nearest = |x: Vec3, y: Vec3| if x.dist(near) <= y.dist(near) { x } else { y };
    let x = match (p.arc, q.arc) {
        (None, None) => return None,
        (Some(arc), None) | (None, Some(arc)) => {
            let (line, round) = if p.arc.is_none() { (p, q) } else { (q, p) };
            let d = (line.b - line.a).normalized()?;
            let r = round.a.dist(arc.centre);
            // The foot of the centre on the line, and the half chord beside it: r² − h², with h
            // the centre's distance from the line, factored to keep its digits near tangency.
            let f = arc.centre - line.a;
            let foot = line.a + d * f.dot(d);
            let h = foot.dist(arc.centre);
            let chord2 = (r - h) * (r + h);
            if chord2 < 0.0 {
                return None;
            }
            let s = chord2.sqrt();
            nearest(foot + d * s, foot - d * s)
        }
        (Some(a1), Some(a2)) => {
            let (r1, r2) = (p.a.dist(a1.centre), q.a.dist(a2.centre));
            let gap = a2.centre - a1.centre;
            let apart = gap.len();
            let u = gap.normalized()?;
            // Along u from the first centre to the chord, then half the chord across it.
            let along = (r1 * r1 - r2 * r2 + apart * apart) / (2.0 * apart);
            let half2 = (r1 - along) * (r1 + along);
            if half2 < 0.0 {
                return None;
            }
            let (base, across) = (a1.centre + u * along, n.cross(u) * half2.sqrt());
            nearest(base + across, base - across)
        }
    };
    x.is_finite().then_some(x)
}

/// The pieces of a loop (seen from outside the body, whose normal is `n`), from its record edges:
/// lines, and arcs with their circle.
fn pieces(asm: &Assembly, lp: &[AUse], n: Vec3) -> Result<Vec<Piece>> {
    lp.iter()
        .map(|u| {
            let e = asm.edges.get(u.edge as usize).ok_or_else(|| fail("edge"))?;
            let (s, t) = asm.ends(u).ok_or_else(|| fail("edge"))?;
            let (a, b) = (asm.point(s).ok_or_else(|| fail("vertex"))?, asm.point(t).ok_or_else(|| fail("vertex"))?);
            // The arc's angles, on the circle itself (its NURBS form, if any, is parameterised otherwise).
            let [t0, t1] = solidcore_ops::label::curve_interval(&e.curve, e.nurbs.as_ref(), e.interval).ok_or_else(|| fail("edge"))?;
            match &e.curve {
                Curve::Line { .. } => Ok(Piece { a, b, arc: None }),
                Curve::Circle { frame, .. } => {
                    // The parameter is the angle about the frame's z, which lies along ±n.
                    let z = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis));
                    if z.cross(n).len() > 1e-9 {
                        return Err(fail("an arc not in the face's plane"));
                    }
                    let along = if u.forward { 1.0 } else { -1.0 };
                    let sweep = (t1 - t0) * along * z.dot(n).signum();
                    Ok(Piece { a, b, arc: Some(Arc { centre: Vec3::from(frame.origin), sweep }) })
                }
                Curve::Ellipse { .. } => Err(LipRefusal::RimCurve("an ellipse").into()),
                Curve::Polyline { .. } => Err(LipRefusal::RimCurve("a polyline").into()),
                Curve::Spline { .. } | Curve::RationalSpline { .. } => Err(LipRefusal::RimCurve("a spline").into()),
                Curve::SurfaceIntersection { .. } => Err(LipRefusal::RimCurve("a curve where two surfaces meet").into()),
            }
        })
        .collect()
}

/// Signed area ordering, through points along the arcs: a whole circle has one vertex, two
/// semicircles two, whose polygon has no area.
fn turn(pieces: &[Piece], n: Vec3) -> f64 {
    let ring: Ring<Vec3> =
        pieces.iter().flat_map(|p| if p.arc.is_some() { vec![p.a, p.at(n, 1.0 / 3.0), p.at(n, 2.0 / 3.0)] } else { vec![p.a] }).collect();
    ring.area_vector().dot(n)
}

/// The face's two loops, outer then inner (a rim), as uses seen from outside.
fn rim_loops(asm: &Assembly, fi: usize, n: Vec3) -> Result<(Vec<AUse>, Vec<AUse>)> {
    let f = asm.faces.get(fi).and_then(Option::as_ref).ok_or_else(|| fail("face"))?;
    let [a, b] = f.loops.as_slice() else { return Err(fail("the face must have an outer and one inner loop (a rim)")) };
    let (ta, tb) = (turn(&pieces(asm, a, n)?, n), turn(&pieces(asm, b, n)?, n));
    Ok(if ta >= tb { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) })
}

fn profile(pieces: &[Piece], plane: &Plane) -> Result<Loop2> {
    let mut pieces = pieces.to_vec();
    // A source edge's vertex may be within its own tolerance of the circle rather than on it.
    // An extruded tool has fresh, tighter tolerances, so join the following piece to the
    // circle's evaluated end instead of carrying that source vertex into the tool profile.
    for i in 0..pieces.len() {
        let j = (i + 1) % pieces.len();
        let Some(piece) = pieces.get(i).copied() else { return Err(fail("piece")) };
        if piece.arc.is_some() {
            let end = piece.at(plane.normal(), 1.0);
            if let Some(current) = pieces.get_mut(i) {
                current.b = end;
            }
            if i != j
                && let Some(next) = pieces.get_mut(j)
            {
                next.a = end;
            }
        }
    }
    let segs = pieces
        .iter()
        .map(|p| {
            let (a, b) = (plane.to_local(p.a), plane.to_local(p.b));
            Ok(match p.arc {
                None => Seg2::Line { a, b },
                // The plane's normal is the face's: the arc turns the same way in it.
                Some(arc) => {
                    let center = plane.to_local(arc.centre);
                    Seg2::Arc { center, radius: a.dist(center), start: (a - center).angle(), sweep: arc.sweep }
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
        let p = pieces(&asm, &along, n)?;
        let tol = (body.size() * 1e-5).max(1e-7);
        let plane = Plane::from_normal(at, n).ok_or_else(|| fail("rim plane"))?;
        let into = profile(&offset_loop(&p, n, width, tol)?, &plane)?;
        let away = profile(&offset_loop(&p, n, -free, tol)?, &plane)?;
        let (outer, inner) = if outside { (away, into) } else { (into, away) };
        Ok((plane, Region2 { outer: outer.ccw(), holes: vec![inner.ccw().reversed()] }))
    })
}

/// Why a band cannot be raised where it would leave no sound face beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LipRefusal {
    /// The band is as wide as the rim (to within the model's tolerance): no face would be left.
    #[error("the band is as wide as the rim, so no face is left beside it")]
    BandFillsRim,
    /// The band's far edge crosses the rim's other edge in some places only.
    #[error("the band's far edge crosses the rim's other edge in part, so the face left beside it has no single side")]
    BandCrossesRim,
    /// A rim edge of a curve the band cannot follow, such as the ellipse a tilted cut leaves.
    #[error("not supported yet: a rim edge that is {0}; a lip follows only lines and arcs")]
    RimCurve(&'static str),
}

/// A rim edge's NURBS form moved by `d`.
fn moved_form(c: &Curve, d: Vec3) -> Option<Curve> {
    match c {
        Curve::Spline { knots, points } => {
            Some(Curve::Spline { knots: knots.clone(), points: points.iter().map(|&p| arr(Vec3::from(p) + d)).collect() })
        }
        Curve::RationalSpline { knots, homogeneous } => Some(Curve::RationalSpline {
            knots: knots.clone(),
            homogeneous: homogeneous.iter().map(|&[x, y, z, w]| [x + d.x * w, y + d.y * w, z + d.z * w, w]).collect(),
        }),
        _ => None,
    }
}

/// The surface a rim edge's NURBS form sweeps along `up`: the form along u, from 0 to `up`'s
/// length along v. A wall on it has the rim edge, and the edge above it on the form moved by
/// `up`, as straight lines in its parameters, as an extrusion's side has its profile edges.
fn swept_form(c: &Curve, up: Vec3) -> Option<Surface> {
    let v_knots = vec![0.0, 0.0, up.len(), up.len()];
    match c {
        Curve::Spline { knots, points } => {
            Some(Surface::Spline { u_knots: knots.clone(), v_knots, points: points.iter().map(|&p| vec![p, arr(Vec3::from(p) + up)]).collect() })
        }
        Curve::RationalSpline { knots, homogeneous } => Some(Surface::RationalSpline {
            u_knots: knots.clone(),
            v_knots,
            homogeneous: homogeneous.iter().map(|&q| vec![q, [q[0] + up.x * q[3], q[1] + up.y * q[3], q[2] + up.z * q[3], q[3]]]).collect(),
        }),
        _ => None,
    }
}

/// The edge above rim edge `e` (used `forward` from `a` to `b`), moved by `up` with its NURBS
/// form, when it is an arc that has one: the use from `a` to `b` and the wall's form.
fn form_above(asm: &mut Assembly, e: u32, forward: bool, a: u32, b: u32, up: Vec3) -> Result<Option<(AUse, Surface)>> {
    let edge = asm.edges.get(e as usize).ok_or_else(|| fail("edge"))?;
    let (Curve::Circle { frame, radius }, Some(form)) = (&edge.curve, &edge.nurbs) else { return Ok(None) };
    let (Some(moved), Some(swept)) = (moved_form(form, up), swept_form(form, up)) else { return Ok(None) };
    let frame = Frame { origin: arr(Vec3::from(frame.origin) + up), ..frame.clone() };
    let (curve, interval) = (Curve::Circle { frame, radius: *radius }, edge.interval);
    let (s, t) = if forward { (a, b) } else { (b, a) };
    let top = asm.edge(curve, interval, s, t)?;
    if let Some(x) = asm.edges.get_mut(top as usize) {
        x.nurbs = Some(moved);
    }
    Ok(Some((AUse { edge: top, forward, given: None }, swept)))
}

/// An edge from vertex `a` to vertex `b` along `p` (a line, or an arc of its circle about `n`):
/// the use that runs from `a` to `b`.
fn piece_edge(asm: &mut Assembly, a: u32, b: u32, p: &Piece, n: Vec3) -> Result<AUse> {
    let Some(arc) = p.arc else { return Ok(AUse { edge: asm.line(a, b)?, forward: true, given: None }) };
    let r = arc.centre.dist(p.a);
    let x = (p.a - arc.centre).normalized().ok_or_else(|| fail("arc"))?;
    let frame = Frame { origin: arr(arc.centre), x_axis: arr(x), y_axis: arr(n.cross(x)) };
    // Counter-clockwise about n from a (angle 0) to b, or clockwise: then from b to a.
    let tau = std::f64::consts::TAU;
    let (interval, ends, forward) = if arc.sweep > 0.0 { ([0.0, arc.sweep], (a, b), true) } else { ([tau + arc.sweep, tau], (b, a), false) };
    Ok(AUse { edge: asm.edge(Curve::Circle { frame, radius: r }, interval, ends.0, ends.1)?, forward, given: None })
}

/// The wall raised along `p` (from `bottom` to `top`, joined by `up_a` and `up_b` from its
/// ends), its outside on the right of `p` seen from `n` (`right`) or on its left.
fn wall(p: &Piece, n: Vec3, bottom: &AUse, top: &AUse, up_a: u32, up_b: u32, right: bool, shell: usize, form: Option<Surface>) -> Result<AFace> {
    let mut lp = vec![
        bottom.clone(),
        AUse { edge: up_b, forward: true, given: None },
        AUse { edge: top.edge, forward: !top.forward, given: None },
        AUse { edge: up_a, forward: false, given: None },
    ];
    let (surface, natural_out) = match p.arc {
        None => {
            let d = (p.b - p.a).normalized().ok_or_else(|| fail("zero edge"))?;
            // Natural normal d × n: the right of the piece.
            (Surface::Plane { origin: arr(p.a), u_axis: arr(d), v_axis: arr(n) }, right)
        }
        Some(arc) => {
            let x = (p.a - arc.centre).normalized().ok_or_else(|| fail("arc"))?;
            let surface =
                Surface::Cylinder { frame: Frame { origin: arr(arc.centre), x_axis: arr(x), y_axis: arr(n.cross(x)) }, radius: arc.centre.dist(p.a) };
            // Right of an arc turning counter-clockwise about n is away from its axis.
            (surface, (arc.sweep > 0.0) == right)
        }
    };
    // The loop runs up the piece's right: the outside there. On its left it runs the other way.
    if !right {
        lp.reverse();
        for u in &mut lp {
            u.forward = !u.forward;
        }
    }
    // On the swept form of the edge below, the natural normal is the edge's direction × n: out
    // on the right of a use that runs the edge's way.
    let natural_out = if form.is_some() { bottom.forward == right } else { natural_out };
    Ok(AFace { surface, nurbs: form, natural_out, loops: vec![lp], shell, domain: None })
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
        let below = pieces(&asm, &along, n)?;
        let moved = offset_loop(&below, n, width, tol)?;
        // Where the band's far edge lies against the face's other loop, decided by the kernel from
        // the two loops' exact lines and arcs, loops within `tol` (from the body's size) meeting.
        let plane = Plane::from_normal(at, n).ok_or_else(|| fail("rim plane"))?;
        let (far, other_loop) = (profile(&moved, &plane)?, profile(&pieces(&asm, &other, n)?, &plane)?);
        let placed = loop_placement(&far, &other_loop, tol).ok_or_else(|| fail("where the band's far edge lies"))?;
        // The band's edge follows the outer loop (the other is the hole) or the hole (the other is
        // the outer loop): it clears the other loop on the face's side, or reaches past it.
        let past = match placed {
            LoopPlacement::Coincident => return Err(LipRefusal::BandFillsRim.into()),
            LoopPlacement::Crossing => return Err(LipRefusal::BandCrossesRim.into()),
            LoopPlacement::Inside => outside,
            LoopPlacement::Outside => !outside,
        };
        let k = moved.len();
        let up = n * height;
        // The band's other edge, on the face, and the band's top edges above both.
        let cverts: Vec<u32> = moved.iter().map(|p| asm.vertex(p.a)).collect::<Result<_>>()?;
        let averts: Vec<u32> = along.iter().map(|u| asm.ends(u).map(|e| e.0).ok_or_else(|| fail("edge"))).collect::<Result<_>>()?;
        let atop: Vec<u32> = below.iter().map(|p| asm.vertex(p.a + up)).collect::<Result<_>>()?;
        let ctop: Vec<u32> = moved.iter().map(|p| asm.vertex(p.a + up)).collect::<Result<_>>()?;
        let lift = |p: &Piece| p.moved(up);
        let (mut cedges, mut aup, mut cup, mut atops, mut ctops) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        // The NURBS forms of the walls on arcs of the face's loop that have their own: the wall on
        // the edge's form swept up, so the edge below and the one above lie on it exactly.
        let mut forms: Vec<Option<Surface>> = Vec::with_capacity(k);
        for i in 0..k {
            let j = (i + 1) % k;
            let (Some(&ca), Some(&cb), Some(&ta), Some(&tb), Some(&da), Some(&db), Some(&va)) =
                (cverts.get(i), cverts.get(j), atop.get(i), atop.get(j), ctop.get(i), ctop.get(j), averts.get(i))
            else {
                return Err(fail("vertex"));
            };
            let (Some(pb), Some(pc)) = (below.get(i), moved.get(i)) else { return Err(fail("piece")) };
            cedges.push(piece_edge(&mut asm, ca, cb, pc, n)?);
            let above = match (pb.arc, along.get(i)) {
                (Some(_), Some(u)) => form_above(&mut asm, u.edge, u.forward, ta, tb, up)?,
                _ => None,
            };
            let (top, form) = match above {
                Some((top, form)) => (top, Some(form)),
                None => (piece_edge(&mut asm, ta, tb, &lift(pb), n)?, None),
            };
            atops.push(top);
            forms.push(form);
            ctops.push(piece_edge(&mut asm, da, db, &lift(pc), n)?);
            aup.push(asm.line(va, ta)?);
            cup.push(asm.line(ca, da)?);
        }
        // The rest of the face: its other loop, and the band's edge as a loop of its own.
        // (The band's edge runs the face's way round: a hole when the band follows the inner
        // loop, the outer loop when it follows the outer.) A band that reaches past the other
        // loop leaves the underside of its overhang: facing the other way, the other loop outside.
        let rest_loops = if outside != past { vec![cedges.clone(), other] } else { vec![other, cedges.clone()] };
        let rest = AFace {
            surface: face.surface.clone(),
            nurbs: face.nurbs.clone(),
            natural_out: face.natural_out != past,
            loops: rest_loops,
            shell: face.shell,
            domain: face.domain,
        };
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
                let form = if right { forms.get(i).cloned().flatten() } else { None };
                walls.push(wall(p, n, &bottom, top, ua, ub, right, face.shell, form)?);
            }
        }
        // The band's top: above the face's loop as it runs, above the band's edge the other way.
        let reversed = |l: &[AUse]| -> Vec<AUse> { l.iter().rev().map(|u| AUse { edge: u.edge, forward: !u.forward, given: None }).collect() };
        let top_loops = if outside { vec![atops.clone(), reversed(&ctops)] } else { vec![reversed(&ctops), atops.clone()] };
        let origin = below.first().map(|p| p.a + up).ok_or_else(|| fail("piece"))?;
        let u = n.any_perp();
        let top = AFace {
            surface: Surface::Plane { origin: arr(origin), u_axis: arr(u), v_axis: arr(n.cross(u)) },
            nurbs: None,
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
