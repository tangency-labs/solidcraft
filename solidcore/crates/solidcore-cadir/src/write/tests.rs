//! Records written into CADIR and read back: the kernel's own primitives, cadmpeg's unit cube
//! read and written again, STEP files solidcore's reader tests with, and the operand records of
//! solidcore's kernel cases. A record read back from what was written is the record written, as
//! much of it as CADIR carries ([`assert_same`]); the written model passes cadmpeg's own
//! validation, which maps each pcurve through its face's surface onto its edge's vertices.
//! Carriers are checked against the record's formulas, and broken records are refused with their
//! reason, the model left as it was.

use super::*;
use crate::read_bodies;
use cadmpeg_ir::document::CadIr;
use solidcore::math::det;
use solidcore_record::brep::{Curve2, Frame, Loop as RLoop};

/// A NURBS form's point at `t`, as the kernel evaluates it.
fn point_on(c: &BCurve, t: f64) -> Option<[f64; 3]> {
    let (curve, _) = spline_of(c)?;
    curve.eval(t).map(|(p, _)| [p.x, p.y, p.z])
}

/// `record` written alone into a document whose tolerance is the record's.
fn document(record: &Brep) -> (CadIr, Written) {
    let mut ir = CadIr::empty();
    ir.tolerances.linear = cadmpeg_ir::scalar::PositiveLength::new(record.tolerance).unwrap();
    let w = write_body(&mut ir.model, record, "test:written", Some("part")).unwrap();
    ir.finalize();
    (ir, w)
}

/// cadmpeg's validation findings of `ir` at error severity.
fn errors(ir: &CadIr) -> Vec<String> {
    let report = cadmpeg_ir::validate::validate_neutral(ir, Vec::new()).unwrap();
    report
        .findings
        .iter()
        .filter(|f| f.severity >= cadmpeg_ir::report::Severity::Error)
        .map(|f| format!("{f:?}"))
        .collect()
}

/// `record` written and read back.
fn read_back(record: &Brep) -> Brep {
    let (ir, _) = document(record);
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    assert_eq!(bodies.read.len(), 1);
    bodies.read.into_iter().next().unwrap().solid.brep
}

/// Whether two numbers agree: to `tol` relative to the larger, or absolutely below 1.
fn near(x: f64, y: f64, tol: f64) -> bool {
    (x - y).abs() <= tol * x.abs().max(y.abs()).max(1.0)
}

/// Where JSON values `a` and `b` differ, pushed to `out` (at most 40): structure and every
/// string and flag exactly, numbers to `tol` ([`near`]).
fn differ(a: &serde_json::Value, b: &serde_json::Value, tol: f64, at: &str, out: &mut Vec<String>) {
    use serde_json::Value;
    if out.len() >= 40 {
        return;
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if !near(x, y, tol) {
                out.push(format!("{at}: {x} against {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                differ(x, y, tol, &format!("{at}[{i}]"), out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                match (x.get(k), y.get(k)) {
                    (Some(x), Some(y)) => differ(x, y, tol, &format!("{at}.{k}"), out),
                    _ => out.push(format!("{at}.{k}: on one side only")),
                }
            }
        }
        _ if a != b => out.push(format!("{at}: {a} against {b}")),
        _ => {}
    }
}

// What CADIR carries of a record: its named carriers, points and topology. A record's NURBS
// forms are its holder's (the kernel's, or a reader's), which the reader makes its own of, on its
// own parameters and with its own natural normal: what lies on a form (a face's domain, pcurves
// and side, an edge's interval and its uses' pcurves, a circle's frame) is the reader's too, and
// so are the tolerances it raises from the gaps it measures. Where the reader starts a loop is
// its own choice too (a seam's first use), which numbers the edges and vertices: the record read
// back is compared renumbered as the written one ([`renumbered_like`]), its pcurves by their
// points.

type P = [f64; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P, b: P) -> P {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: P) -> P {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// The point where use `u` of record `b` starts, and where it ends.
fn ends_of(b: &Brep, u: &solidcore_record::brep::OrientedEdge) -> [P; 2] {
    let e = &b.edges[u.edge as usize];
    let at = |k: usize| b.vertices[e.vertices[k] as usize].point;
    if u.forward {
        [at(0), at(1)]
    } else {
        [at(1), at(0)]
    }
}

/// Whether the natural normal of `face`'s NURBS form agrees with its named surface's, as the STEP
/// writer measures it; `None` for a face without a form, or one it has no measure for.
fn form_keeps(face: &solidcore_record::brep::Face) -> Option<bool> {
    let n = face.nurbs.as_ref()?;
    form_map(&face.surface, n, face.domain, Map::IDENTITY).map(|m| m.keeps_normal())
}

/// `read` on `written`'s sides: a face whose form's natural normal is the other way round from
/// its written counterpart's (each holder makes its own form) has its loops reversed and its
/// sides in its shells turned, which keeps the face it is.
fn sided_like(read: &Brep, written: &Brep) -> Brep {
    let mut out = read.clone();
    let mut turn = vec![false; out.faces.len()];
    for (f, (r, w)) in out.faces.iter_mut().zip(&written.faces).enumerate() {
        if let (Some(a), Some(b)) = (form_keeps(r), form_keeps(w))
            && a != b
        {
            turn[f] = true;
            for l in &mut r.loops {
                if let RLoop::Edges { edges } = l {
                    edges.reverse();
                    edges.iter_mut().for_each(|u| u.forward = !u.forward);
                }
            }
        }
    }
    for s in &mut out.shells {
        if let solidcore_record::brep::Shell::Faces { faces, .. } = s {
            for of in faces.iter_mut().filter(|of| turn[of.face as usize]) {
                of.forward = !of.forward;
            }
        }
    }
    out
}

/// `read` numbered as `written`: each edge loop of each face turned to start where `written`'s
/// does (the same sides, from and to the same points), and each edge and vertex given the index
/// of its counterpart, one to one; why not, where the loops do not match so.
fn renumbered_like(read: &Brep, written: &Brep, tol: f64) -> Result<Brep, String> {
    let same = |p: P, q: P| (0..3).all(|i| near(p[i], q[i], tol));
    let mut out = read.clone();
    let mut edges: Vec<Option<u32>> = vec![None; read.edges.len()];
    let mut vertices: Vec<Option<u32>> = vec![None; read.vertices.len()];
    if read.faces.len() != written.faces.len() {
        return Err(format!(
            "{} faces against {}",
            written.faces.len(),
            read.faces.len()
        ));
    }
    for (f, (rf, wf)) in out.faces.iter_mut().zip(&written.faces).enumerate() {
        if rf.loops.len() != wf.loops.len() {
            return Err(format!(
                "face {f}: {} loops against {}",
                wf.loops.len(),
                rf.loops.len()
            ));
        }
        for (li, (rl, wl)) in rf.loops.iter_mut().zip(&wf.loops).enumerate() {
            match (rl, wl) {
                (RLoop::Edges { edges: re }, RLoop::Edges { edges: we }) => {
                    let n = re.len();
                    if n != we.len() {
                        return Err(format!("face {f} loop {li}: {} uses against {n}", we.len()));
                    }
                    let fits = |k: usize| {
                        (0..n).all(|i| {
                            let (r, w) = (&re[(i + k) % n], &we[i]);
                            let ([r0, r1], [w0, w1]) = (ends_of(read, r), ends_of(written, w));
                            r.forward == w.forward
                                && same(r0, w0)
                                && same(r1, w1)
                                && edges[r.edge as usize].is_none_or(|m| m == w.edge)
                        })
                    };
                    let k = (0..n)
                        .find(|k| fits(*k))
                        .ok_or_else(|| format!("face {f} loop {li}: no turn of it matches"))?;
                    re.rotate_left(k);
                    for (r, w) in re.iter().zip(we) {
                        edges[r.edge as usize] = Some(w.edge);
                    }
                }
                (RLoop::Vertex { vertex: r }, RLoop::Vertex { vertex: w }) => {
                    vertices[*r as usize] = Some(*w);
                }
                _ => return Err(format!("face {f} loop {li}: of another kind")),
            }
        }
    }
    for (rs, ws) in out.shells.iter().zip(&written.shells) {
        if let (
            solidcore_record::brep::Shell::Faces { wires: r, .. },
            solidcore_record::brep::Shell::Faces { wires: w, .. },
        ) = (rs, ws)
        {
            for (r, w) in r.iter().zip(w) {
                edges[*r as usize] = Some(*w);
            }
        }
    }
    for (r, m) in edges.iter().enumerate() {
        let m = m.ok_or_else(|| format!("edge {r}: no counterpart"))?;
        for (a, b) in read.edges[r]
            .vertices
            .iter()
            .zip(written.edges[m as usize].vertices)
        {
            vertices[*a as usize] = Some(b);
        }
    }
    let one_to_one = |map: &[Option<u32>], n: usize, what: &str| -> Result<Vec<u32>, String> {
        let map: Vec<u32> = map
            .iter()
            .enumerate()
            .map(|(i, m)| m.ok_or_else(|| format!("{what} {i}: no counterpart")))
            .collect::<Result<_, _>>()?;
        let mut seen = vec![false; n];
        for &m in &map {
            if map.len() != n || std::mem::replace(&mut seen[m as usize], true) {
                return Err(format!("{what}s: not one to one"));
            }
        }
        Ok(map)
    };
    let edges = one_to_one(&edges, written.edges.len(), "edge")?;
    let vertices = one_to_one(&vertices, written.vertices.len(), "vertex")?;
    let mut new_edges = written.edges.clone();
    for (r, &m) in edges.iter().enumerate() {
        let mut e = out.edges[r].clone();
        e.vertices = e.vertices.map(|v| vertices[v as usize]);
        new_edges[m as usize] = e;
    }
    let mut new_vertices = written.vertices.clone();
    for (r, &m) in vertices.iter().enumerate() {
        new_vertices[m as usize] = out.vertices[r].clone();
    }
    out.edges = new_edges;
    out.vertices = new_vertices;
    for face in &mut out.faces {
        for l in &mut face.loops {
            match l {
                RLoop::Edges { edges: es } => {
                    es.iter_mut().for_each(|u| u.edge = edges[u.edge as usize]);
                }
                RLoop::Vertex { vertex } => *vertex = vertices[*vertex as usize],
            }
        }
    }
    for s in &mut out.shells {
        if let solidcore_record::brep::Shell::Faces { wires, .. } = s {
            wires.iter_mut().for_each(|e| *e = edges[*e as usize]);
        }
    }
    Ok(out)
}

/// A circle's or ellipse's locus, whatever its frame: its centre, its axis, its radii and an
/// ellipse's major direction (up to its sign).
fn locus(c: &BCurve) -> Option<serde_json::Value> {
    let (frame, radii) = match c {
        BCurve::Circle { frame, radius } => (frame, [*radius, *radius]),
        BCurve::Ellipse { frame, radii } => (frame, *radii),
        _ => return None,
    };
    let z = unit(cross(frame.x_axis, frame.y_axis));
    let (major, minor) = (radii[0].max(radii[1]), radii[0].min(radii[1]));
    let mut along = if radii[0] >= radii[1] {
        frame.x_axis
    } else {
        frame.y_axis
    };
    if let Some(first) = along.iter().find(|x| x.abs() > 1e-6)
        && *first < 0.0
    {
        along = along.map(|x| -x);
    }
    let along = if major == minor { [0.0; 3] } else { along };
    Some(serde_json::json!({
        "centre": frame.origin, "axis": z, "major": major, "minor": minor, "along": along,
    }))
}

/// Points of a use's pcurve over its edge's `interval`, which compare where its knots and
/// control points are another holder's choice.
fn pcurve_points(c: &Curve2, [t0, t1]: [f64; 2]) -> serde_json::Value {
    let points: Vec<[f64; 2]> = (0..=8)
        .map(|k| {
            let t = t0 + (t1 - t0) * k as f64 / 8.0;
            solidcore_ops::pcurve::eval(c, t).unwrap_or([f64::NAN; 2])
        })
        .collect();
    serde_json::json!(points)
}

/// `record` as JSON, without what CADIR does not carry of `original`.
fn carried(record: &Brep, original: &Brep) -> serde_json::Value {
    let mut v = serde_json::to_value(record).unwrap();
    let formed_edge = |e: u64| original.edges[e as usize].nurbs.is_some();
    for (f, face) in v["faces"].as_array_mut().unwrap().iter_mut().enumerate() {
        let formed = original.faces[f].nurbs.is_some();
        let face = face.as_object_mut().unwrap();
        face.remove("domain");
        if formed {
            face.remove("nurbs");
            // A torus's tube start is where its form starts the tube.
            if let Some(s) = face.get_mut("surface").and_then(|s| s.as_object_mut()) {
                s.remove("tube_start");
            }
        }
        for (l, lv) in face["loops"].as_array_mut().unwrap().iter_mut().enumerate() {
            for (k, u) in lv["edges"].as_array_mut().into_iter().flatten().enumerate() {
                let e = u["edge"].as_u64().unwrap();
                let u = u.as_object_mut().unwrap();
                if formed || formed_edge(e) {
                    u.remove("pcurve");
                } else if let RLoop::Edges { edges } = &record.faces[f].loops[l] {
                    let interval = record.edges[e as usize].interval;
                    u.insert("pcurve".into(), pcurve_points(&edges[k].pcurve, interval));
                }
            }
        }
    }
    for (e, edge) in v["edges"].as_array_mut().unwrap().iter_mut().enumerate() {
        let edge = edge.as_object_mut().unwrap();
        edge.remove("tolerance");
        edge.remove("uses");
        if original.edges[e].nurbs.is_some() {
            edge.remove("nurbs");
            edge.remove("interval");
            if let Some(l) = locus(&record.edges[e].curve) {
                edge.insert("curve".into(), l);
            }
        }
    }
    for vertex in v["vertices"].as_array_mut().unwrap() {
        vertex.as_object_mut().unwrap().remove("tolerance");
    }
    v
}

/// Where the record read back differs from `written`, as much of it as CADIR carries: the same
/// items in the same (canonical) order, the same topology and sides, and every named carrier,
/// interval, pcurve and point the same to `tol` relative.
fn differences(written: &Brep, read: &Brep, tol: f64) -> Vec<String> {
    let read = match renumbered_like(&sided_like(read, written), written, tol) {
        Ok(r) => r,
        Err(m) => return vec![m],
    };
    let (a, b) = (carried(written, written), carried(&read, written));
    let mut all = Vec::new();
    differ(&a, &b, tol, "record", &mut all);
    all.extend(other_arcs(written, &read));
    all
}

/// An arc edge's point at the middle of its interval: on its form where it has one, else on its
/// circle or ellipse; `None` for another edge.
fn arc_middle(e: &solidcore_record::brep::Edge) -> Option<P> {
    let t = 0.5 * (e.interval[0] + e.interval[1]);
    if let Some(form) = &e.nurbs {
        return point_on(form, t);
    }
    let (frame, [rx, ry]) = match &e.curve {
        BCurve::Circle { frame, radius } => (frame, [*radius, *radius]),
        BCurve::Ellipse { frame, radii } => (frame, *radii),
        _ => return None,
    };
    let along = add(
        scaled(rx * det::cos(t), frame.x_axis),
        scaled(ry * det::sin(t), frame.y_axis),
    );
    Some(add(frame.origin, along))
}

/// The circle and ellipse edges of `read` that are the other arc between their vertices than
/// `written`'s: the locus, the vertices and the topology compared (`carried`) are the same for an
/// arc and the arc completing its circle, so each arc's middle is placed against `written`'s
/// chord, from its start vertex to its end about its conic's axis, and the two must lie on one
/// side of it (a conic's two arcs between two points lie each on its own side of their chord).
/// An arc whose middle is within rounding of its chord (a short one) is told from the rest of its
/// circle by its middle's distance from the other's instead. A closed edge is not placed.
fn other_arcs(written: &Brep, read: &Brep) -> Vec<String> {
    let mut out = Vec::new();
    for (e, (w, r)) in written.edges.iter().zip(&read.edges).enumerate() {
        let frame = match &w.curve {
            BCurve::Circle { frame, .. } | BCurve::Ellipse { frame, .. } => frame,
            _ => continue,
        };
        if w.vertices[0] == w.vertices[1] {
            continue;
        }
        let (a, b) = (
            written.vertices[w.vertices[0] as usize].point,
            written.vertices[w.vertices[1] as usize].point,
        );
        let z = unit(cross(frame.x_axis, frame.y_axis));
        let side = |m: P| dot(cross(sub(b, a), sub(m, a)), z);
        let chord = dot(sub(b, a), sub(b, a)).sqrt();
        let small = 1e-9 * chord * chord.max(1.0);
        let (Some(mw), Some(mr)) = (arc_middle(w), arc_middle(r)) else {
            out.push(format!("edge {e}: its arc's middle is not found"));
            continue;
        };
        let (sw, sr) = (side(mw), side(mr));
        // Where both middles are off the chord, their sides tell the arcs apart. Where one is
        // within rounding of it (a short arc), its side is no sign: the arcs are told apart by
        // their middles instead, which on one short arc are within its chord of each other, and
        // on an arc and the rest of its circle are not.
        let other = if sw.abs() > small && sr.abs() > small {
            sw.signum() != sr.signum()
        } else {
            let gap = dot(sub(mw, mr), sub(mw, mr)).sqrt();
            gap > chord + w.tolerance.max(1e-9 * chord.max(1.0))
        };
        if other {
            out.push(format!(
                "edge {e}: read back as the other arc between its vertices ({sw} against {sr})"
            ));
        }
    }
    out
}

fn assert_same(written: &Brep, read: &Brep, tol: f64, what: &str) {
    let all = differences(written, read, tol);
    assert!(
        all.is_empty(),
        "{what}: the record read back differs:\n{}",
        all.join("\n")
    );
}

/// Turn pcurve `c`'s control points by `f` (its homogeneous ones through their points) and its
/// knots by `t`.
fn remap(c: &mut Curve2, f: impl Fn([f64; 2]) -> [f64; 2], t: impl Fn(f64) -> f64) {
    match c {
        Curve2::Spline { knots, points } => {
            knots.iter_mut().for_each(|k| *k = t(*k));
            points.iter_mut().for_each(|p| *p = f(*p));
        }
        Curve2::RationalSpline { knots, homogeneous } => {
            knots.iter_mut().for_each(|k| *k = t(*k));
            for h in homogeneous {
                let [u, v] = f([h[0] / h[2], h[1] / h[2]]);
                *h = [u * h[2], v * h[2], h[2]];
            }
        }
    }
}

/// `record` as CADIR holds it, by CADIR's conventions (cadmpeg-ir's carriers), where they differ
/// from the record's:
/// - a line is `origin + t direction`, the direction a unit vector: a record line on no NURBS
///   form runs `|end - start|` times as fast, its edge's interval and its uses' pcurves' knots
///   scaled with it;
/// - an ellipse's first radius is its major one: one whose second is larger is the same ellipse
///   on its frame turned a quarter turn about its axis, at the parameter a quarter turn behind;
/// - a polyline is the degree-one spline on its own parameter, the reader's form of CADIR's;
/// - a plane is on an orthonormal frame: the record's (u, v) on its axes map linearly onto the
///   frame's, x along the first axis and z along their cross product;
/// - a torus has no tube start: its meridian angle is the record's `v + tube_start`;
/// - a cone of no angle is the cylinder of its radius (the reader reads it so);
/// - a face or edge named by a carrier CADIR has no form for, or one whose side its NURBS form
///   cannot be measured against, or a face the STEP writer keeps on its form (`as_form`), is its
///   NURBS form.
fn as_cadir_holds_it(record: &Brep) -> Brep {
    use solidcore_record::brep::Surface as S;
    let mut out = record.clone();
    let mut edge_maps: Vec<Option<(f64, f64)>> = vec![None; out.edges.len()];
    for (e, edge) in out.edges.iter_mut().enumerate() {
        if let Some(form) = &edge.nurbs {
            if matches!(
                edge.curve,
                BCurve::Polyline { .. } | BCurve::SurfaceIntersection { .. }
            ) {
                edge.curve = form.clone();
                edge.nurbs = None;
            }
            continue;
        }
        match &mut edge.curve {
            BCurve::Line { start, end } => {
                let d = sub(*end, *start);
                let l = dot(d, d).sqrt();
                *end = [
                    start[0] + d[0] / l,
                    start[1] + d[1] / l,
                    start[2] + d[2] / l,
                ];
                edge.interval = edge.interval.map(|t| t * l);
                edge_maps[e] = Some((l, 0.0));
            }
            BCurve::Ellipse { frame, radii } if radii[1] > radii[0] => {
                let x = frame.x_axis;
                frame.x_axis = frame.y_axis;
                frame.y_axis = x.map(|c| -c);
                *radii = [radii[1], radii[0]];
                let q = -std::f64::consts::FRAC_PI_2;
                edge.interval = edge.interval.map(|t| t + q);
                edge_maps[e] = Some((1.0, q));
            }
            BCurve::Polyline { points } => {
                let n = points.len();
                let knots = std::iter::once(0.0)
                    .chain((0..n).map(|i| i as f64))
                    .chain(std::iter::once((n - 1) as f64))
                    .collect();
                edge.curve = BCurve::Spline {
                    knots,
                    points: points.clone(),
                };
            }
            _ => {}
        }
        // An arc of a circle or ellipse starts within its first turn (cadmpeg's validation of
        // an edge's range on one).
        if matches!(edge.curve, BCurve::Circle { .. } | BCurve::Ellipse { .. }) {
            let tau = std::f64::consts::TAU;
            let t0 = edge.interval[0];
            let mut s = -(t0 / tau).floor() * tau;
            if t0 + s >= tau {
                s -= tau;
            }
            edge.interval = edge.interval.map(|t| t + s);
            let (k, q) = edge_maps[e].unwrap_or((1.0, 0.0));
            edge_maps[e] = Some((k, q + s));
        }
    }
    let tolerance = out.tolerance;
    for face in &mut out.faces {
        let formed = face.nurbs.is_some();
        let unmeasured = formed
            && matches!(
                face.surface,
                S::Plane { .. } | S::PlaneThrough { .. } | S::PlacedRevolution { .. }
            );
        if formed && (as_form(face, tolerance) || unmeasured) {
            face.surface = face.nurbs.take().unwrap();
            continue;
        }
        // CADIR's (u, v) as `m (u, v) + offset`.
        let (mut m, mut offset) = ([[1.0, 0.0], [0.0, 1.0]], [0.0, 0.0]);
        match &mut face.surface {
            S::Plane {
                origin,
                u_axis,
                v_axis,
            } if !formed => {
                let (a, b) = (*u_axis, *v_axis);
                let x = unit(a);
                let z = unit(cross(a, b));
                let y = cross(z, x);
                *u_axis = x;
                *v_axis = y;
                m = [[dot(a, x), dot(b, x)], [dot(a, y), dot(b, y)]];
                let _ = origin;
            }
            S::PlaneThrough {
                origin,
                u_point,
                v_point,
            } if !formed => {
                let (a, b) = (sub(*u_point, *origin), sub(*v_point, *origin));
                let x = unit(a);
                let z = unit(cross(a, b));
                let y = cross(z, x);
                m = [[dot(a, x), dot(b, x)], [dot(a, y), dot(b, y)]];
                face.surface = S::Plane {
                    origin: *origin,
                    u_axis: x,
                    v_axis: y,
                };
            }
            S::Torus { tube_start, .. } => {
                let s = *tube_start;
                *tube_start = 0.0;
                if !formed {
                    offset = [0.0, s];
                }
            }
            S::Cone {
                frame,
                radius_at_origin,
                half_angle,
            } if *half_angle == 0.0 => {
                face.surface = S::Cylinder {
                    frame: frame.clone(),
                    radius: *radius_at_origin,
                };
            }
            _ => {}
        }
        for l in &mut face.loops {
            if let RLoop::Edges { edges } = l {
                for u in edges {
                    let (k, q) = edge_maps[u.edge as usize].unwrap_or((1.0, 0.0));
                    let chart = |[u, v]: [f64; 2]| {
                        [
                            m[0][0] * u + m[0][1] * v + offset[0],
                            m[1][0] * u + m[1][1] * v + offset[1],
                        ]
                    };
                    remap(&mut u.pcurve, chart, |t| k * t + q);
                }
            }
        }
    }
    out
}

/// A kernel body's record.
fn record(body: solidcore_ops::Result<solidcore_ops::Body>) -> Brep {
    body.unwrap().to_record().unwrap().0
}

#[test]
fn the_kernels_primitives_read_back_as_written() {
    use solidcore_ops::body::{make_box, make_cone, make_cylinder, make_sphere, make_torus};
    for (what, r) in [
        ("box", record(make_box([1.0, 2.0, 3.0], [10.0, 20.0, 5.0]))),
        (
            "cylinder",
            record(make_cylinder([0.0, 0.0, 0.0], 4.0, 10.0)),
        ),
        ("sphere", record(make_sphere([1.0, 1.0, 1.0], 6.0))),
        ("cone", record(make_cone([0.0, 0.0, 0.0], 5.0, 0.0, 8.0))),
        ("frustum", record(make_cone([0.0, 0.0, 0.0], 5.0, 2.0, 8.0))),
        ("torus", record(make_torus([0.0, 0.0, 0.0], 10.0, 3.0))),
    ] {
        let (ir, _) = document(&r);
        assert_eq!(errors(&ir), Vec::<String>::new(), "{what}");
        assert_same(&as_cadir_holds_it(&r), &read_back(&r), 1e-9, what);
    }
}

/// Every `*.bodies.json.gz` under `dir`, recursively, in path order.
fn corpus(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            corpus(&p, out);
        } else if p.to_string_lossy().ends_with(".bodies.json.gz") {
            out.push(p);
        }
    }
}

/// The record shells of region `k` of what was written, as the model holds them (each shell's id
/// ends in its record index).
fn region_shells(ir: &CadIr, w: &Written, k: usize) -> Vec<u32> {
    let id = &w.regions[k];
    let region = ir.model.regions.iter().find(|r| r.id == *id).unwrap();
    region
        .shells
        .iter()
        .map(|s| s.as_str().rsplit('#').next().unwrap().parse().unwrap())
        .collect()
}

/// The faces of record shells `shells`, the edges their loops use and the vertices of those
/// edges and of their vertex loops: what a region of those shells reads back with.
fn counts(r: &Brep, shells: &[u32]) -> [usize; 3] {
    let (mut faces, mut edges, mut vertices) = (
        0,
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    );
    for &s in shells {
        let BShell::Faces { faces: of, .. } = &r.shells[s as usize] else {
            continue;
        };
        for f in of {
            faces += 1;
            for l in &r.faces[f.face as usize].loops {
                match l {
                    RLoop::Edges { edges: uses } => {
                        for u in uses {
                            edges.insert(u.edge);
                            vertices.extend(r.edges[u.edge as usize].vertices);
                        }
                    }
                    RLoop::Vertex { vertex } => {
                        vertices.insert(*vertex);
                    }
                }
            }
        }
    }
    [faces, edges.len(), vertices.len()]
}

/// Every operand record of solidcore's kernel cases (`crates/solidcore/tests/kernel_cases`, the
/// kernel's own records of fillets, shells, booleans, sweeps, threads and imports) is written,
/// the model passes cadmpeg's validation, and the record read back is the one written, as much
/// as CADIR carries of it ([`differences`]). The reader reads a record whose faces need pcurves
/// it has no closed form for (an edge on a spline surface) only with a kernel's trims, which
/// this crate has none of: those are refused by the reader with that reason (or read as an open
/// surface without the faces so refused, which its notes name, and only that region is spared
/// the comparison); a record of several regions reads as one record each, every region read
/// whole checked against the shells written in it (their counts of shells, voids, faces, edges
/// and vertices).
/// The corpus's files are taken in four shares (by their place in path order), one test each, so
/// the four run side by side.
fn kernel_cases_read_back_as_written(share: usize) {
    use std::io::Read as _;
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases");
    let mut files = Vec::new();
    corpus(&root, &mut files);
    let (mut records, mut compared) = (0usize, 0usize);
    let mut wrong = Vec::new();
    for f in files.iter().skip(share).step_by(4) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(f).unwrap())
            .read_to_string(&mut text)
            .unwrap();
        let bodies: std::collections::BTreeMap<String, Brep> = serde_json::from_str(&text).unwrap();
        for (name, r) in bodies {
            records += 1;
            let what = format!("{}:{name}", f.display());
            let (ir, w) = document(&r);
            assert_eq!(errors(&ir), Vec::<String>::new(), "{what}");
            let bodies = read_bodies(&ir, None);
            // Every written region is accounted for once: read, or refused for want of trims. A
            // region refused for anything else fails the test, whichever region it is.
            let mut seen: Vec<usize> = bodies
                .read
                .iter()
                .map(|r| r.region)
                .chain(bodies.failures.iter().map(|f| f.region))
                .collect();
            seen.sort_unstable();
            assert_eq!(
                seen,
                (0..w.regions.len()).collect::<Vec<_>>(),
                "{what}: {:?}",
                bodies.failures
            );
            for failure in &bodies.failures {
                assert!(
                    failure.message.contains("no exact pcurve"),
                    "{what}: {failure:?}"
                );
            }
            // Every shell of the record is written in one region: a shell the writer left out
            // fails here, whatever the regions written then read.
            let mut written: Vec<u32> = (0..w.regions.len())
                .flat_map(|k| region_shells(&ir, &w, k))
                .collect();
            written.sort_unstable();
            let all: Vec<u32> = (0..u32::try_from(r.shells.len()).unwrap()).collect();
            assert_eq!(written, all, "{what}: the shells written");
            // A region the reader refuses reads again as an open surface of the faces that do
            // read (#18457): the faces it refused for want of trims are left out, which its
            // notes say. That is the same refusal, so that region is not compared; a face left
            // out for any other reason fails the test. Every other region read is checked.
            let refused = |read: &crate::Read| {
                let mut left_out = false;
                for note in read.solid.notes.iter().filter(|n| {
                    n.starts_with("not closed, so its faces came in as an open surface: ")
                        || n.starts_with("left out: ")
                }) {
                    assert!(note.contains("no exact pcurve"), "{what}: {note}");
                    left_out = true;
                }
                left_out
            };
            let whole: Vec<&crate::Read> = bodies.read.iter().filter(|x| !refused(x)).collect();
            if w.regions.len() == 1 {
                // One region read whole: the record as written, all of it CADIR carries.
                let Some(read) = whole.first() else { continue };
                let all = differences(&as_cadir_holds_it(&r), &read.solid.brep, 1e-9);
                compared += 1;
                if !all.is_empty() {
                    wrong.push(format!("{what}: {}", all.join("; ")));
                }
                continue;
            }
            // Several regions: each read whole as written, a solid's outer shell and its voids
            // (each void a void region of its own), a piece of the sheets bounding nothing, with
            // the faces, edges and vertices of its shells.
            let layout = layout(&r).unwrap();
            for read in whole {
                let k = read.region;
                let shells = region_shells(&ir, &w, k);
                let b = &read.solid.brep;
                let voids = b.regions.iter().filter(|r| r.void).count();
                assert_eq!(b.shells.len(), shells.len(), "{what}: region {k}");
                match layout.solids.get(k) {
                    Some(solid) => {
                        assert_eq!(voids, solid.voids.len(), "{what}: region {k}");
                        assert_eq!(b.regions.len(), shells.len() + 1, "{what}: region {k}");
                    }
                    None => {
                        assert_eq!(voids, 0, "{what}: region {k}");
                        assert_eq!(b.regions.len(), 1, "{what}: sheets bound nothing");
                    }
                }
                let [faces, edges, vertices] = counts(&r, &shells);
                assert_eq!(
                    [b.faces.len(), b.edges.len(), b.vertices.len()],
                    [faces, edges, vertices],
                    "{what}: region {k}'s faces, edges and vertices"
                );
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The corpus has over a thousand records, most of them compared (the rest need trims, or
    // have several regions). A share is a quarter of its files, not of its records, so a file
    // added elsewhere in path order moves records between shares: the floor leaves room for that.
    assert!(
        records >= 200 && compared * 10 >= records * 8,
        "{records} {compared}"
    );
}

#[test]
fn the_kernel_cases_records_read_back_as_written_first_share() {
    kernel_cases_read_back_as_written(0);
}

#[test]
fn the_kernel_cases_records_read_back_as_written_second_share() {
    kernel_cases_read_back_as_written(1);
}

#[test]
fn the_kernel_cases_records_read_back_as_written_third_share() {
    kernel_cases_read_back_as_written(2);
}

#[test]
fn the_kernel_cases_records_read_back_as_written_fourth_share() {
    kernel_cases_read_back_as_written(3);
}

/// A record the reader made (cadmpeg's unit cube, and the STEP files solidcore's STEP reader is
/// tested with) reads back as written.
#[test]
fn records_the_reader_made_read_back_as_written() {
    let mut made = Vec::new();
    let cube = read_bodies(&cadmpeg_ir::examples::unit_cube().unwrap(), None);
    assert!(cube.failures.is_empty());
    made.extend(
        cube.read
            .into_iter()
            .map(|r| ("cube".to_string(), r.solid.brep)),
    );
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // Each file with the records it holds: one per MANIFOLD_SOLID_BREP and one per
    // SHELL_BASED_SURFACE_MODEL of a single shell in it (the tetrahedra's files hold a solid and
    // a one-face open shell's model); and the solids refused, for want of the pcurves the reader
    // makes only with a kernel's trims (the fillet's and the rod's: faces on B-spline surfaces,
    // whose edges have no closed-form pcurve, in files that write no PCURVE, and this crate has
    // no trims), so that a file read differently from this fails here rather than leaving the
    // others to pass.
    for (file, records, refused) in [
        (
            "../solidcore-step/tests/fixtures/closed-spline-cylinder.step",
            1,
            0,
        ),
        (
            "../solidcore-step/tests/fixtures/discord-2026-10-30-fillet.step",
            0,
            1,
        ),
        (
            "../solidcore-step/tests/fixtures/rod_doubled_edge.step",
            0,
            1,
        ),
        ("../solidcore-ops/tests/data/assembly_extract.stp", 2, 0),
        ("../step-export/src/tetra_disk.stp", 2, 0),
        (
            "../goldens/src/units/topology_brep_constructor_tetra.stp",
            2,
            0,
        ),
    ] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        let ex = solidcore_record::step::p21::parse(&text).unwrap();
        let read = solidcore_step::topology::read_solids(&ex);
        assert_eq!(
            (read.solids.len(), read.failures.len()),
            (records, refused),
            "{file}: {:?}",
            read.failures
        );
        for f in &read.failures {
            assert!(f.message.starts_with("pcurves failed"), "{file}: {f:?}");
        }
        for s in read.solids {
            made.push((format!("{file} #{}", s.item), s.brep));
        }
    }
    for (what, r) in &made {
        let (ir, _) = document(r);
        assert_eq!(errors(&ir), Vec::<String>::new(), "{what}");
        assert_same(&as_cadir_holds_it(r), &read_back(r), 1e-9, what);
    }
}

fn v3(p: cadmpeg_ir::math::Point3) -> P {
    [p.x, p.y, p.z]
}

fn d3(d: &cadmpeg_ir::units::UnitVector3) -> P {
    let r = d.as_raw();
    [r.x, r.y, r.z]
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(k: f64, a: P) -> P {
    a.map(|x| k * x)
}

fn close(a: P, b: P, tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}

/// The solved surfaces of `ir`.
fn surfaces(ir: &CadIr) -> Vec<&cadmpeg_ir::geometry::SolvedSurfaceGeometry> {
    ir.model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            cadmpeg_ir::geometry::SurfaceGeometry::Solved(g) => Some(g),
            _ => None,
        })
        .collect()
}

/// The kernel's primitives are written on their exact surfaces: a box on six planes facing along
/// the axes, a cylinder, sphere, cone and torus on CADIR's of the dimensions they were made
/// with. Each is one solid body, every record face and edge one CADIR face and edge.
#[test]
fn the_kernels_primitives_are_written_on_their_exact_surfaces() {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry as G;
    use solidcore_ops::body::{make_box, make_cone, make_cylinder, make_sphere, make_torus};
    let written = |r: &Brep| {
        let (ir, w) = document(r);
        assert_eq!(ir.model.bodies.len(), 1);
        assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
        assert_eq!(w.faces.iter().flatten().count(), r.faces.len());
        assert_eq!(w.edges.iter().flatten().count(), r.edges.len());
        assert_eq!(ir.model.faces.len(), r.faces.len());
        assert_eq!(ir.model.edges.len(), r.edges.len());
        assert_eq!(ir.model.vertices.len(), r.vertices.len());
        ir
    };

    let ir = written(&record(make_box([1.0, 2.0, 3.0], [10.0, 20.0, 5.0])));
    let mut normals: Vec<P> = surfaces(&ir)
        .into_iter()
        .map(|g| match g {
            G::Plane(p) => d3(p.frame().axis()).map(|x| x.abs()),
            other => panic!("{other:?}"),
        })
        .collect();
    normals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(normals.len(), 6);
    for (n, axis) in normals
        .chunks(2)
        .zip([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]])
    {
        assert!(n.iter().all(|n| close(*n, axis, 1e-12)), "{normals:?}");
    }

    let ir = written(&record(make_cylinder([0.0, 0.0, 0.0], 4.0, 10.0)));
    let cylinders: Vec<_> = surfaces(&ir)
        .into_iter()
        .filter_map(|g| match g {
            G::Cylinder(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(cylinders.len(), 1);
    let c = cylinders[0];
    assert!((c.radius().get() - 4.0).abs() < 1e-12);
    assert!(close(
        d3(c.frame().axis()).map(f64::abs),
        [0.0, 0.0, 1.0],
        1e-12
    ));
    let o = v3(c.origin().get());
    assert!(o[0].abs() < 1e-12 && o[1].abs() < 1e-12, "{o:?}");

    let ir = written(&record(make_sphere([1.0, 1.0, 1.0], 6.0)));
    let spheres: Vec<_> = surfaces(&ir)
        .into_iter()
        .filter_map(|g| match g {
            G::Sphere(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(spheres.len(), 1);
    assert!((spheres[0].radius().get().abs() - 6.0).abs() < 1e-12);
    assert!(close(v3(spheres[0].center().get()), [1.0; 3], 1e-12));

    let ir = written(&record(make_torus([0.0, 0.0, 0.0], 10.0, 3.0)));
    let tori: Vec<_> = surfaces(&ir)
        .into_iter()
        .filter_map(|g| match g {
            G::Torus(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tori.len(), 1);
    assert!((tori[0].major_radius().get() - 10.0).abs() < 1e-12);
    assert!((tori[0].minor_radius().get().abs() - 3.0).abs() < 1e-12);
    assert!(close(v3(tori[0].center().get()), [0.0; 3], 1e-12));
    assert!(close(
        d3(tori[0].frame().axis()).map(f64::abs),
        [0.0, 0.0, 1.0],
        1e-12
    ));

    // A cone of base radius 5 and height 8 to its apex: `radius + v tan(half_angle)` along the
    // axis from its origin reaches 0 at the apex, (0, 0, 8).
    let ir = written(&record(make_cone([0.0, 0.0, 0.0], 5.0, 0.0, 8.0)));
    let cones: Vec<_> = surfaces(&ir)
        .into_iter()
        .filter_map(|g| match g {
            G::Cone(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(cones.len(), 1);
    let c = cones[0];
    let tan = det::tan(c.half_angle().get());
    assert!((tan.abs() - 5.0 / 8.0).abs() < 1e-12, "{tan}");
    let apex = add(
        v3(c.origin().get()),
        scaled(-c.radius().get() / tan, d3(c.frame().axis())),
    );
    assert!(close(apex, [0.0, 0.0, 8.0], 1e-9), "{apex:?}");
}

/// A record curve is written on an affine image of its parameter: a point of the record's at
/// `t` (by the record's formula) is CADIR's at the parameter the write gives for `t` (by
/// cadmpeg-ir's formula). A line runs on its unit direction, `|end - start|` times as fast; an
/// ellipse whose second radius is the larger is written on its major axis.
#[test]
fn a_curve_is_written_on_an_image_of_its_parameter() {
    use cadmpeg_ir::geometry::SolvedCurveGeometry as G;
    let (start, end) = ([1.0, 1.0, 1.0], [4.0, 5.0, 1.0]);
    let (g, p) = geometry::curve(&BCurve::Line { start, end })
        .unwrap()
        .unwrap();
    let G::Line(l) = g else { panic!("{g:?}") };
    assert!((p.scale - 5.0).abs() < 1e-15 && p.shift == 0.0, "{p:?}");
    for t in [-1.0, 0.0, 0.3, 1.0, 2.5] {
        let record = add(start, scaled(t, sub(end, start)));
        let cadir = add(v3(l.origin().get()), scaled(p.apply(t), d3(&l.direction())));
        assert!(close(record, cadir, 1e-12), "{t}");
    }

    let frame = Frame {
        origin: [1.0, 2.0, 3.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    for radii in [[2.0, 5.0], [5.0, 2.0], [3.0, 3.0]] {
        let c = BCurve::Ellipse {
            frame: frame.clone(),
            radii,
        };
        let (g, p) = geometry::curve(&c).unwrap().unwrap();
        let G::Ellipse(e) = g else { panic!("{g:?}") };
        let (a, b) = (e.major_radius().get(), e.minor_radius().get());
        assert_eq!((a, b), (radii[0].max(radii[1]), radii[0].min(radii[1])));
        let (x, z) = (d3(e.frame().reference()), d3(e.frame().axis()));
        let y = cross(z, x);
        for t in [0.0_f64, 0.4, 1.0, 2.0, 4.0, 6.0] {
            let record = add(
                frame.origin,
                add(
                    scaled(radii[0] * det::cos(t), frame.x_axis),
                    scaled(radii[1] * det::sin(t), frame.y_axis),
                ),
            );
            let s = p.apply(t);
            let cadir = add(
                v3(e.center().get()),
                add(scaled(a * det::cos(s), x), scaled(b * det::sin(s), y)),
            );
            assert!(close(record, cadir, 1e-12), "{radii:?} {t}");
        }
    }
}

/// A record surface is written with an affine image of its (u, v): the record's point at (u, v)
/// (by its formula) is CADIR's at the image (by cadmpeg-ir's). A plane on axes that are neither
/// unit nor square is on the orthonormal frame of its first axis and normal; a torus with a tube
/// start has CADIR's meridian angle moved by it. Neither turns the normal.
#[test]
fn a_surface_is_written_with_an_image_of_its_parameters() {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry as G;
    let (origin, a, b) = ([1.0, 2.0, 3.0], [2.0, 0.0, 0.0], [1.0, 3.0, 0.0]);
    let plane = solidcore_record::brep::Surface::Plane {
        origin,
        u_axis: a,
        v_axis: b,
    };
    let Some((G::Plane(pl), chart)) = geometry::surface(&plane).unwrap() else {
        panic!()
    };
    assert!(chart.keeps_normal());
    let (x, z) = (d3(pl.frame().reference()), d3(pl.frame().axis()));
    let y = cross(z, x);
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (-2.0, 3.5)] {
        let record = add(origin, add(scaled(u, a), scaled(v, b)));
        let [s, t] = chart.apply([u, v]);
        let cadir = add(v3(pl.origin().get()), add(scaled(s, x), scaled(t, y)));
        assert!(close(record, cadir, 1e-12), "{u} {v}");
    }

    let frame = Frame {
        origin: [0.0, 0.0, 1.0],
        x_axis: [0.0, 1.0, 0.0],
        y_axis: [-1.0, 0.0, 0.0],
    };
    let (major, minor, tube_start) = (10.0, 3.0, 0.7);
    let torus = solidcore_record::brep::Surface::Torus {
        frame: frame.clone(),
        major_radius: major,
        minor_radius: minor,
        tube_start,
    };
    let Some((G::Torus(to), chart)) = geometry::surface(&torus).unwrap() else {
        panic!()
    };
    assert!(chart.keeps_normal());
    let (x, z) = (d3(to.frame().reference()), d3(to.frame().axis()));
    let y = cross(z, x);
    let fz = cross(frame.x_axis, frame.y_axis);
    for (u, v) in [(0.0_f64, 0.0_f64), (1.0, 2.0), (4.0, -1.0)] {
        let w = v + tube_start;
        let ring = add(
            scaled(det::cos(u), frame.x_axis),
            scaled(det::sin(u), frame.y_axis),
        );
        let record = add(
            frame.origin,
            add(
                scaled(major + minor * det::cos(w), ring),
                scaled(minor * det::sin(w), fz),
            ),
        );
        let [s, t] = chart.apply([u, v]);
        let (big, small) = (to.major_radius().get(), to.minor_radius().get());
        let ring = add(scaled(det::cos(s), x), scaled(det::sin(s), y));
        let cadir = add(
            v3(to.center().get()),
            add(
                scaled(big + small * det::cos(t), ring),
                scaled(small * det::sin(t), z),
            ),
        );
        assert!(close(record, cadir, 1e-12), "{u} {v}");
    }
}

/// Broken records are refused with the reason, never a panic, and the model is left as it was:
/// an index out of range, a point that is not a number, an empty loop, an identity prefix CADIR
/// does not take, a body the model has already, and a curve CADIR has no exact form for.
#[test]
fn a_broken_record_is_refused_with_its_reason_and_writes_nothing() {
    let good = record(solidcore_ops::body::make_box([0.0; 3], [1.0, 2.0, 3.0]));
    let mut ir = CadIr::empty();
    write_body(&mut ir.model, &good, "test:first", None).unwrap();
    let before = serde_json::to_value(&ir.model).unwrap();
    let refused = |r: &Brep, prefix: &str| {
        let mut model = ir.model.clone();
        let e = write_body(&mut model, r, prefix, None).unwrap_err();
        assert_eq!(serde_json::to_value(&model).unwrap(), before, "{e}");
        e
    };
    let invalid = |e: WriteError| assert!(matches!(e, WriteError::Invalid(_)), "{e:?}");

    let mut r = good.clone();
    r.edges[0].vertices[1] = 99;
    invalid(refused(&r, "test:second"));

    let mut r = good.clone();
    r.vertices[2].point[1] = f64::NAN;
    invalid(refused(&r, "test:second"));

    let mut r = good.clone();
    r.faces[3].loops = vec![RLoop::Edges { edges: Vec::new() }];
    invalid(refused(&r, "test:second"));

    let mut r = good.clone();
    if let solidcore_record::brep::Shell::Faces { faces, .. } = &mut r.shells[0] {
        faces[0].face = 1000;
    }
    invalid(refused(&r, "test:second"));

    let mut r = good.clone();
    r.faces.truncate(2);
    invalid(refused(&r, "test:second"));

    for prefix in ["test", "test:a:b", "test second", ""] {
        let e = refused(&good, prefix);
        assert!(matches!(e, WriteError::Id(_)), "{prefix}: {e:?}");
    }
    let e = refused(&good, "test:first");
    assert!(matches!(e, WriteError::Id(_)), "{e:?}");

    // A traced intersection with no NURBS form has no exact CADIR curve.
    let mut r = good.clone();
    let plane = |n: P| solidcore_record::brep::Surface::Plane {
        origin: [0.0; 3],
        u_axis: if n[0] == 0.0 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        },
        v_axis: cross(
            n,
            if n[0] == 0.0 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            },
        ),
    };
    let BCurve::Line { start, end } = r.edges[0].curve.clone() else {
        panic!("{:?}", r.edges[0].curve)
    };
    let [t0, t1] = r.edges[0].interval;
    r.edges[0].curve = BCurve::SurfaceIntersection {
        surfaces: [
            Box::new(plane([1.0, 0.0, 0.0])),
            Box::new(plane([0.0, 1.0, 0.0])),
        ],
        leader: Box::new(BCurve::Polyline {
            points: vec![
                add(start, scaled(t0, sub(end, start))),
                add(start, scaled(t1, sub(end, start))),
            ],
        }),
        tolerance: 1e-6,
    };
    r.edges[0].interval = [0.0, 1.0];
    let e = refused(&r, "test:second");
    assert!(
        matches!(&e, WriteError::Unsupported(m) if m.contains("intersection")),
        "{e:?}"
    );
}

/// A circle edge held on a rational quadratic arc of `sweep` radians (negative: clockwise) about
/// the standard frame, radius `radius`, from (radius, 0, 0), between two vertices.
fn arc_edge(radius: f64, sweep: f64) -> (solidcore_record::brep::Edge, P, P) {
    let half = sweep / 2.0;
    let (p, q) = (
        [radius, 0.0, 0.0],
        [radius * det::cos(sweep), radius * det::sin(sweep), 0.0],
    );
    let (mid, w) = ([radius, radius * det::tan(half), 0.0], det::cos(half));
    let edge = solidcore_record::brep::Edge {
        curve: BCurve::Circle {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius,
        },
        nurbs: Some(BCurve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [p[0], p[1], p[2], 1.0],
                [mid[0] * w, mid[1] * w, mid[2] * w, w],
                [q[0], q[1], q[2], 1.0],
            ],
        }),
        interval: [0.0, 1.0],
        vertices: [0, 1],
        tolerance: 1e-6,
        uses: Vec::new(),
    };
    (edge, p, q)
}

/// A circle edge held on a NURBS arc is written on the arc its form runs along: a short arc as
/// short as it is (not a whole turn), a counter-clockwise one on its own interval, and one whose
/// form runs clockwise about the circle's frame (which no CADIR circle interval can say) as its
/// form.
#[test]
fn a_circle_edge_on_its_form_keeps_the_arc_the_form_runs_along() {
    use std::f64::consts::FRAC_PI_2;
    let (edge, p, q) = arc_edge(10.0, FRAC_PI_2);
    let [a0, a1] = named_interval(&edge, p, q).unwrap();
    assert!(
        a0.abs() < 1e-12 && (a1 - FRAC_PI_2).abs() < 1e-12,
        "{a0} {a1}"
    );

    let (edge, p, q) = arc_edge(10.0, -FRAC_PI_2);
    assert_eq!(named_interval(&edge, p, q), None);

    let (edge, p, q) = arc_edge(1e9, 5e-12);
    let [a0, a1] = named_interval(&edge, p, q).unwrap();
    assert!(a1 - a0 > 0.0 && a1 - a0 < 1e-11, "{a0} {a1}");
    // Its ends are its vertices, within their tolerance.
    for (a, x) in [(a0, p), (a1, q)] {
        let on = [1e9 * det::cos(a), 1e9 * det::sin(a), 0.0];
        let gap = (det::powi(on[0] - x[0], 2) + det::powi(on[1] - x[1], 2)).sqrt();
        assert!(gap < 1e-6, "{a}: {gap}");
    }

    // Written: the clockwise arc is its form, the counter-clockwise one its circle.
    for (sweep, circle) in [(FRAC_PI_2, true), (-FRAC_PI_2, false)] {
        let (edge, _, q) = arc_edge(10.0, sweep);
        assert_eq!(written_as_circle(edge, q), circle, "{sweep}");
    }
}

/// A closed circle edge whose form winds twice about the circle has no named range that keeps
/// both turns: it is its form (`named_interval` gives none). Winding once, it is the whole turn.
#[test]
fn a_closed_circle_edge_winding_twice_is_written_as_its_form() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
    // `turns` turns of the unit circle as rational quadratic quarter arcs, from (1, 0, 0).
    let wound = |turns: usize| {
        let quarters = 4 * turns;
        let at = |a: f64, k: f64| [k * det::cos(a), k * det::sin(a), 0.0];
        let w = det::cos(FRAC_PI_4);
        let mut homogeneous = Vec::new();
        for k in 0..quarters {
            let a = k as f64 * FRAC_PI_2;
            let p = at(a, 1.0);
            let m = at(a + FRAC_PI_4, 1.0 / w);
            homogeneous.push([p[0], p[1], p[2], 1.0]);
            homogeneous.push([m[0] * w, m[1] * w, m[2] * w, w]);
        }
        homogeneous.push([1.0, 0.0, 0.0, 1.0]);
        let mut knots = vec![0.0; 3];
        for k in 1..quarters {
            knots.extend([k as f64; 2]);
        }
        knots.extend([quarters as f64; 3]);
        solidcore_record::brep::Edge {
            curve: BCurve::Circle {
                frame: Frame {
                    origin: [0.0; 3],
                    x_axis: [1.0, 0.0, 0.0],
                    y_axis: [0.0, 1.0, 0.0],
                },
                radius: 1.0,
            },
            nurbs: Some(BCurve::RationalSpline { knots, homogeneous }),
            interval: [0.0, quarters as f64],
            vertices: [0, 0],
            tolerance: 1e-6,
            uses: Vec::new(),
        }
    };
    let p = [1.0, 0.0, 0.0];
    let [a0, a1] = named_interval(&wound(1), p, p).unwrap();
    assert!(a0.abs() < 1e-12 && (a1 - TAU).abs() < 1e-12, "{a0} {a1}");
    assert_eq!(named_interval(&wound(2), p, p), None);
}

/// Whether edge `edge` (from (10, 0, 0) to `q`) is written as a circle, not as its form: written
/// by [`write_body`] as a wire edge of a valid record (an open box well away from it), so the
/// record's validation (coordinates and weights within 1e12) holds for it too.
fn written_as_circle(edge: solidcore_record::brep::Edge, q: P) -> bool {
    written_on_circle(edge, [10.0, 0.0, 0.0], q).unwrap()
}

/// Whether edge `edge` (from `p` to `q`) is written as a circle, not as its form, as
/// [`written_as_circle`] writes it; why not, when the record is refused.
fn written_on_circle(edge: solidcore_record::brep::Edge, p: P, q: P) -> Result<bool, WriteError> {
    let (geometry, _) = written_edge(edge, p, q)?;
    Ok(matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
    ))
}

/// Edge `edge` (from `p` to `q`) as [`written_on_circle`] writes it: its CADIR curve and the
/// range written on it; why not, when the record is refused.
fn written_edge(
    mut edge: solidcore_record::brep::Edge,
    p: P,
    q: P,
) -> Result<(CurveGeometry, Option<[f64; 2]>), WriteError> {
    let mut r = open_box([100.0, 0.0, 0.0]);
    let v = u32::try_from(r.vertices.len()).unwrap();
    for point in [p, q] {
        r.vertices.push(solidcore_record::brep::Vertex {
            point,
            tolerance: 1e-6,
        });
    }
    edge.vertices = [v, v + 1];
    let e = r.edges.len();
    r.edges.push(edge);
    if let BShell::Faces { wires, .. } = &mut r.shells[0] {
        wires.push(u32::try_from(e).unwrap());
    }
    r.validate_built().unwrap();
    let mut model = Model::default();
    let w = write_body(&mut model, &r, "test:arc", None)?;
    assert!(w.edges[e].is_some());
    let curve = format!("test:arc:curve#{e}");
    let written = model
        .curves
        .iter()
        .find(|c| c.id.as_str() == curve)
        .unwrap();
    let id = w.edges[e].clone().unwrap();
    let range = model
        .edges
        .iter()
        .find(|x| x.id == id)
        .unwrap()
        .param_range()
        .map(|r| r.get());
    Ok((written.geometry.clone(), range))
}

/// The range edge `edge` (from `p` to `q`) is written on, where it is written on its circle.
fn circle_range(edge: solidcore_record::brep::Edge, p: P, q: P) -> Option<[f64; 2]> {
    match written_edge(edge, p, q).unwrap() {
        (CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)), range) => range,
        _ => None,
    }
}

/// Whether the angle interval `got` is `want` moved by whole turns, within `tol` at each end.
fn same_arc(got: [f64; 2], want: [f64; 2], tol: f64) -> bool {
    use std::f64::consts::TAU;
    let turns = ((got[0] - want[0]) / TAU).round() * TAU;
    (got[0] - want[0] - turns).abs() <= tol && (got[1] - want[1] - turns).abs() <= tol
}

/// A line edge held on its NURBS form whose named line runs the other way, from its end vertex
/// to its start, is written on that line turned: from the start vertex to the end, on the
/// interval between them (0 to the edge's 10 mm length), so the edge's curve runs as its coedges'
/// senses take it. A line named the edge's way is written as named.
#[test]
fn a_line_named_against_its_edge_is_written_turned() {
    let (p, q) = ([0.0, -20.0, 0.0], [10.0, -20.0, 0.0]);
    let line_edge = |start: P, end: P| solidcore_record::brep::Edge {
        curve: BCurve::Line { start, end },
        nurbs: Some(BCurve::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![p, q],
        }),
        interval: [0.0, 1.0],
        vertices: [0, 1],
        tolerance: 1e-6,
        uses: Vec::new(),
    };
    for (start, end) in [(q, p), (p, q)] {
        let (geometry, range) = written_edge(line_edge(start, end), p, q).unwrap();
        let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = geometry else {
            panic!("not a line: {geometry:?}")
        };
        let range =
            range.unwrap_or_else(|| panic!("no range on the line named {start:?} to {end:?}"));
        let (o, d) = (v3(line.origin().get()), d3(&line.direction()));
        let at = |t: f64| add(o, scaled(t, d));
        assert!(range[0] < range[1], "{range:?}");
        assert!(
            close(at(range[0]), p, 1e-12) && close(at(range[1]), q, 1e-12),
            "{range:?} on {o:?} + t {d:?}"
        );
    }
}

/// A circle edge whose form turns most of its arc in a sliver of its parameter is written on
/// the arc the form runs along: a 270° arc of three quarter arcs, the first two on 0.01 of its
/// parameter each and the last reparameterized (its weights times 1, 50, 50²) to turn most of
/// its quarter early. A counter-clockwise one is written on its circle, every point of the form
/// inside the interval written; a clockwise one (whose ends are those of a counter-clockwise
/// quarter arc, which 16 even samples of it once read it as) as its form.
#[test]
fn a_circle_edge_on_an_unevenly_parameterized_form_keeps_its_arc() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
    let uneven = |turn: f64| {
        let (r, w) = (10.0, det::cos(FRAC_PI_4));
        let at = |a: f64, k: f64, w: f64| [k * det::cos(a) * w, k * det::sin(a) * w, 0.0, w];
        let mut homogeneous = vec![at(0.0, r, 1.0)];
        for k in 0..3 {
            let a = turn * FRAC_PI_2 * k as f64;
            homogeneous.push(at(a + turn * FRAC_PI_4, r / w, w));
            homogeneous.push(at(a + turn * FRAC_PI_2, r, 1.0));
        }
        let c: f64 = 50.0;
        for (h, k) in homogeneous[5..].iter_mut().zip([c, c * c]) {
            h.iter_mut().for_each(|x| *x *= k);
        }
        let q = [0.0, -turn * r, 0.0];
        let edge = solidcore_record::brep::Edge {
            curve: BCurve::Circle {
                frame: Frame {
                    origin: [0.0; 3],
                    x_axis: [1.0, 0.0, 0.0],
                    y_axis: [0.0, 1.0, 0.0],
                },
                radius: r,
            },
            nurbs: Some(BCurve::RationalSpline {
                knots: vec![0.0, 0.0, 0.0, 0.01, 0.01, 0.02, 0.02, 1.0, 1.0, 1.0],
                homogeneous,
            }),
            interval: [0.0, 1.0],
            vertices: [0, 1],
            tolerance: 1e-6,
            uses: Vec::new(),
        };
        (edge, [r, 0.0, 0.0], q)
    };
    let angle = |x: P| det::atan2(x[1], x[0]);

    // Most of the turn is in the first 1/16 of the parameter.
    let (edge, p, q) = uneven(1.0);
    let form = edge.nurbs.clone().unwrap();
    let early = point_on(&form, 1.0 / 16.0).unwrap();
    assert!(angle(early).rem_euclid(TAU) > PI, "{early:?}");

    let [a0, a1] = named_interval(&edge, p, q).unwrap();
    assert!(a0.abs() < 1e-9 && (a1 - 1.5 * PI).abs() < 1e-9, "{a0} {a1}");
    for k in 1..200 {
        let t = det::powi(k as f64 / 200.0, 3);
        let x = point_on(&form, t).unwrap();
        let a = angle(x).rem_euclid(TAU);
        assert!(a0 - 1e-9 <= a && a <= a1 + 1e-9, "{t}: {x:?} at {a}");
    }
    // Written on its circle, on that interval (moved by whole turns to start within the first).
    let range = circle_range(edge, p, q).expect("written on its circle");
    assert!(
        same_arc(range, [a0, a1], 1e-9),
        "{range:?} is not {a0} {a1}"
    );

    let (edge, p, q) = uneven(-1.0);
    let form = edge.nurbs.clone().unwrap();
    let middle = point_on(&form, 0.03).unwrap();
    assert!(angle(middle) > std::f64::consts::FRAC_PI_2, "{middle:?}");
    assert_eq!(named_interval(&edge, p, q), None);
    assert!(!written_as_circle(edge, q));
}

/// A circle edge whose form turns 330° in the first thousandth of its parameter is written on
/// the arc it runs along, though the form's points at its 16 even steps (and their middles) step
/// the short way, 30° in all. The form is one degree-six rational Bézier piece, (z / |z|)⁶ for z
/// running straight from 1 to e^(±55°i), reparameterized (weights times 1000^i) so z is most of
/// the way there by t = 1/1001, and every coefficient scaled by 1e-8 (the same curve) so its
/// weights, 1e-8 to 1e10, are within the record's limits. Counter-clockwise it is written as its
/// circle on [0, 11π/6], clockwise (whose ends are those of a counter-clockwise 30° arc) as its
/// form, each through `write_body`: its pieces' rounding bounded, though its coefficients span
/// eighteen orders of magnitude.
#[test]
fn a_circle_edge_whose_form_turns_between_its_samples_keeps_its_arc() {
    use std::f64::consts::PI;
    let r = 10.0;
    let form = |sign: f64| {
        let e = sign * 55f64.to_radians();
        // |z|² is a quadratic Bézier with coefficients 1, cos 55°, 1; |z|⁶ its cube.
        let times = |a: &[f64], b: &[f64]| {
            let (m, n) = (a.len() - 1, b.len() - 1);
            let choose =
                |n: usize, k: usize| (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64);
            (0..=m + n)
                .map(|k| {
                    (0..=m)
                        .filter(|i| k >= *i && k - i <= n)
                        .map(|i| choose(m, i) * choose(n, k - i) * a[i] * b[k - i])
                        .sum::<f64>()
                        / choose(m + n, k)
                })
                .collect::<Vec<_>>()
        };
        let square = [1.0, det::cos(e), 1.0];
        let w = times(&times(&square, &square), &square);
        let c: f64 = 1000.0;
        (0..7)
            .map(|i| {
                let k = 1e-8 * det::powi(c, i as i32);
                let a = e * i as f64;
                [r * det::cos(a) * k, r * det::sin(a) * k, 0.0, w[i] * k]
            })
            .collect::<Vec<_>>()
    };
    let edge = |sign: f64| {
        // (z / |z|)⁶ ends at ±330°, at ∓30°.
        let q = [
            r * det::cos(-sign * PI / 6.0),
            r * det::sin(-sign * PI / 6.0),
            0.0,
        ];
        let edge = solidcore_record::brep::Edge {
            curve: BCurve::Circle {
                frame: Frame {
                    origin: [0.0; 3],
                    x_axis: [1.0, 0.0, 0.0],
                    y_axis: [0.0, 1.0, 0.0],
                },
                radius: r,
            },
            nurbs: Some(BCurve::RationalSpline {
                knots: vec![
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
                ],
                homogeneous: form(sign),
            }),
            interval: [0.0, 1.0],
            vertices: [0, 1],
            tolerance: 1e-6,
            uses: Vec::new(),
        };
        (edge, [r, 0.0, 0.0], q)
    };
    let angle = |x: P| det::atan2(x[1], x[0]);

    // Clockwise: at t = 1/1001 it is at -165°, outside the 30° arc its ends make.
    let (cw, p, q) = edge(-1.0);
    let x = point_on(cw.nurbs.as_ref().unwrap(), 1.0 / 1001.0).unwrap();
    assert!((angle(x) + 165f64.to_radians()).abs() < 1e-9, "{x:?}");
    assert!((angle(q) - PI / 6.0).abs() < 1e-12, "{q:?}");
    assert_eq!(named_interval(&cw, p, q), None);
    assert!(!written_as_circle(cw, q));

    let (ccw, p, q) = edge(1.0);
    let [a0, a1] = named_interval(&ccw, p, q).unwrap();
    assert!(
        a0.abs() < 1e-12 && (a1 - 11.0 * PI / 6.0).abs() < 1e-12,
        "{a0} {a1}"
    );
    let range = circle_range(ccw, p, q).expect("written on its circle");
    assert!(
        same_arc(range, [a0, a1], 1e-9),
        "{range:?} is not {a0} {a1}"
    );
}

/// A circle edge whose form's rounding is bounded within its turn's proof is written on its arc;
/// one where it is not is written as its form. A quarter arc of radius 1e-3 near the origin, or of
/// radius 1 a million away, keeps its arc; the same small arc 1e11 away, where a coordinate's ulp
/// (1.5e-5) is a hundredth of the radius and the pieces' rounding no longer bounds their angle
/// to the margin the proof takes, is its form (`named_interval` gives none).
#[test]
fn a_circle_edge_whose_forms_rounding_cannot_be_bounded_is_written_as_its_form() {
    use std::f64::consts::FRAC_PI_2;
    let moved = |radius: f64, by: f64| {
        let (mut edge, p, q) = arc_edge(radius, FRAC_PI_2);
        if let BCurve::Circle { frame, .. } = &mut edge.curve {
            frame.origin[0] += by;
        }
        if let Some(BCurve::RationalSpline { homogeneous, .. }) = &mut edge.nurbs {
            homogeneous.iter_mut().for_each(|h| h[0] += h[3] * by);
        }
        (edge, [p[0] + by, p[1], p[2]], [q[0] + by, q[1], q[2]])
    };
    for (radius, by, kept) in [(1e-3, 0.0, true), (1.0, 1e6, true), (1e-3, 1e11, false)] {
        let (edge, p, q) = moved(radius, by);
        let interval = named_interval(&edge, p, q);
        assert_eq!(interval.is_some(), kept, "{radius} at {by}: {interval:?}");
        if let Some([a0, a1]) = interval {
            assert!(
                a0.abs() < 1e-6 && (a1 - FRAC_PI_2).abs() < 1e-6,
                "{radius} at {by}: {a0} {a1}"
            );
        }
    }
}

/// A pcurve whose knots start a remap's rounding inside its edge's interval (which the record's
/// check allows) is bound to a range inside its own knots, which CADIR checks it against.
#[test]
fn a_pcurve_is_bound_to_a_range_inside_its_own_knots() {
    let mut r = record(solidcore_ops::body::make_box([0.0; 3], [1.0, 2.0, 3.0]));
    let mut moved = 0;
    for face in &mut r.faces {
        for l in &mut face.loops {
            let RLoop::Edges { edges } = l else { continue };
            for u in edges {
                let Curve2::Spline { knots, points } = &mut u.pcurve else {
                    continue;
                };
                let degree = knots.len() - points.len() - 1;
                let [t0, t1] = [knots[degree], knots[points.len()]];
                let shift = 5e-10 * t0.abs().max(t1.abs()).max(1.0);
                for k in knots.iter_mut().take(degree + 1) {
                    *k += shift;
                }
                moved += 1;
            }
        }
    }
    assert!(moved > 0);
    r.validate_built().unwrap();
    let (ir, _) = document(&r);
    // Each use's range is written, so cadmpeg checks it against its pcurve's knots (it skips a
    // use with none).
    let uses: Vec<_> = ir.model.coedges.iter().flat_map(|c| &c.pcurves).collect();
    assert!(!uses.is_empty());
    assert!(uses.iter().all(|u| u.parameter_range.is_some()), "{uses:?}");
    assert_eq!(errors(&ir), Vec::<String>::new());
}

/// Records `parts` in one: each part's shells, faces, edges and vertices after the parts before
/// it, and `regions` over the shells.
fn merged(parts: &[&Brep], regions: Vec<solidcore_record::brep::Region>) -> Brep {
    let mut out = Brep {
        version: parts[0].version,
        tolerance: parts[0].tolerance,
        regions,
        shells: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
    };
    for r in parts {
        let at = |n: usize| u32::try_from(n).unwrap();
        let (fo, eo, vo) = (
            at(out.faces.len()),
            at(out.edges.len()),
            at(out.vertices.len()),
        );
        out.vertices.extend(r.vertices.iter().cloned());
        for e in &r.edges {
            let mut e = e.clone();
            e.vertices = e.vertices.map(|v| v + vo);
            for u in &mut e.uses {
                u.face += fo;
            }
            out.edges.push(e);
        }
        for f in &r.faces {
            let mut f = f.clone();
            for l in &mut f.loops {
                match l {
                    RLoop::Edges { edges } => edges.iter_mut().for_each(|u| u.edge += eo),
                    RLoop::Vertex { vertex } => *vertex += vo,
                }
            }
            out.faces.push(f);
        }
        for s in &r.shells {
            let mut s = s.clone();
            if let BShell::Faces { faces, wires } = &mut s {
                faces.iter_mut().for_each(|f| f.face += fo);
                wires.iter_mut().for_each(|w| *w += eo);
            }
            out.shells.push(s);
        }
    }
    out
}

/// A box without its last face: an open sheet of five.
fn open_box(origin: [f64; 3]) -> Brep {
    let mut r = record(solidcore_ops::body::make_box(origin, [1.0, 1.0, 1.0]));
    let last = u32::try_from(r.faces.len() - 1).unwrap();
    r.faces.pop();
    if let BShell::Faces { faces, .. } = &mut r.shells[0] {
        faces.retain(|f| f.face != last);
    }
    for e in &mut r.edges {
        e.uses.retain(|u| u.face != last);
    }
    r.regions = vec![solidcore_record::brep::Region {
        shells: vec![0],
        void: false,
    }];
    r.validate_built().unwrap();
    r
}

/// A general body (a hollow box beside a sheet) reads back as its two regions, each as written:
/// the box with its cavity a void, its outer shell first, and the sheet open.
#[test]
fn a_hollow_solid_beside_a_sheet_reads_back_with_its_void() {
    use solidcore_ops::body::make_box;
    use solidcore_ops::boolean::{BooleanOp, boolean};
    use solidcore_record::brep::Region;
    let hollow = boolean(
        make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
        make_box([3.0; 3], [4.0, 4.0, 4.0]).unwrap(),
        BooleanOp::Subtract,
    );
    let hollow = record(hollow.map(|b| b.unwrap()));
    assert_eq!(hollow.regions.iter().filter(|r| r.void).count(), 1);
    let sheet = open_box([20.0, 0.0, 0.0]);
    let region = |shells: Vec<u32>, void| Region { shells, void };
    // The sheet first, as the canonical order walks the infinite region's shells.
    let both = merged(
        &[&sheet, &hollow],
        vec![
            region(vec![0, 1], false),
            region(vec![1, 2], false),
            region(vec![2], true),
        ],
    );
    both.validate_built().unwrap();
    let (ir, w) = document(&both);
    assert_eq!(errors(&ir), Vec::<String>::new());
    assert_eq!(w.regions.len(), 2);
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    let read: Vec<&Brep> = bodies.read.iter().map(|r| &r.solid.brep).collect();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].regions.iter().filter(|r| r.void).count(), 1);
    assert_same(&as_cadir_holds_it(&hollow), read[0], 1e-9, "the hollow box");
    assert_eq!(read[1].regions.len(), 1, "the sheet bounds nothing");
    assert_same(&as_cadir_holds_it(&sheet), read[1], 1e-9, "the sheet");
}

/// Face `face` of `ir` on a plane moved 5 mm along its normal, off its edges: the reader
/// refuses its region, naming the face.
fn plane_moved_off(ir: &mut CadIr, face: &cadmpeg_ir::ids::FaceId) {
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::math::{Point3, Vector3};
    let surface = ir
        .model
        .faces
        .iter()
        .find(|f| f.id == *face)
        .unwrap()
        .surface
        .clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|s| s.id == surface)
        .unwrap();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("{:?}", surface.geometry)
    };
    let o = plane.origin().get();
    let (n, u) = (
        plane.frame().axis().as_raw(),
        plane.frame().reference().as_raw(),
    );
    let moved = PlaneSurface::try_new(
        Point3::new(o.x + 5.0 * n.x, o.y + 5.0 * n.y, o.z + 5.0 * n.z),
        Vector3::new(n.x, n.y, n.z),
        Vector3::new(u.x, u.y, u.z),
    )
    .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(moved));
}

/// The first face of record shell `s`, as written.
fn first_face_of(r: &Brep, w: &Written, s: usize) -> cadmpeg_ir::ids::FaceId {
    let BShell::Faces { faces, .. } = &r.shells[s] else {
        panic!("shell {s}")
    };
    w.faces[faces[0].face as usize].clone().unwrap()
}

/// A general body's hollow box whose outer shell has a face the reader refuses comes in as a
/// solid with that face left out: its outer shell an open surface of the faces that read, and its
/// cavity, closed still, a void (as a solid body's does), not material.
#[test]
fn a_hollow_solid_of_a_general_body_missing_a_face_keeps_its_cavity_a_void() {
    use solidcore_ops::body::make_box;
    use solidcore_ops::boolean::{BooleanOp, boolean};
    use solidcore_record::brep::Region;
    let hollow = boolean(
        make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
        make_box([3.0; 3], [4.0, 4.0, 4.0]).unwrap(),
        BooleanOp::Subtract,
    );
    let hollow = record(hollow.map(|b| b.unwrap()));
    let sheet = open_box([20.0, 0.0, 0.0]);
    let region = |shells: Vec<u32>, void| Region { shells, void };
    let both = merged(
        &[&sheet, &hollow],
        vec![
            region(vec![0, 1], false),
            region(vec![1, 2], false),
            region(vec![2], true),
        ],
    );
    let (mut ir, w) = document(&both);
    // Shell 1 is the hollow box's outer shell.
    plane_moved_off(&mut ir, &first_face_of(&both, &w, 1));
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    let read = bodies.read.iter().find(|r| r.region == 0).unwrap();
    let s = &read.solid;
    assert!(
        s.notes.iter().any(|n| n.starts_with("not closed")),
        "{:?}",
        s.notes
    );
    assert_eq!(
        s.brep.faces.len(),
        5 + 6,
        "the outer shell less its face, and the cavity"
    );
    assert_eq!(s.brep.shells.len(), 2);
    let voids: Vec<&Region> = s.brep.regions.iter().filter(|r| r.void).collect();
    assert_eq!(voids.len(), 1, "{:?}", s.brep.regions);
    assert_eq!(
        s.brep.regions.len(),
        2,
        "the open surface's, and the cavity's"
    );
}

/// The same with a whole sphere for its cavity: one face with no loops (CADIR's form for a face
/// of a closed surface), whose seam only the reader makes. The cavity is closed on the retry as on
/// the first read, so it is still a void, not material.
#[test]
fn a_general_bodys_whole_sphere_cavity_stays_a_void_when_a_face_is_left_out() {
    use cadmpeg_ir::topology::FaceLoops;
    use solidcore_ops::body::{make_box, make_sphere};
    use solidcore_ops::boolean::{BooleanOp, boolean};
    use solidcore_record::brep::Region;
    let hollow = boolean(
        make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
        make_sphere([5.0; 3], 2.0).unwrap(),
        BooleanOp::Subtract,
    );
    let hollow = record(hollow.map(|b| b.unwrap()));
    assert_eq!(hollow.regions.iter().filter(|r| r.void).count(), 1);
    let sheet = open_box([20.0, 0.0, 0.0]);
    let region = |shells: Vec<u32>, void| Region { shells, void };
    let both = merged(
        &[&sheet, &hollow],
        vec![
            region(vec![0, 1], false),
            region(vec![1, 2], false),
            region(vec![2], true),
        ],
    );
    let (mut ir, w) = document(&both);
    // Shell 2 is the cavity: its one face, on the sphere, written with no loops.
    let sphere = first_face_of(&both, &w, 2);
    let face = ir.model.faces.iter_mut().find(|f| f.id == sphere).unwrap();
    face.loops = FaceLoops::unspecified(Vec::new());
    let whole = read_bodies(&ir, None);
    assert!(whole.failures.is_empty(), "{:?}", whole.failures);
    let first = &whole.read.iter().find(|r| r.region == 0).unwrap().solid;
    assert_eq!(
        first.brep.regions.iter().filter(|r| r.void).count(),
        1,
        "{:?}",
        first.brep.regions
    );
    // Shell 1 is the hollow box's outer shell.
    plane_moved_off(&mut ir, &first_face_of(&both, &w, 1));
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    let s = &bodies.read.iter().find(|r| r.region == 0).unwrap().solid;
    assert!(
        s.notes.iter().any(|n| n.starts_with("not closed")),
        "{:?}",
        s.notes
    );
    assert_eq!(
        s.brep.faces.len(),
        5 + 1,
        "the outer shell less its face, and the sphere"
    );
    let voids = s.brep.regions.iter().filter(|r| r.void).count();
    assert_eq!(
        voids, 1,
        "the sphere is still the cavity: {:?}",
        s.brep.regions
    );
}

/// A general body's hollow box whose outer shell has a face the reader refuses and writes one of
/// its other edges twice: one use of it moved to a second edge on the same vertices and curve
/// over the same interval. The reader makes the two one edge (`Assembly::finish`), so the outer
/// shell is closed as written and the cavity is still a void, not material. Written once (the
/// control), it reads the same.
#[test]
fn a_general_bodys_cavity_stays_a_void_when_its_outer_shell_writes_an_edge_twice() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId};
    use solidcore_ops::body::make_box;
    use solidcore_ops::boolean::{BooleanOp, boolean};
    use solidcore_record::brep::Region;
    let hollow = boolean(
        make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap(),
        make_box([3.0; 3], [4.0, 4.0, 4.0]).unwrap(),
        BooleanOp::Subtract,
    );
    let hollow = record(hollow.map(|b| b.unwrap()));
    let sheet = open_box([20.0, 0.0, 0.0]);
    let region = |shells: Vec<u32>, void| Region { shells, void };
    let both = merged(
        &[&sheet, &hollow],
        vec![
            region(vec![0, 1], false),
            region(vec![1, 2], false),
            region(vec![2], true),
        ],
    );
    let coedges_of = |ir: &CadIr, face: &FaceId| -> Vec<CoedgeId> {
        let face = ir.model.faces.iter().find(|f| f.id == *face).unwrap();
        face.loops
            .iter()
            .flat_map(|l| {
                ir.model
                    .loops
                    .iter()
                    .find(|x| x.id == *l)
                    .unwrap()
                    .coedges()
                    .to_vec()
            })
            .collect()
    };
    let edge_of = |ir: &CadIr, c: &CoedgeId| -> EdgeId {
        ir.model
            .coedges
            .iter()
            .find(|x| x.id == *c)
            .unwrap()
            .edge
            .clone()
    };
    for twice in [false, true] {
        let (mut ir, w) = document(&both);
        // Shell 1 is the hollow box's outer shell.
        let refused = first_face_of(&both, &w, 1);
        if twice {
            let on_refused: HashSet<EdgeId> = coedges_of(&ir, &refused)
                .iter()
                .map(|c| edge_of(&ir, c))
                .collect();
            let BShell::Faces { faces, .. } = &both.shells[1] else {
                panic!("shell 1")
            };
            let face = w.faces[faces[1].face as usize].clone().unwrap();
            let coedge = coedges_of(&ir, &face)
                .into_iter()
                .find(|c| !on_refused.contains(&edge_of(&ir, c)))
                .unwrap();
            let edge = edge_of(&ir, &coedge);
            let mut second = ir
                .model
                .edges
                .iter()
                .find(|e| e.id == edge)
                .unwrap()
                .clone();
            second.id = EdgeId::try_from("test:twin:edge#0".to_string()).unwrap();
            // Each use of the edge is now the only use of an edge of its own.
            for c in ir.model.coedges.iter_mut().filter(|c| c.edge == edge) {
                c.radial_next = c.id.clone();
                if c.id == coedge {
                    c.edge = second.id.clone();
                }
            }
            ir.model.edges.push(second);
        }
        plane_moved_off(&mut ir, &refused);
        let bodies = read_bodies(&ir, None);
        assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
        let s = &bodies.read.iter().find(|r| r.region == 0).unwrap().solid;
        assert_eq!(
            s.notes
                .iter()
                .any(|n| n.contains("edges the file writes twice")),
            twice,
            "{:?}",
            s.notes
        );
        assert_eq!(s.brep.faces.len(), 5 + 6, "twice: {twice}");
        let voids = s.brep.regions.iter().filter(|r| r.void).count();
        assert_eq!(voids, 1, "twice: {twice}: {:?}", s.brep.regions);
        assert_eq!(s.brep.regions.len(), 2, "twice: {twice}");
    }
}

/// A general body's region of a closed box and an open sheet whose sheet has a face the reader
/// refuses comes in as a surface model of the faces that read, as it does whole: the box,
/// closed still, bounds its material, so the record is no sheet.
#[test]
fn a_general_region_of_a_box_and_a_sheet_missing_a_face_keeps_the_box_material() {
    use solidcore_record::brep::Region;
    let cube = record(solidcore_ops::body::make_box(
        [20.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    ));
    let sheet = open_box([0.0; 3]);
    let region = |shells: Vec<u32>, void| Region { shells, void };
    let both = merged(
        &[&sheet, &cube],
        vec![region(vec![0, 1], false), region(vec![1], false)],
    );
    both.validate_built().unwrap();
    let (mut ir, w) = document(&both);
    assert_eq!(w.regions.len(), 2);
    // The sheet's shell moved into the box's region: one region of a closed shell and an open
    // one, as another writer may write a general body.
    let (boxed, sheets) = (w.regions[0].clone(), w.regions[1].clone());
    let moved = ir
        .model
        .regions
        .iter()
        .find(|r| r.id == sheets)
        .unwrap()
        .shells
        .clone();
    ir.model.regions.retain(|r| r.id != sheets);
    for r in &mut ir.model.regions {
        if r.id == boxed {
            r.shells.extend(moved.iter().cloned());
        }
    }
    for s in &mut ir.model.shells {
        if moved.contains(&s.id) {
            s.region = boxed.clone();
        }
    }
    for b in &mut ir.model.bodies {
        b.regions.retain(|r| *r != sheets);
    }
    let whole = read_bodies(&ir, None);
    assert!(whole.failures.is_empty(), "{:?}", whole.failures);
    assert!(!whole.read[0].solid.sheet, "the box bounds material");
    plane_moved_off(&mut ir, &first_face_of(&both, &w, 0));
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    assert_eq!(bodies.read.len(), 1);
    let s = &bodies.read[0].solid;
    assert!(
        s.notes.iter().any(|n| n.starts_with("not closed")),
        "{:?}",
        s.notes
    );
    assert_eq!(
        s.brep.faces.len(),
        4 + 6,
        "the sheet less its face, and the box"
    );
    assert!(!s.sheet, "the box bounds material still");
    assert_eq!(s.brep.regions.len(), 2, "{:?}", s.brep.regions);
    assert!(
        s.brep.regions.iter().all(|r| !r.void),
        "{:?}",
        s.brep.regions
    );
}

/// A procedural blend's net is written with its definition's certified bound ε as its face's
/// tolerance (the definition itself has no CADIR form): a variable round of a box's edge, each of
/// whose blend faces carries its own bound, and every other face none.
#[test]
fn a_procedural_blends_net_is_written_with_its_bound_as_its_tolerance() {
    use solidcore_record::brep::Surface as S;
    use solidcore_record::brep::blend::BlendDefinition;
    solidcore_ops::session::set_thread_fixes(
        solidcore::kernel::UpstreamFixes::NAMES.iter().copied(),
    )
    .unwrap();
    // The first edge of the box, and its end, a round from radius 2 to 5 takes.
    let mut rounded = None;
    'edges: for e in 0..12 {
        for v in 0..8 {
            let b = solidcore_ops::body::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
            if solidcore_ops::blend::fillet_variable(
                &b,
                e,
                v,
                1,
                &[0.0, 0.0, 1.0, 1.0],
                &[2.0, 5.0],
            )
            .is_ok()
            {
                rounded = Some(b);
                break 'edges;
            }
        }
    }
    let r = record(Ok(rounded.unwrap()));
    let bound = |f: &solidcore_record::brep::Face| match &f.surface {
        S::Blend { definition } => match definition.as_ref() {
            BlendDefinition::Procedural(d) => Some(d.bound),
            BlendDefinition::ExactRound(_) => None,
        },
        _ => None,
    };
    assert!(
        r.faces.iter().filter_map(bound).any(|b| b > 0.0),
        "a procedural round with a bound"
    );
    let (ir, w) = document(&r);
    assert_eq!(errors(&ir), Vec::<String>::new());
    for (k, face) in r.faces.iter().enumerate() {
        let id = w.faces[k].as_ref().unwrap();
        let written = ir.model.faces.iter().find(|f| f.id == *id).unwrap();
        assert_eq!(
            written.tolerance.map(|t| t.get()),
            bound(face).filter(|b| *b > 0.0),
            "face {k}"
        );
    }
}

/// A record of two sheets apart is written as two regions, a CADIR region being one connected
/// piece of its body, and reads back as one record each, each the sheet written.
#[test]
fn sheets_apart_are_written_as_regions_of_their_own() {
    use solidcore_record::brep::Region;
    let (a, b) = (open_box([0.0; 3]), open_box([5.0, 0.0, 0.0]));
    let two = merged(
        &[&a, &b],
        vec![Region {
            shells: vec![0, 1],
            void: false,
        }],
    );
    two.validate_built().unwrap();
    let (ir, w) = document(&two);
    assert_eq!(errors(&ir), Vec::<String>::new());
    assert_eq!(w.regions.len(), 2);
    let bodies = read_bodies(&ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    assert_eq!(bodies.read.len(), 2);
    assert_same(
        &as_cadir_holds_it(&a),
        &bodies.read[0].solid.brep,
        1e-9,
        "the first sheet",
    );
    assert_same(
        &as_cadir_holds_it(&b),
        &bodies.read[1].solid.brep,
        1e-9,
        "the second sheet",
    );
}

/// A wire edge two shells list is refused, as CADIR's wire edge is one shell's; listed by one,
/// it is written.
#[test]
fn a_wire_edge_two_shells_list_is_refused() {
    use solidcore_record::brep::Region;
    let (a, b) = (open_box([0.0; 3]), open_box([5.0, 0.0, 0.0]));
    let mut two = merged(
        &[&a, &b],
        vec![Region {
            shells: vec![0, 1],
            void: false,
        }],
    );
    let (va, vb) = (0, u32::try_from(a.vertices.len()).unwrap());
    let (p, q) = (
        two.vertices[va as usize].point,
        two.vertices[vb as usize].point,
    );
    let e = u32::try_from(two.edges.len()).unwrap();
    two.edges.push(solidcore_record::brep::Edge {
        curve: BCurve::Line { start: p, end: q },
        nurbs: None,
        interval: [0.0, 1.0],
        vertices: [va, vb],
        tolerance: 1e-6,
        uses: Vec::new(),
    });
    if let BShell::Faces { wires, .. } = &mut two.shells[0] {
        wires.push(e);
    }
    two.validate_built().unwrap();
    let mut model = Model::default();
    assert!(write_body(&mut model, &two, "test:wire", None).is_ok());
    if let BShell::Faces { wires, .. } = &mut two.shells[1] {
        wires.push(e);
    }
    two.validate_built().unwrap();
    let mut model = Model::default();
    let refused = write_body(&mut model, &two, "test:wire", None).unwrap_err();
    assert!(
        matches!(&refused, WriteError::Invalid(m) if m.contains("one shell's")),
        "{refused:?}"
    );
    assert!(model.bodies.is_empty());
}

/// A circle edge from angle `start`, turning `sweep` (counter-clockwise, under half a turn), of
/// radius `radius` about the standard frame, on a rational quadratic arc over [0, 1].
fn arc_from(radius: f64, start: f64, sweep: f64) -> (solidcore_record::brep::Edge, P, P) {
    let half = sweep / 2.0;
    let at = |a: f64, k: f64| [k * det::cos(a), k * det::sin(a), 0.0];
    let (p, q) = (at(start, radius), at(start + sweep, radius));
    let (mid, w) = (at(start + half, radius / det::cos(half)), det::cos(half));
    let edge = solidcore_record::brep::Edge {
        curve: BCurve::Circle {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius,
        },
        nurbs: Some(BCurve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [p[0], p[1], p[2], 1.0],
                [mid[0] * w, mid[1] * w, mid[2] * w, w],
                [q[0], q[1], q[2], 1.0],
            ],
        }),
        interval: [0.0, 1.0],
        vertices: [0, 1],
        tolerance: 1e-6,
        uses: Vec::new(),
    };
    (edge, p, q)
}

/// An arc starting just short of a whole turn (`-1e-16` rad) starts at 0, CADIR's first turn,
/// which moves it by 1e-16 rad and the rounding of a turn (about 1e-15 rad): nothing on a circle
/// of 10 mm, but 1e-4 mm on one of 1e11 mm, past the 1e-6 mm its ends are held to. There it is
/// written as its form; so is an arc whose ends a turn added rounds to one angle (8e-16 and
/// 4.6e-16 rad short of it, closer than a turn's ulp, 8.9e-16 rad).
#[test]
fn an_arc_moved_a_turn_past_its_tolerance_is_written_as_its_form() {
    let (edge, p, q) = arc_from(10.0, -1e-16, 1.0);
    assert_eq!(written_on_circle(edge, p, q), Ok(true), "a small circle");
    let (edge, p, q) = arc_from(1e11, -1e-16, 1.0);
    assert_eq!(written_on_circle(edge, p, q), Ok(false), "a huge circle");
    let (edge, p, q) = arc_from(1e11, -8e-16, 3.4e-16);
    assert_eq!(
        written_on_circle(edge, p, q),
        Ok(false),
        "a sliver of a huge circle"
    );
}

/// The comparison of a record read back tells an arc from the arc completing its circle between
/// the same vertices, which the locus, vertices and topology compared do not.
#[test]
fn the_comparison_tells_an_arc_from_the_rest_of_its_circle() {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let (quarter, p, q) = arc_edge(10.0, FRAC_PI_2);
    let base = open_box([0.0; 3]);
    let holding = |e: solidcore_record::brep::Edge| Brep {
        version: base.version,
        tolerance: base.tolerance,
        regions: Vec::new(),
        shells: Vec::new(),
        faces: Vec::new(),
        edges: vec![e],
        vertices: [p, q]
            .map(|point| solidcore_record::brep::Vertex {
                point,
                tolerance: 1e-6,
            })
            .to_vec(),
    };
    let mut same = quarter.clone();
    same.nurbs = None;
    same.interval = [0.0, FRAC_PI_2];
    // The rest of the circle, counter-clockwise from q to p.
    let mut rest = same.clone();
    rest.interval = [FRAC_PI_2, TAU];
    rest.vertices = [1, 0];
    let written = holding(quarter);
    assert_eq!(other_arcs(&written, &holding(same)), Vec::<String>::new());
    assert_eq!(other_arcs(&written, &holding(rest)).len(), 1);
    // A short arc (1e-5 rad of a unit circle: its middle within rounding of its chord) and the
    // rest of its circle, either one written.
    let (short, p, q) = arc_edge(1.0, 1e-5);
    let base = holding(short.clone());
    let holding = |e: solidcore_record::brep::Edge| Brep {
        edges: vec![e],
        vertices: [p, q]
            .map(|point| solidcore_record::brep::Vertex {
                point,
                tolerance: 1e-6,
            })
            .to_vec(),
        ..base.clone()
    };
    let mut same = short.clone();
    same.nurbs = None;
    same.interval = [0.0, 1e-5];
    let mut rest = same.clone();
    rest.interval = [1e-5, TAU];
    rest.vertices = [1, 0];
    assert_eq!(
        other_arcs(&holding(short.clone()), &holding(same)),
        Vec::<String>::new()
    );
    assert_eq!(other_arcs(&holding(short), &holding(rest)).len(), 1);
}
