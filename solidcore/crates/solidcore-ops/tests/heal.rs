//! Healing tolerance-scale defects of imported bodies: synthetic bodies that carry each defect
//! class SMLib's full check reports on an import, built as records.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::panic,
    clippy::single_match_else,
    clippy::disallowed_methods // the tests' closed-form oracles, not ops geometry
)]

use std::collections::HashMap;

use solidcore_ops::check::CheckLevel;
use solidcore_ops::Body;
use solidcore_record::brep::{
    Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
    Region, Shell, Surface, Vertex,
};

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// A planar record face's unit normal and a point of its plane.
fn plane_of(face: &Face) -> ([f64; 3], [f64; 3]) {
    let Surface::Plane {
        origin,
        u_axis,
        v_axis,
    } = face.surface
    else {
        panic!("not a plane: {:?}", face.surface)
    };
    (unit(cross(u_axis, v_axis)), origin)
}

/// Tolerances of a synthetic body: the body's, every edge's and every vertex's.
#[derive(Clone, Copy)]
struct Tols {
    body: f64,
    edge: f64,
    vertex: f64,
}

/// A closed polyhedron record from its corner points and its faces, each a cycle of corner
/// indices counter-clockwise seen from outside: planar faces (each on the plane through its
/// first corner, its second and its last), straight edges and straight pcurves.
fn polyhedron(corners: &[[f64; 3]], cycles: &[&[usize]], tols: Tols) -> Record {
    let mut edge_of: HashMap<(usize, usize), u32> = HashMap::new();
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut uses: Vec<Vec<EdgeUseRef>> = Vec::new();
    let mut faces = Vec::new();
    for (fi, cycle) in cycles.iter().enumerate() {
        let o = corners[cycle[0]];
        let x = unit(sub(corners[cycle[1]], o));
        let z = unit(cross(x, sub(corners[cycle[cycle.len() - 1]], o)));
        let y = cross(z, x);
        let uv = |p: [f64; 3]| [dot(sub(p, o), x), dot(sub(p, o), y)];
        let mut edges = Vec::new();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for k in 0..cycle.len() {
            let (a, b) = (cycle[k], cycle[(k + 1) % cycle.len()]);
            let (e, forward) = match edge_of.get(&(b, a)) {
                Some(&e) => (e, false),
                None => {
                    let e = ends.len() as u32;
                    edge_of.insert((a, b), e);
                    ends.push((a, b));
                    uses.push(Vec::new());
                    (e, true)
                }
            };
            uses[e as usize].push(EdgeUseRef {
                face: fi as u32,
                r#loop: 0,
                position: k as u32,
                upper: !forward,
            });
            let (s, t) = ends[e as usize];
            for p in [uv(corners[s]), uv(corners[t])] {
                for i in 0..2 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            edges.push(OrientedEdge {
                edge: e,
                forward,
                pcurve: Curve2::Spline {
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    points: vec![uv(corners[s]), uv(corners[t])],
                },
            });
        }
        faces.push(Face {
            nurbs: None,
            surface: Surface::Plane {
                origin: o,
                u_axis: x,
                v_axis: y,
            },
            domain: [[lo[0], hi[0]], [lo[1], hi[1]]],
            loops: vec![Loop::Edges { edges }],
        });
    }
    // Vertices in first appearance along the edges.
    let mut vertex_of: HashMap<usize, u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut number = |c: usize, vertices: &mut Vec<Vertex>| {
        *vertex_of.entry(c).or_insert_with(|| {
            vertices.push(Vertex {
                point: corners[c],
                tolerance: tols.vertex,
            });
            vertices.len() as u32 - 1
        })
    };
    let edges = ends
        .iter()
        .zip(uses)
        .map(|(&(s, t), uses)| {
            let vs = [number(s, &mut vertices), number(t, &mut vertices)];
            Edge {
                nurbs: None,
                curve: Curve::Line {
                    start: corners[s],
                    end: corners[t],
                },
                interval: [0.0, 1.0],
                vertices: vs,
                tolerance: tols.edge,
                uses,
            }
        })
        .collect();
    Record {
        version: solidcore_record::brep::VERSION,
        tolerance: tols.body,
        regions: vec![
            Region {
                shells: vec![0],
                void: false,
            },
            Region {
                shells: vec![0],
                void: false,
            },
        ],
        shells: vec![Shell::Faces {
            faces: (0..cycles.len() as u32)
                .map(|face| OrientedFace {
                    face,
                    forward: true,
                })
                .collect(),
            wires: vec![],
        }],
        faces,
        edges,
        vertices,
    }
}

/// A 10 mm box whose top steps up by `step` along y = 5: the top's back half is `step` higher
/// than its front half, and a riser strip 10 long and `step` high joins them. With `step` under
/// the edge tolerance the riser is a sliver whose two long sides lie within tolerance of each
/// other, and its two end edges (on the side faces) are shorter than their tolerance.
fn hair_step_box(step: f64, tols: Tols) -> Record {
    let h = 10.0 + step;
    let corners = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 5.0, 10.0],
        [0.0, 5.0, 10.0],
        [0.0, 5.0, h],
        [10.0, 5.0, h],
        [10.0, 10.0, h],
        [0.0, 10.0, h],
    ];
    let cycles: [&[usize]; 8] = [
        &[0, 3, 2, 1],        // bottom
        &[4, 5, 6, 7],        // front half of the top
        &[7, 6, 9, 8],        // riser
        &[8, 9, 10, 11],      // back half of the top
        &[0, 1, 5, 4],        // front
        &[2, 3, 11, 10],      // back
        &[1, 2, 10, 9, 6, 5], // right
        &[0, 4, 7, 8, 11, 3], // left
    ];
    polyhedron(&corners, &cycles, tols)
}

/// [`hair_step_box`] whose step ends at the right face in a 45° triangular cap: the corner where
/// the riser, the back half of the top and the right face would meet is cut by a triangle whose
/// three sides, `step / 2` long or less, are all shorter than their tolerance, and whose area is
/// far under the face tolerance squared.
fn hair_step_box_with_cap(step: f64, tols: Tols) -> Record {
    let h = 10.0 + step;
    let d = step / 2.0;
    let corners = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 5.0, 10.0],
        [0.0, 5.0, 10.0],
        [0.0, 5.0, h],
        [10.0 - d, 5.0, h], // 9: the cap's corner on the riser's top side
        [10.0, 10.0, h],
        [0.0, 10.0, h],
        [10.0, 5.0 + d, h], // 12: the cap's corner on the right face's top side
        [10.0, 5.0, h - d], // 13: the cap's corner on the riser's right side
    ];
    let cycles: [&[usize]; 9] = [
        &[0, 3, 2, 1],
        &[4, 5, 6, 7],
        &[7, 6, 13, 9, 8],
        &[8, 9, 12, 10, 11],
        &[0, 1, 5, 4],
        &[2, 3, 11, 10],
        &[1, 2, 10, 12, 13, 6, 5],
        &[0, 4, 7, 8, 11, 3],
        &[9, 13, 12], // the cap
    ];
    polyhedron(&corners, &cycles, tols)
}

/// A 10 mm box whose bottom front edge, (0, 0, 0) to (10, 0, 0), is a cubic B-spline of three
/// Bezier pieces (interior knots of multiplicity 3) along the straight edge, the middle piece's
/// four control points one point: a span of zero length. Its pcurves on both faces are the same
/// spline in the faces' parameters.
fn box_with_a_collapsed_spline_span(tols: Tols) -> Record {
    box_with_a_short_spline_span(tols, 1e-9)
}

/// [`box_with_a_collapsed_spline_span`] whose middle piece's control points are `dt` apart in
/// the line's parameter (the edge is 10 mm long, so `10 dt` mm apart).
fn box_with_a_short_spline_span(tols: Tols, dt: f64) -> Record {
    let corners = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let cycles: [&[usize]; 6] = [
        &[0, 3, 2, 1],
        &[4, 5, 6, 7],
        &[0, 1, 5, 4],
        &[1, 2, 6, 5],
        &[2, 3, 7, 6],
        &[3, 0, 4, 7],
    ];
    let mut rec = polyhedron(&corners, &cycles, tols);
    let (p, q) = (corners[0], corners[1]);
    let e = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Line { start, end } if (start, end) == (p, q) || (start, end) == (q, p)))
        .unwrap();
    let Curve::Line { start: a, end: b } = rec.edges[e].curve.clone() else {
        unreachable!()
    };
    let at = |t: f64| {
        [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]
    };
    let (m0, m1, m2, m3) = (
        at(0.5),
        at(0.5 + dt),
        at(0.5 + 2.0 * dt),
        at(0.5 + 3.0 * dt),
    );
    let points = vec![
        a,
        at(1.0 / 6.0),
        at(2.0 / 6.0),
        m0,
        m1,
        m2,
        m3,
        at(4.0 / 6.0),
        at(5.0 / 6.0),
        b,
    ];
    let knots = vec![
        0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0,
    ];
    rec.edges[e].curve = Curve::Spline {
        knots: knots.clone(),
        points: points.clone(),
    };
    rec.edges[e].interval = [0.0, 3.0];
    for u in rec.edges[e].uses.clone() {
        let face = &mut rec.faces[u.face as usize];
        let Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } = face.surface
        else {
            unreachable!()
        };
        let uv: Vec<[f64; 2]> = points
            .iter()
            .map(|&p| [dot(sub(p, origin), u_axis), dot(sub(p, origin), v_axis)])
            .collect();
        let Loop::Edges { edges } = &mut face.loops[u.r#loop as usize] else {
            unreachable!()
        };
        edges[u.position as usize].pcurve = Curve2::Spline {
            knots: knots.clone(),
            points: uv,
        };
    }
    rec
}

/// `rec` with the vertex at `corner` moved by `by`, its edges' curves left where they were: a
/// vertex/edge gap of `|by|`, which the tolerances must cover (as a file's stated accuracy does).
fn with_vertex_gap(mut rec: Record, corner: [f64; 3], by: [f64; 3]) -> Record {
    let v = rec.vertices.iter_mut().find(|v| v.point == corner).unwrap();
    v.point = [corner[0] + by[0], corner[1] + by[1], corner[2] + by[2]];
    rec
}

const TOLS: Tols = Tols {
    body: 1e-5,
    edge: 1e-3,
    vertex: 1e-3,
};

/// The failed tests of `body`'s full check, by number, sorted.
fn failed(body: &Body) -> Vec<u32> {
    let mut t = body.check(CheckLevel::All).unwrap().failed_tests();
    t.sort_unstable();
    t
}

/// Every vertex of `after` lies within `moved` of a vertex of `before` (a squeeze moves the
/// vertex it keeps to the midpoint of the two) whose tolerance is at least its own (the larger
/// of the two such vertices), and no edge of `after` has a tolerance past the largest of
/// `before`'s: the heal grew no tolerance.
fn no_tolerance_grew(before: &Record, after: &Record, moved: f64) {
    let dist = |a: [f64; 3], b: [f64; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    for v in &after.vertices {
        let near: Vec<f64> = before
            .vertices
            .iter()
            .filter(|w| dist(w.point, v.point) <= moved)
            .map(|w| w.tolerance)
            .collect();
        assert!(
            !near.is_empty(),
            "a vertex at {:?} far from every vertex before",
            v.point
        );
        let most = near.iter().copied().fold(0.0, f64::max);
        assert!(
            v.tolerance <= most,
            "vertex at {:?}: {} > {most}",
            v.point,
            v.tolerance
        );
    }
    let largest = before.edges.iter().map(|e| e.tolerance).fold(0.0, f64::max);
    for e in &after.edges {
        assert!(
            e.tolerance <= largest,
            "edge tolerance {} > {largest}",
            e.tolerance
        );
    }
    assert!(after.tolerance <= before.tolerance);
}

/// Heals `rec` and checks the result: kept, valid, the counts, no tolerance grown, and the
/// volume moved by no more than the surface area times the largest certified bound (no point
/// of the boundary moved farther than that bound).
fn heals(rec: &Record, counts: [usize; 3]) -> solidcore_ops::heal::HealReport {
    let (body, order) = Body::from_record(rec, true).unwrap();
    let (healed, report) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    assert!(report.kept, "{report:?}");
    let healed = healed.unwrap();
    // every healed face is one face of the body, each once
    let mut sources = healed.face_source.clone();
    assert_eq!(sources.len(), counts[0]);
    sources.sort_unstable();
    sources.dedup();
    assert_eq!(sources.len(), counts[0]);
    assert!(sources.iter().all(|&f| f < rec.faces.len()));
    assert_eq!(healed.record.faces.len(), counts[0]);
    // and each healed face lies on its own source face's plane (the heal moves no surface)
    for (i, &src) in healed.face_source.iter().enumerate() {
        let (n0, o0) = plane_of(&rec.faces[src]);
        let (n1, o1) = plane_of(&healed.record.faces[i]);
        assert!(
            dot(n0, n1).abs() >= 1.0 - 1e-12,
            "face {i} from {src}: {n0:?} {n1:?}"
        );
        assert!(
            dot(sub(o1, o0), n0).abs() <= 1e-9,
            "face {i} from {src}: off its plane"
        );
    }
    let healed = healed.body;
    assert!(healed.check(CheckLevel::All).unwrap().is_valid());
    assert_eq!(report.after.as_ref().map(|a| a.failures), Some(0));
    assert_eq!(healed.counts().unwrap(), counts);
    let (before, _) = body.to_record().unwrap();
    let (after, _) = healed.to_record().unwrap();
    let tol = before
        .edges
        .iter()
        .map(|e| e.tolerance)
        .fold(f64::INFINITY, f64::min);
    no_tolerance_grew(&before, &after, tol);
    assert!(
        report.largest_bound > 0.0 && report.largest_bound <= tol,
        "{report:?}"
    );
    let area = 600.0 + 1.0;
    let (v0, v1) = (body.volume(1e-12).unwrap(), healed.volume(1e-12).unwrap());
    assert!((v1 - v0).abs() <= area * report.largest_bound, "{v0} {v1}");
    report
}

/// The hair step: its riser's two end edges are shorter than their tolerance (#7, and #11 for
/// each of their two uses) and its two long sides lie within tolerance of each other (#34).
/// Both end edges are squeezed into their midpoints, the eight edges that met their vertices
/// re-ended there, and the riser, now two coincident edges, collapsed: the two tops meet at one edge and the check passes.
#[test]
fn a_hair_step_heals_to_one_edge_between_its_two_tops() {
    let rec = hair_step_box(3e-4, TOLS);
    let (body, _) = Body::from_record(&rec, true).unwrap();
    assert_eq!(failed(&body), [7, 7, 11, 11, 11, 11, 34]);
    let report = heals(&rec, [7, 15, 10]);
    assert_eq!(
        (
            report.squeezed_edges,
            report.re_ended_edges,
            report.collapsed_faces
        ),
        (2, 8, 1)
    );
    assert!(report.refused.is_empty(), "{report:?}");

    // vertex tolerances below the step: no point lies within half a vertex's tolerance of both
    // tops, so a squeeze would have to grow one; it is refused, with why, and nothing is kept
    let low = hair_step_box(
        3e-4,
        Tols {
            body: 1e-5,
            edge: 1e-3,
            vertex: 1e-5,
        },
    );
    let (body, order) = Body::from_record(&low, true).unwrap();
    let (healed, report) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    assert!(healed.is_none() && !report.kept, "{report:?}");
    assert!(
        report
            .refused
            .iter()
            .any(|r| r.contains("off a face around it")),
        "{report:?}"
    );
}

/// The hair step on a body whose stated tolerance covers a real gap elsewhere (a corner a
/// fraction of the tolerance off its edges): the heal shrinks no tolerance to pass the check,
/// as SMLib's healer does, and grows none.
#[test]
fn a_hair_step_heals_where_the_tolerance_covers_a_real_gap() {
    let rec = with_vertex_gap(
        hair_step_box(3e-4, TOLS),
        [10.0, 0.0, 0.0],
        [0.0, 0.0, 2e-4],
    );
    let (body, _) = Body::from_record(&rec, true).unwrap();
    assert_eq!(failed(&body), [7, 7, 11, 11, 11, 11, 34]);
    heals(&rec, [7, 15, 10]);
    // SMLib's own healer on the same record: every tolerance reset small, the gap no longer
    // covered
    let (smlib_healed, _) = Body::from_record(&rec, false).unwrap();
    let healed = smlib_healed.check(CheckLevel::All).unwrap();
    assert!(!healed.is_valid());
    assert!(healed.failed_tests().iter().all(|&t| t <= 3), "{healed}");
}

/// The hair step ending in a 45° cap at the right face: four short edges squeezed (the cap's
/// sides and the riser's ends), and two slivers collapsed (the cap, then the riser).
#[test]
fn a_hair_step_with_a_triangular_cap_heals() {
    let rec = hair_step_box_with_cap(3e-4, TOLS);
    let (body, _) = Body::from_record(&rec, true).unwrap();
    let before = failed(&body);
    assert!(
        before.contains(&7) && before.contains(&11) && before.contains(&34),
        "{before:?}"
    );
    let report = heals(&rec, [7, 15, 10]);
    assert_eq!((report.squeezed_edges, report.collapsed_faces), (4, 2));
    let gap = with_vertex_gap(
        hair_step_box_with_cap(3e-4, TOLS),
        [10.0, 0.0, 0.0],
        [0.0, 0.0, 2e-4],
    );
    heals(&gap, [7, 15, 10]);
}

/// A box edge whose cubic spline has a piece of no length between two C0 joints (#15 on the
/// curve and on both trims): the piece is cut out, the trims made again, and the check passes.
#[test]
fn a_spline_edge_with_a_piece_of_no_length_heals() {
    let rec = box_with_a_collapsed_spline_span(TOLS);
    let (body, _) = Body::from_record(&rec, true).unwrap();
    assert_eq!(failed(&body), [15, 15, 15]);
    let report = heals(&rec, [6, 12, 8]);
    assert_eq!(report.cut_pieces, 1);
    assert_eq!((report.squeezed_edges, report.collapsed_faces), (0, 0));
}

/// The real-defect side of the cut's certificate: a middle piece the check calls one point
/// (its control points 3e-7 mm apart, under the check's 1e-6) on an edge whose tolerance is
/// 1e-8 is not cut, since cutting would move the curve farther than that tolerance; the
/// refusal names the bound and the edge keeps its curve.
#[test]
fn a_piece_longer_than_its_edge_tolerance_is_not_cut() {
    use solidcore::heal::tolerance_defects::{cut_degenerate_pieces, Healed, Refusal};
    use solidcore::topology::brep::p3::edit_brep;
    let tols = Tols {
        body: 1e-9,
        edge: 1e-8,
        vertex: 1e-8,
    };
    let rec = box_with_a_short_spline_span(tols, 1e-8);
    let mut k = solidcore::Kernel::default();
    let (mut b, order) = solidcore_convert::brep::to_kernel(&mut k, &rec, true).unwrap();
    let spline = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Spline { .. }))
        .unwrap();
    let e = order.edges[spline];
    let knots = |b: &solidcore::topology::Brep| match solidcore::topology::edge::curve(b, e)
        .unwrap()
        .unwrap()
        .kind()
    {
        solidcore::curve::curve::CurveKind::BSpline(s) => s.knots(None).unwrap(),
        _ => panic!("the edge's curve is not a spline"),
    };
    let before = knots(&b);
    let (healed, any) = edit_brep(&mut k, &mut b, |k, b, scope| {
        let mut healed = Healed::default();
        let any = cut_degenerate_pieces(k, b, scope.env, &mut healed)?;
        Ok((healed, any))
    })
    .unwrap();
    assert!(!any && healed.cut_pieces.is_empty(), "{healed:?}");
    let [(edge, Refusal::PieceTooLong(bound))] = healed.refused_edges[..] else {
        panic!("one PieceTooLong refusal: {healed:?}")
    };
    assert_eq!(edge, e);
    // the piece's control points span 3e-7 mm: the bound is at least that, past 1e-8
    assert!((3e-7 * (1.0 - 1e-6)..=1e-6).contains(&bound), "{bound}");
    assert_eq!(knots(&b), before);
}

/// A body the check passes is given back as it is.
#[test]
fn a_body_that_passes_is_not_healed() {
    let (body, order) = Body::from_record(&hair_step_box(3e-3, TOLS), true).unwrap();
    let (healed, report) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    assert!(healed.is_none() && !report.kept);
    assert_eq!(report.why_not.as_deref(), Some("the body passes the check"));
}

/// The real-defect side of the squeeze's certificate: the riser's end edge, a hair shorter than
/// its tolerance, is squeezed into its midpoint; a hair longer, the edges re-ended there would
/// lie more than half their tolerance off the face they leave (SMLib holds a tolerance to twice
/// the gap), so it is refused with that bound, and nothing changes.
#[test]
fn an_edge_a_hair_longer_than_its_tolerance_is_not_squeezed() {
    use solidcore::heal::tolerance_defects::{collapse_short_edge, Refusal};
    use solidcore::topology::brep::p3::edit_brep;
    let riser_end = |rec: &Record, step: f64| {
        let (a, b) = ([0.0, 5.0, 10.0], [0.0, 5.0, 10.0 + step]);
        rec.edges
            .iter()
            .position(|e| matches!(e.curve, Curve::Line { start, end } if (start, end) == (a, b) || (start, end) == (b, a)))
            .unwrap()
    };
    let mut k = solidcore::Kernel::default();
    let probe = hair_step_box(3e-4, TOLS);
    let (b, order) = solidcore_convert::brep::to_kernel(&mut k, &probe, true).unwrap();
    let tol =
        solidcore::topology::edge::tolerance(&b, order.edges[riser_end(&probe, 3e-4)]).unwrap();
    for (scale, squeezed) in [(0.999, true), (1.001, false)] {
        let step = scale * tol;
        let rec = hair_step_box(step, TOLS);
        let (mut b, order) = solidcore_convert::brep::to_kernel(&mut k, &rec, true).unwrap();
        let e = order.edges[riser_end(&rec, step)];
        let (edges, vertices) = (b.edge_list().len(), b.vertex_list().len());
        let r = edit_brep(&mut k, &mut b, |k, b, scope| {
            collapse_short_edge(k, b, scope.env, e, None)
        })
        .unwrap();
        match r {
            Ok(s) => {
                assert!(squeezed, "{scale}");
                // the edge lies within half its length of its midpoint
                assert!(
                    s.bound <= tol && s.bound >= 0.5 * step * (1.0 - 1e-9),
                    "{} {step}",
                    s.bound
                );
                assert!(s.re_ended.iter().all(|(_, g)| *g <= 0.5 * tol), "{s:?}");
                assert_eq!(
                    (b.edge_list().len(), b.vertex_list().len()),
                    (edges - 1, vertices - 1)
                );
            }
            Err(Refusal::NeighbourOffFace(_, _, Some(g))) => {
                assert!(!squeezed, "{scale}");
                assert!(g > 0.5 * tol, "{g} {tol}");
                assert_eq!(
                    (b.edge_list().len(), b.vertex_list().len()),
                    (edges, vertices)
                );
            }
            Err(other) => panic!("{scale}: {other:?}"),
        }
    }
}
