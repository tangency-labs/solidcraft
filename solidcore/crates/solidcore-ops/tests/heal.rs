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

/// A 50 mm block with a notch `depth` deep and 10 wide cut across its front face (y = 0) from
/// x = 20 to 30, top to bottom: the notch's floor lies at y = `depth`, joined to the front by
/// two walls `depth` wide.
fn notched_block(depth: f64, tols: Tols) -> Record {
    let outline = [
        [0.0, 0.0],
        [20.0, 0.0],
        [20.0, depth],
        [30.0, depth],
        [30.0, 0.0],
        [50.0, 0.0],
        [50.0, 50.0],
        [0.0, 50.0],
    ];
    // the outline at z = 0 (corners 0 to 7) and at z = 50 (8 to 15)
    let corners: Vec<[f64; 3]> = [0.0, 50.0]
        .iter()
        .flat_map(|&z| outline.iter().map(move |&[x, y]| [x, y, z]))
        .collect();
    let walls: Vec<[usize; 4]> = (0..8)
        .map(|i| [i, (i + 1) % 8, (i + 1) % 8 + 8, i + 8])
        .collect();
    let mut cycles: Vec<&[usize]> =
        vec![&[0, 7, 6, 5, 4, 3, 2, 1], &[8, 9, 10, 11, 12, 13, 14, 15]];
    cycles.extend(walls.iter().map(|w| &w[..]));
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
/// of the two such vertices), and every edge of `after` runs between vertices within `moved`
/// of the ends of an edge of `before` whose tolerance is at least its own (the larger of such
/// edges): the heal grew no tolerance.
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
    // each edge against the edges before between vertices within `moved` of its own (a
    // re-ended edge moved one end; a collapse keeps one of two edges between the same two)
    let ends = |r: &Record, e: &Edge| e.vertices.map(|i| r.vertices[i as usize].point);
    for e in &after.edges {
        let [p, q] = ends(after, e);
        let most = before
            .edges
            .iter()
            .filter(|w| {
                let [a, b] = ends(before, w);
                (dist(a, p) <= moved && dist(b, q) <= moved)
                    || (dist(a, q) <= moved && dist(b, p) <= moved)
            })
            .map(|w| w.tolerance)
            .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
        let Some(most) = most else {
            panic!("an edge from {p:?} to {q:?} far from every edge before");
        };
        assert!(
            e.tolerance <= most,
            "edge from {p:?} to {q:?}: tolerance {} > {most}",
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
    let gap = 2e-4;
    let rec = with_vertex_gap(hair_step_box(3e-4, TOLS), [10.0, 0.0, 0.0], [0.0, 0.0, gap]);
    let (body, order) = Body::from_record(&rec, true).unwrap();
    assert_eq!(failed(&body), [7, 7, 11, 11, 11, 11, 34]);
    heals(&rec, [7, 15, 10]);
    // `no_tolerance_grew` is one-sided; the heal shrinks none either: the gapped corner, which
    // no squeeze moves, keeps the stated tolerance
    let (healed, _) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    let (after, _) = healed.unwrap().body.to_record().unwrap();
    let corner = after.vertices.iter().find(|v| v.point == [10.0, 0.0, gap]);
    assert_eq!(corner.map(|v| v.tolerance), Some(TOLS.vertex));

    // SMLib's own healer on the same record passes the check by shrinking: every tolerance is
    // reset under the stated one, and the gapped corner keeps the gap gain times twice its gap
    // as its zone (#17739), half the record tolerance; the check holds a gap to half the zone
    let (smlib_healed, _) = Body::from_record(&rec, false).unwrap();
    let healed = smlib_healed.check(CheckLevel::All).unwrap();
    assert!(healed.is_valid(), "{healed}");
    let (after, _) = smlib_healed.to_record().unwrap();
    for v in &after.vertices {
        assert!(v.tolerance < TOLS.vertex, "{v:?}");
        if v.point == [10.0, 0.0, gap] {
            assert!(v.tolerance / 2.0 >= 2.0 * gap, "{v:?}");
        }
    }
    assert!(
        after.vertices.iter().any(|v| v.point == [10.0, 0.0, gap]),
        "the gapped corner is kept"
    );
    assert!(after.edges.iter().all(|e| e.tolerance < TOLS.edge));
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

/// A body the check passes is given back as it is: it has nothing the heal takes, so it is not
/// checked either.
#[test]
fn a_body_that_passes_is_not_healed() {
    let (body, order) = Body::from_record(&hair_step_box(3e-3, TOLS), true).unwrap();
    assert!(body.check(CheckLevel::All).unwrap().is_valid());
    let (healed, report) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    assert!(healed.is_none() && !report.kept);
    assert_eq!(
        report.why_not.as_deref(),
        Some("nothing the heal takes could be proved")
    );
    assert_eq!((report.before, report.after), (None, None));
}

/// A block notched twice the zone tolerance deep (2e-5 at the default 1e-5) through SMLib's own
/// healer: on the bottom and the top face, the edges where the notch's two walls meet them,
/// `depth` long, leave the faces' domain side y = 0 square to it. Their far ends stand `depth`
/// off that side, past the tolerance within which two places coincide (the larger of the
/// contact tolerance and the faces' zone tolerance, 1e-5; `geom::tol::coincidence_tol_3d`),
/// so each edge leaves the side. The check read the side to upstream's intersection
/// tolerance, 2e-5 inclusive, at which the whole edge lies on it, running square to the
/// side's direction rather than along it, and refused the body (Loop test 2, "Bad Loop
/// Orient"). It is a valid solid of the block less the notch, 50³ − 10 · 50 · `depth` (to
/// 1e-6 of the notch's volume; measured within 1e-9 of it), as a notch past the band was
/// (2.1e-5, 5e-5).
#[test]
fn a_block_notched_twice_the_tolerance_deep_is_valid() {
    for tol in [1e-5, 1e-7] {
        let tols = Tols {
            body: 1e-5,
            edge: tol,
            vertex: tol,
        };
        for depth in [2e-5, 2.1e-5, 5e-5] {
            let case = format!("tolerance {tol:e}, depth {depth:e}");
            let (body, _) = Body::from_record(&notched_block(depth, tols), false)
                .unwrap_or_else(|e| panic!("{case}: {e:?}"));
            assert_eq!(failed(&body), [0_u32; 0], "{case}");
            // the block's six faces and the notch's three, with the front face in two
            assert_eq!(body.counts().unwrap(), [10, 24, 16], "{case}");
            let notch = 50.0_f64.powi(3) - body.volume(1e-9).unwrap();
            let want = 10.0 * 50.0 * depth;
            assert!(
                (notch - want).abs() <= 1e-6 * want,
                "{case}: {notch} vs {want}"
            );
        }
    }
}

/// A body the check fails for a defect the heal cannot prove away (a spline piece the check
/// calls one point, on an edge whose tolerance is far smaller than the piece) is given back as
/// it is without its full check: with no edit proved there is nothing to compare, and the full
/// check is the costliest step of the heal. The refusal says why the piece stays.
#[test]
fn a_body_with_nothing_the_heal_takes_is_given_back_unchecked() {
    let tols = Tols {
        body: 1e-9,
        edge: 1e-8,
        vertex: 1e-8,
    };
    let rec = box_with_a_short_spline_span(tols, 1e-8);
    let (body, order) = Body::from_record(&rec, true).unwrap();
    assert!(failed(&body).contains(&15), "{:?}", failed(&body));
    let (healed, report) = solidcore_ops::heal::heal_tolerance_defects(&body, &order).unwrap();
    assert!(healed.is_none() && !report.kept);
    assert_eq!(
        report.why_not.as_deref(),
        Some("nothing the heal takes could be proved")
    );
    assert_eq!((report.before, report.after), (None, None));
    assert_eq!(
        report.cut_pieces + report.squeezed_edges + report.collapsed_faces,
        0
    );
    assert!(
        report
            .refused
            .iter()
            .any(|r| r.starts_with("short edge left:")),
        "{:?}",
        report.refused
    );
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

/// [`box_with_a_short_spline_span`] whose bottom front edge has five Bezier pieces, the second
/// and the fourth each with control points `dt` apart in the line's parameter (`10 dt` mm): two
/// pieces the check calls one point, on the one edge. The curve is the line at its own
/// parameter.
fn box_with_two_short_spline_spans(tols: Tols, dt: f64) -> Record {
    let mut rec = box_with_a_short_spline_span(tols, dt);
    let e = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Spline { .. }))
        .unwrap();
    let Curve::Spline { points: old, .. } = rec.edges[e].curve.clone() else {
        unreachable!()
    };
    let (a, b) = (old[0], old[old.len() - 1]);
    let at = |t: f64| {
        [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]
    };
    let short = |t: f64| [at(t + dt), at(t + 2.0 * dt), at(t + 3.0 * dt)];
    let mut points = vec![a, at(0.1), at(0.2), at(0.3)];
    points.extend(short(0.3));
    let t = 0.3 + 3.0 * dt;
    points.extend([
        at(t + (0.6 - t) / 3.0),
        at(t + 2.0 * (0.6 - t) / 3.0),
        at(0.6),
    ]);
    points.extend(short(0.6));
    let t = 0.6 + 3.0 * dt;
    points.extend([at(t + (1.0 - t) / 3.0), at(t + 2.0 * (1.0 - t) / 3.0), b]);
    // each piece's parameter length is its share of the line, so the curve runs at one speed
    // (the face-gap certificate follows the curve by its parameter, and does not close on a
    // piece that is short in space and long in parameter)
    let (s1, s2) = (0.3 + 3.0 * dt, 0.6 + 3.0 * dt);
    let knots = vec![
        0.0, 0.0, 0.0, 0.0, 0.3, 0.3, 0.3, s1, s1, s1, 0.6, 0.6, 0.6, s2, s2, s2, 1.0, 1.0, 1.0,
        1.0,
    ];
    assert_eq!(points.len() + 4, knots.len());
    rec.edges[e].curve = Curve::Spline {
        knots: knots.clone(),
        points: points.clone(),
    };
    rec.edges[e].interval = [0.0, 1.0];
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

/// #17708 review: two pieces of no length on one edge are bounded together. Each cut is bounded
/// from the curve the cut before left, so the curve moves from the one read by up to the sum of
/// the bounds: with the edge's tolerance between one piece's span and two, the first piece is
/// cut and the second refused with the sum, past the tolerance.
#[test]
fn two_cuts_on_one_edge_are_bounded_by_their_sum() {
    use solidcore::heal::tolerance_defects::{cut_degenerate_pieces, Healed, Refusal};
    use solidcore::topology::brep::p3::edit_brep;
    // each short piece spans 3 dt of the 10 mm edge: 3e-7 mm
    let (dt, span) = (1e-8, 3e-7);
    let tol = 1.5 * span;
    let tols = Tols {
        body: 1e-9,
        edge: tol,
        vertex: tol,
    };
    let rec = box_with_two_short_spline_spans(tols, dt);
    let mut k = solidcore::Kernel::default();
    let (mut b, order) = solidcore_convert::brep::to_kernel(&mut k, &rec, true).unwrap();
    let spline = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Spline { .. }))
        .unwrap();
    let e = order.edges[spline];
    // the edge's tolerance between one piece's span and two
    let healed = edit_brep(&mut k, &mut b, |k, b, scope| {
        solidcore::topology::edge::set_tolerance(b, e, tol, false, false)?;
        let mut healed = Healed::default();
        cut_degenerate_pieces(k, b, scope.env, &mut healed)?;
        Ok(healed)
    })
    .unwrap();
    let [(cut, moved)] = healed.cut_pieces[..] else {
        panic!("one edge cut: {healed:?}")
    };
    assert_eq!(cut, e);
    assert!((span * (1.0 - 1e-6)..=tol).contains(&moved), "{moved}");
    let [(edge, Refusal::PieceTooLong(total))] = healed.refused_edges[..] else {
        panic!("the second cut refused: {healed:?}")
    };
    assert_eq!(edge, e);
    assert!(
        total > tol && total >= 2.0 * span * (1.0 - 1e-6),
        "{total} {tol}"
    );
}

/// The hair step with its riser's two end edges squeezed (as the heal's first step does): the
/// riser is a face bounded by its two long sides, between the same two vertices. The brep, the
/// riser and its two edges.
fn squeezed_riser(
    k: &mut solidcore::Kernel,
) -> (
    solidcore::topology::Brep,
    solidcore::topology::FaceId,
    [solidcore::topology::EdgeId; 2],
) {
    use solidcore::heal::tolerance_defects::{collapse_short_edges, squeeze_targets, Healed};
    use solidcore::topology::brep::p3::edit_brep;
    use solidcore::topology::{brep::p1, edge};
    let (mut b, _) =
        solidcore_convert::brep::to_kernel(k, &hair_step_box(3e-4, TOLS), true).unwrap();
    edit_brep(k, &mut b, |k, b, scope| {
        let targets = squeeze_targets(k, b)?;
        collapse_short_edges(k, b, scope.env, &targets, &mut Healed::default())
    })
    .unwrap();
    let mut on: HashMap<solidcore::topology::FaceId, Vec<solidcore::topology::EdgeId>> =
        HashMap::new();
    for e in p1::edges(&b, None).unwrap() {
        for f in edge::faces(&b, e, None).unwrap() {
            on.entry(f).or_default().push(e);
        }
    }
    let riser: Vec<_> = on.iter().filter(|(_, es)| es.len() == 2).collect();
    let [(f, es)] = riser[..] else {
        panic!("one two-edge face: {riser:?}")
    };
    (b, *f, [es[0], es[1]])
}

/// What collapsing the two-edge faces of `b` does after `change`: the faces collapsed and the
/// faces refused.
fn collapse_after(
    k: &mut solidcore::Kernel,
    b: &mut solidcore::topology::Brep,
    change: impl FnOnce(
        &mut solidcore::topology::Brep,
        &mut solidcore::topology::edge::NotifyEnv<'_>,
    ) -> solidcore::Result<()>,
) -> solidcore::heal::tolerance_defects::Healed {
    use solidcore::heal::tolerance_defects::{collapse_coincident_two_edge_faces, Healed};
    use solidcore::topology::brep::p3::edit_brep;
    edit_brep(k, b, |k, b, scope| {
        change(b, scope.env)?;
        let mut healed = Healed::default();
        collapse_coincident_two_edge_faces(k, b, scope.env, &mut healed)?;
        Ok(healed)
    })
    .unwrap()
}

/// The real-defect side of the sliver collapse's certificate (#17708 review): a riser whose two
/// sides are a hair farther apart than the kept edge's tolerance is not collapsed, and the
/// refusal names the bound; one whose removed edge has the larger tolerance is not collapsed
/// either (collapsing would grow the kept edge's), while the same tolerance on the kept edge
/// collapses it.
#[test]
fn a_sliver_wider_than_its_tolerance_or_losing_the_larger_tolerance_is_not_collapsed() {
    use solidcore::heal::tolerance_defects::Refusal;
    use solidcore::topology::{edge, vertex};
    let mut k = solidcore::Kernel::default();
    let (_, f, [e1, e2]) = squeezed_riser(&mut k);
    // As squeezed, the riser collapses.
    let (mut b, ..) = squeezed_riser(&mut k);
    let healed = collapse_after(&mut k, &mut b, |_, _| Ok(()));
    assert_eq!(healed.collapsed.len(), 1, "{healed:?}");
    let tol = TOLS.edge;
    // One side bowed in its middle a hair past the tolerance, away from the other.
    for side in [e1, e2] {
        let (mut b, ..) = squeezed_riser(&mut k);
        let bow = 1.01 * tol;
        let healed = collapse_after(&mut k, &mut b, |b, env| {
            let (s, t) = edge::start_end_vertices(b, side)?;
            let (p, q) = (vertex::point(b, s.unwrap())?, vertex::point(b, t.unwrap())?);
            let mid = (p + q) * 0.5;
            // a quadratic Bezier is `bow` from its chord at its middle when its middle control
            // point is twice that off the chord
            let off = solidcore::math::Vec3::new(0.0, 0.0, 2.0 * bow);
            let cpts = [p, mid + off, q]
                .map(solidcore::nurbs::math::CPoint::from_point)
                .to_vec();
            let nurb = solidcore::nurbs::crv_basic::Curve::new(
                cpts,
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )?;
            let c = solidcore::curve::curve::Curve::from(
                solidcore::nurbs::b_spline_curve::BSplineCurve::from_nurb(nurb, 3, false),
            );
            let interval = solidcore::curve::curve::p2::CurveEvaluate::natural_interval(&c);
            let options = edge::SetCurveOptions {
                delete_old_curve: true,
                ..edge::SetCurveOptions::default()
            };
            edge::set_curve(b, side, Some(c), options, env, [None, None])?;
            edge::set_interval(b, side, &interval)
        });
        assert!(healed.collapsed.is_empty(), "{healed:?}");
        let [(face, Refusal::NotCoincident(d))] = healed.refused_faces[..] else {
            panic!("refused as not coincident: {healed:?}")
        };
        assert_eq!(face, f);
        assert!(d >= bow * (1.0 - 1e-9) && d > tol, "{d} {bow}");
    }
    // The larger tolerance on each side in turn: removed, it refuses; kept, it collapses.
    let mut outcomes = Vec::new();
    for side in [e1, e2] {
        let (mut b, ..) = squeezed_riser(&mut k);
        let healed = collapse_after(&mut k, &mut b, |b, _| {
            edge::set_tolerance(b, side, 2.0 * tol, false, false)
        });
        outcomes.push(match (&healed.collapsed[..], &healed.refused_faces[..]) {
            ([_], []) => "collapsed",
            ([], [(face, Refusal::WouldGrow)]) if *face == f => "would grow",
            _ => panic!("{healed:?}"),
        });
    }
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["collapsed", "would grow"]);
}

/// The squeeze's clusters are found under the kernel's token: a cancelled heal stops there too,
/// before the squeeze's own edge limit is checked; not cancelled, the hair step's riser ends
/// squeeze to their targets.
#[test]
fn squeeze_targets_stop_when_cancelled() {
    use solidcore::heal::tolerance_defects::squeeze_targets;
    let mut k = solidcore::Kernel::default();
    let (b, _) =
        solidcore_convert::brep::to_kernel(&mut k, &hair_step_box(3e-4, TOLS), true).unwrap();
    assert!(!squeeze_targets(&k, &b).unwrap().is_empty());
    let token = solidcore::kernel::CancelToken::new();
    token.cancel();
    k.set_cancel_token(token);
    assert!(squeeze_targets(&k, &b).unwrap_err().is_cancelled());
}

/// #17708 review: a NaN tolerance on either side of a sliver refuses its collapse, and no NaN
/// reaches the edge kept.
#[test]
fn a_sliver_with_a_nan_tolerance_is_not_collapsed() {
    use solidcore::topology::edge;
    let mut k = solidcore::Kernel::default();
    let (_, f, [e1, e2]) = squeezed_riser(&mut k);
    for side in [e1, e2] {
        let (mut b, ..) = squeezed_riser(&mut k);
        let healed = collapse_after(&mut k, &mut b, |b, _| {
            edge::set_tolerance(b, side, f64::NAN, false, false)
        });
        assert!(healed.collapsed.is_empty(), "{healed:?}");
        assert!(
            matches!(healed.refused_faces[..], [(face, _)] if face == f),
            "{healed:?}"
        );
        assert!(solidcore::topology::brep::p1::edges(&b, None)
            .unwrap()
            .into_iter()
            .filter(|&e| e != side)
            .all(|e| !edge::tolerance(&b, e).unwrap().is_nan()));
    }
}

/// A cut whose curve no longer ends on its vertex is refused, and nothing is cut: the spline
/// box's pieces of no length, with the spline edge's vertex tolerances 0 (so the trimmed form's
/// ends, a rounding off the vertices, are farther than the vertices' tolerances).
#[test]
fn a_cut_whose_curve_misses_its_vertex_is_refused_and_nothing_is_cut() {
    use solidcore::heal::tolerance_defects::{cut_degenerate_pieces, Healed, Refusal};
    use solidcore::topology::brep::p3::edit_brep;
    use solidcore::topology::{edge, vertex};
    let mut k = solidcore::Kernel::default();
    let tol = 3e-6;
    let rec = box_with_two_short_spline_spans(
        Tols {
            body: 1e-9,
            edge: tol,
            vertex: tol,
        },
        1e-8,
    );
    let (mut b, order) = solidcore_convert::brep::to_kernel(&mut k, &rec, true).unwrap();
    let spline = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Spline { .. }))
        .unwrap();
    let e = order.edges[spline];
    let before = format!("{:?}", edge::curve(&b, e).unwrap());
    let healed = edit_brep(&mut k, &mut b, |k, b, scope| {
        let (s, t) = edge::start_end_vertices(b, e)?;
        vertex::set_tolerance(b, s.unwrap(), 0.0, false, false)?;
        vertex::set_tolerance(b, t.unwrap(), 0.0, false, false)?;
        let mut healed = Healed::default();
        let any = cut_degenerate_pieces(k, b, scope.env, &mut healed)?;
        assert!(!any, "nothing is cut");
        Ok(healed)
    })
    .unwrap();
    assert!(healed.cut_pieces.is_empty(), "{healed:?}");
    let [(edge_refused, Refusal::CutEnd(d))] = healed.refused_edges[..] else {
        panic!("one CutEnd refusal: {healed:?}");
    };
    assert_eq!(edge_refused, e);
    assert!(
        d > 0.0 && d < 1e-9,
        "a rounding off the vertex, above its tolerance 0: {d}"
    );
    assert_eq!(
        format!("{:?}", edge::curve(&b, e).unwrap()),
        before,
        "the curve is as read"
    );
}

/// A squeeze whose re-ended neighbour misses the new point (the edge's own vertices at
/// tolerance 0), or whose neighbour's far end misses its own vertex (the neighbours' far
/// vertices at tolerance 0), is refused with `NeighbourEnd`, and the edge stays.
#[test]
fn a_squeeze_whose_neighbour_misses_a_vertex_is_refused() {
    use solidcore::heal::tolerance_defects::{collapse_short_edge, Refusal};
    use solidcore::topology::brep::p3::edit_brep;
    use solidcore::topology::{edge, vertex};
    let mut k = solidcore::Kernel::default();
    let step = 3e-4;
    let (a, z) = ([0.0, 5.0, 10.0], [0.0, 5.0, 10.0 + step]);
    for far in [false, true] {
        let rec = hair_step_box(step, TOLS);
        let (mut b, order) = solidcore_convert::brep::to_kernel(&mut k, &rec, true).unwrap();
        let riser = rec
            .edges
            .iter()
            .position(|e| matches!(e.curve, Curve::Line { start, end } if (start, end) == (a, z) || (start, end) == (z, a)))
            .unwrap();
        let e = order.edges[riser];
        let r = edit_brep(&mut k, &mut b, |k, b, scope| {
            let (s, t) = edge::start_end_vertices(b, e)?;
            let (s, t) = (s.unwrap(), t.unwrap());
            if far {
                for x in [s, t] {
                    for n in vertex::edges(b, x, None)? {
                        let (p, q) = edge::start_end_vertices(b, n)?;
                        for y in [p, q].into_iter().flatten().filter(|y| *y != s && *y != t) {
                            vertex::set_tolerance(b, y, 0.0, false, false)?;
                        }
                    }
                }
            } else {
                vertex::set_tolerance(b, s, 0.0, false, false)?;
                vertex::set_tolerance(b, t, 0.0, false, false)?;
            }
            collapse_short_edge(k, b, scope.env, e, None)
        })
        .unwrap();
        let Err(Refusal::NeighbourEnd(n, d)) = r else {
            panic!("far {far}: a NeighbourEnd refusal: {r:?}");
        };
        assert_ne!(n, e);
        assert!(
            d > 0.0 && d < 1e-9,
            "far {far}: a rounding off the vertex: {d}"
        );
        assert!(b.edge(e).is_ok(), "far {far}: the edge stays");
    }
}

/// A 10 mm box whose front top edge, (0, 0, 10) to (10, 0, 10), is a quadratic B-spline with
/// its middle control point `2 bow` above the edge's midpoint, in the front face's plane y = 0:
/// the edge lies on the front face, and its height over the top face z = 10 is
/// `4 bow t (1 - t)`, so its gap to the top face is `bow`, at its middle. Its x is `10 t`, so the
/// top face's straight pcurve is the curve's foot at every parameter, and the front face's
/// pcurve is the same quadratic in that plane's parameters.
fn box_with_a_bowed_top_edge(bow: f64, tols: Tols) -> Record {
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
    let (p, q) = (corners[4], corners[5]);
    let e = rec
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Line { start, end } if (start, end) == (p, q) || (start, end) == (q, p)))
        .unwrap();
    let Curve::Line { start: a, end: b } = rec.edges[e].curve.clone() else {
        unreachable!()
    };
    let middle = [
        (a[0] + b[0]) / 2.0,
        (a[1] + b[1]) / 2.0,
        (a[2] + b[2]) / 2.0 + 2.0 * bow,
    ];
    let points = vec![a, middle, b];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    rec.edges[e].curve = Curve::Spline {
        knots: knots.clone(),
        points: points.clone(),
    };
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
        let uv = |p: [f64; 3]| [dot(sub(p, origin), u_axis), dot(sub(p, origin), v_axis)];
        let normal = cross(u_axis, v_axis);
        // on the top face (normal z) the pcurve is the straight foot; on the front face the
        // quadratic itself
        let pcurve = if normal[2].abs() > 0.5 {
            Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![uv(a), uv(b)],
            }
        } else {
            Curve2::Spline {
                knots: knots.clone(),
                points: points.iter().map(|&p| uv(p)).collect(),
            }
        };
        // the face's domain holds the pcurve (its control points bound it)
        if let Curve2::Spline { points, .. } = &pcurve {
            for p in points {
                for (range, &x) in face.domain.iter_mut().zip(p) {
                    range[0] = range[0].min(x);
                    range[1] = range[1].max(x);
                }
            }
        }
        let Loop::Edges { edges } = &mut face.loops[u.r#loop as usize] else {
            unreachable!()
        };
        edges[u.position as usize].pcurve = pcurve;
    }
    rec
}

/// SMLib's healer (`Fix_TolSizes`, run by `Body::from_record(rec, false)`) gives an edge
/// `1.1 g - 1e-5` for a sampled edge/face gap `g`. An imported edge whose gap is under the limit
/// the pointer check holds an edge to (1000 times the body's 1e-5, `SmBrep` test 59), but over
/// 0.91 of it, was given a tolerance the check refuses, and its end vertices with it (test 82).
/// Its tolerance is now the certified bound of the gap, which is the bow: covered, and under the
/// limit. Where the formula stays under the limit nothing changes, and a gap past the limit is
/// still refused.
#[test]
fn an_edge_whose_gap_is_under_the_check_limit_heals_to_a_tolerance_under_it() {
    let limit = 1000.0 * 1e-5;
    let zone_of = |body: &Body, bow: f64| {
        let (healed, _) = body.to_record().unwrap();
        // the bowed edge: the one whose curve is the quadratic (a record holds twice the zone)
        let e = healed
            .edges
            .iter()
            .find(|e| matches!(&e.curve, Curve::Spline { points, .. } if points.len() == 3))
            .unwrap_or_else(|| panic!("bow {bow}: no bowed edge in {:?}", healed.edges));
        e.tolerance / 2.0
    };

    // 0.95 of the limit: the formula's 1.1 * 9.5e-3 - 1e-5 = 1.044e-2 is past it
    let bow = 0.95 * limit;
    let (body, _) = Body::from_record(&box_with_a_bowed_top_edge(bow, TOLS), false).unwrap();
    assert_eq!(failed(&body), Vec::<u32>::new());
    let zone = zone_of(&body, bow);
    assert!(bow <= zone && zone < limit, "zone {zone}");
    // the bound's own scale is a hundredth of the limit
    assert!(zone <= bow + limit / 100.0, "zone {zone}");

    // half the limit: the formula, unchanged
    let bow = 0.5 * limit;
    let (body, _) = Body::from_record(&box_with_a_bowed_top_edge(bow, TOLS), false).unwrap();
    assert_eq!(failed(&body), Vec::<u32>::new());
    let zone = zone_of(&body, bow);
    let formula = 1.1 * bow - 1e-5;
    assert!(
        (zone - formula).abs() <= 1e-3 * formula,
        "zone {zone}, formula {formula}"
    );

    // 1.05 of the limit: the gap itself is past it, and the check still refuses the edge
    let bow = 1.05 * limit;
    let (body, _) = Body::from_record(&box_with_a_bowed_top_edge(bow, TOLS), false).unwrap();
    assert!(failed(&body).contains(&59), "{:?}", failed(&body));
}
