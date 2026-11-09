//! B-spline faces that are flat, as the planes they are. A skin through sections makes every wall
//! a B-spline surface, also where the wall is flat (a loft whose corners line up across the
//! sections), and such a face counts as free-form: Convert to Sheet Metal refuses it and nothing
//! downstream takes it for a plane (solidcraft#13331). A B-spline (or NURBS with positive
//! weights) whose control points all lie in one plane lies in that plane, so the face is that
//! plane exactly; its trims, the edges' own curves projected into the plane, are exact too.
use solidcore_record::brep::{Brep, Curve, Curve2, Loop, Surface};

/// How far from the plane a control point may lie, as a fraction of the net's size.
const FLAT: f64 = 1e-9;

type P = [f64; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}
fn scaled(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// A plane's frame: the origin and orthonormal axes, `u × v` the normal.
struct Frame {
    o: P,
    u: P,
    v: P,
    n: P,
}

impl Frame {
    fn uv(&self, p: P) -> [f64; 2] {
        let d = sub(p, self.o);
        [dot(d, self.u), dot(d, self.v)]
    }
    fn off(&self, p: P) -> f64 {
        dot(sub(p, self.o), self.n).abs()
    }
}

/// The control net of a polynomial or rational B-spline surface, as points.
fn net(s: &Surface) -> Option<Vec<Vec<P>>> {
    match s {
        Surface::Spline { points, .. } => Some(points.clone()),
        Surface::RationalSpline { homogeneous, .. } => homogeneous
            .iter()
            .map(|row| row.iter().map(|h| (h[3] > 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])).collect::<Option<Vec<P>>>())
            .collect(),
        _ => None,
    }
}

/// The plane of a net whose points all lie in one, with its normal the way the surface's natural
/// normal (∂S/∂u × ∂S/∂v) points; `None` if the net is not flat or has no clear side.
fn flat_frame(rows: &[Vec<P>]) -> Option<Frame> {
    let pts: Vec<P> = rows.iter().flatten().copied().collect();
    let &o = pts.first()?;
    let size = pts.iter().map(|&q| len(sub(q, o))).fold(0.0, f64::max);
    if !(size > 0.0) {
        return None;
    }
    let a = sub(*pts.iter().max_by(|p, q| len(sub(**p, o)).total_cmp(&len(sub(**q, o))))?, o);
    let c = pts.iter().map(|&q| cross(a, sub(q, o))).max_by(|p, q| len(*p).total_cmp(&len(*q)))?;
    if len(c) <= 1e-6 * size * size {
        return None;
    }
    let mut n = scaled(c, 1.0 / len(c));
    if pts.iter().any(|&q| dot(sub(q, o), n).abs() > FLAT * size) {
        return None;
    }
    // The natural normal's side: the net's cells, each as ∂u × ∂v there.
    let mut side = 0.0;
    for (r0, r1) in rows.iter().zip(rows.iter().skip(1)) {
        for j in 0..r0.len().min(r1.len()).saturating_sub(1) {
            let (p, pu, pv) = (r0[j], r1[j], r0[j + 1]);
            side += dot(cross(sub(pu, p), sub(pv, p)), n);
        }
    }
    if side.abs() <= 1e-6 * size * size {
        return None;
    }
    if side < 0.0 {
        n = scaled(n, -1.0);
    }
    let u = scaled(a, 1.0 / len(a));
    Some(Frame { o, u, v: cross(n, u), n })
}

/// The pcurve of an edge's curve (its NURBS form if it has one) on `interval`, projected into
/// `f`'s plane, with the curve's 3D points within `tol` of it; `None` for any other curve.
fn projected(c: &Curve, interval: [f64; 2], f: &Frame, tol: f64) -> Option<Curve2> {
    let same = |k: &[f64], count: usize| {
        let d = k.len().checked_sub(count + 1)?;
        let (a, b) = (*k.get(d)?, *k.get(count)?);
        let span = (b - a).abs().max(1e-300);
        ((a - interval[0]).abs() <= 1e-9 * span && (b - interval[1]).abs() <= 1e-9 * span).then_some(())
    };
    match c {
        Curve::Line { start, end } => {
            let at = |t: f64| [start[0] + t * (end[0] - start[0]), start[1] + t * (end[1] - start[1]), start[2] + t * (end[2] - start[2])];
            let (a, b) = (at(interval[0]), at(interval[1]));
            (f.off(a) <= tol && f.off(b) <= tol)
                .then(|| Curve2::Spline { knots: vec![interval[0], interval[0], interval[1], interval[1]], points: vec![f.uv(a), f.uv(b)] })
        }
        Curve::Spline { knots, points } => {
            same(knots, points.len())?;
            points
                .iter()
                .all(|&p| f.off(p) <= tol)
                .then(|| Curve2::Spline { knots: knots.clone(), points: points.iter().map(|&p| f.uv(p)).collect() })
        }
        Curve::RationalSpline { knots, homogeneous } => {
            same(knots, homogeneous.len())?;
            let mut out = Vec::with_capacity(homogeneous.len());
            for h in homogeneous {
                if !(h[3] > 0.0) {
                    return None;
                }
                let p = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
                if f.off(p) > tol {
                    return None;
                }
                let [u, v] = f.uv(p);
                out.push([u * h[3], v * h[3], h[3]]);
            }
            Some(Curve2::RationalSpline { knots: knots.clone(), homogeneous: out })
        }
        _ => None,
    }
}

/// `record` with each B-spline face that is flat (its control points in one plane within 10⁻⁹
/// of its size, every edge round it in that plane within the edge's tolerance) made a plane:
/// the same face, edges and loops, the loops' pcurves the edges' curves projected into the
/// plane, whose axes put its natural normal the way the B-spline's pointed, so the loops keep
/// their turning. `None` when no face is flat.
pub(crate) fn flat_splines_as_planes(record: &Brep) -> Option<Brep> {
    let mut out: Option<Brep> = None;
    for (i, face) in record.faces.iter().enumerate() {
        if face.nurbs.is_some() {
            continue;
        }
        let Some(rows) = net(&face.surface) else { continue };
        let Some(frame) = flat_frame(&rows) else { continue };
        let mut loops = Vec::with_capacity(face.loops.len());
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        let mut ok = true;
        'loops: for l in &face.loops {
            let Loop::Edges { edges } = l else {
                loops.push(l.clone());
                continue;
            };
            let mut uses = Vec::with_capacity(edges.len());
            for oe in edges {
                let Some(e) = usize::try_from(oe.edge).ok().and_then(|k| record.edges.get(k)) else {
                    ok = false;
                    break 'loops;
                };
                let Some(pc) = projected(e.nurbs.as_ref().unwrap_or(&e.curve), e.interval, &frame, e.tolerance.max(0.0)) else {
                    ok = false;
                    break 'loops;
                };
                let pts: Vec<[f64; 2]> = match &pc {
                    Curve2::Spline { points, .. } => points.clone(),
                    Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
                };
                for p in pts {
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let mut oe = oe.clone();
                oe.pcurve = pc;
                uses.push(oe);
            }
            loops.push(Loop::Edges { edges: uses });
        }
        if !ok || !(lo[0] < hi[0] && lo[1] < hi[1]) {
            continue;
        }
        let r = out.get_or_insert_with(|| record.clone());
        let Some(f) = r.faces.get_mut(i) else { continue };
        // A margin round the trims: the domain encloses the loops, as the record asks.
        let pad = 1e-9 * (hi[0] - lo[0]).max(hi[1] - lo[1]);
        f.surface = Surface::Plane { origin: frame.o, u_axis: frame.u, v_axis: frame.v };
        f.domain = [[lo[0] - pad, hi[0] + pad], [lo[1] - pad, hi[1] + pad]];
        f.loops = loops;
    }
    out
}
