//! Records assembled through [`Assembly`]'s own entry points, with no file behind them: a box of
//! six planes and twelve lines, one of them written against its vertices, and an edge whose
//! vertex lies far off its curve. Expected values are the box's own.

use std::collections::HashMap;

use super::*;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve as BCurve, Curve2, Frame, Surface as BSurface};

/// The box's corner `c`: bit 0 is x (10 mm), bit 1 y (20 mm), bit 2 z (30 mm).
fn corner(c: usize) -> [f64; 3] {
    [if c & 1 == 0 { 0.0 } else { 10.0 }, if c & 2 == 0 { 0.0 } else { 20.0 }, if c & 4 == 0 { 0.0 } else { 30.0 }]
}

/// Each face: its corners, counter-clockwise about its outward normal, and the plane's axes,
/// whose cross product is that normal.
const FACES: [([usize; 4], [f64; 3], [f64; 3]); 6] = [
    ([0, 2, 3, 1], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ([4, 5, 7, 6], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0, 1, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([2, 6, 7, 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ([0, 4, 6, 2], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([1, 3, 7, 5], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
];

/// The box assembled, its edge from corner 0 to corner 1 written with its line running from 1
/// to 0 while its vertices are given as (0, 1).
fn assembled_box() -> R<(Solid, Vec<bool>)> {
    assembled_box_as(Kind::Solid)
}

/// [`assembled_box`] finished as a record of `kind`.
fn assembled_box_as(kind: Kind) -> R<(Solid, Vec<bool>)> {
    assembled_box_with(kind, |from, to| Ok(BCurve::Line { start: corner(from), end: corner(to) }))
}

fn assembled_box_with(kind: Kind, curve: impl Fn(usize, usize) -> R<BCurve>) -> R<(Solid, Vec<bool>)> {
    let mut asm = Assembly::new(1e-6, None);
    let mut vertices = Vec::new();
    for c in 0..8 {
        vertices.push(asm.add_vertex(corner(c))?);
    }
    // (lower corner, higher corner) → the edge and the corner it starts at.
    let mut edges: HashMap<(usize, usize), (u32, usize)> = HashMap::new();
    let mut alongs = Vec::new();
    let mut faces = Vec::new();
    for (f, (corners, u_axis, v_axis)) in FACES.into_iter().enumerate() {
        let mut uses = Vec::new();
        for k in 0..4 {
            let (i, j) = (corners[k], corners[(k + 1) % 4]);
            let key = (i.min(j), i.max(j));
            let (e, start) = match edges.get(&key) {
                Some(found) => *found,
                None => {
                    let (a, z) = key;
                    // The one edge written backwards: its line runs from its second vertex.
                    let (from, to) = if key == (0, 1) { (z, a) } else { (a, z) };
                    let (e, along) = asm.add_edge(key.0 as u64 * 8 + key.1 as u64, curve(from, to)?, [0.0, 1.0], [vertices[a], vertices[z]])?;
                    alongs.push(along);
                    let start = if along { a } else { z };
                    edges.insert(key, (e, start));
                    (e, start)
                }
            };
            uses.push((e, start == i));
        }
        let plane = BSurface::Plane { origin: corner(corners[0]), u_axis, v_axis };
        faces.push((asm.add_face(f as u64, plane, vec![Bound::Edges(uses)], Some(0), true)?, true));
    }
    Ok((asm.finish(0, "box".into(), vec![faces], kind)?, alongs))
}

#[test]
fn a_box_assembled_from_its_parts_is_a_closed_solid_record() {
    let (solid, _) = assembled_box().unwrap();
    let brep = &solid.brep;
    assert_eq!((brep.faces.len(), brep.edges.len(), brep.vertices.len()), (6, 12, 8));
    assert!(!solid.sheet);
    // The infinite region and the box's.
    assert_eq!(brep.regions.len(), 2);
    for e in &brep.edges {
        assert_eq!(e.uses.len(), 2, "every edge bounds two faces");
    }
    // Each face is named by the origin it was added with.
    let mut origins = solid.face_origin.clone();
    origins.sort_unstable();
    assert_eq!(origins, vec![0, 1, 2, 3, 4, 5]);
    assert!(solid.turned.is_empty(), "no face disagrees with its loops: {:?}", solid.turned);
}

/// Each record edge is named by the origin it was added with (the box's `8 * low + high`
/// corner), each once.
#[test]
fn every_edge_of_the_box_is_named_by_its_origin() {
    let (solid, _) = assembled_box().unwrap();
    let mut origins: Vec<u64> = solid.edge_origin.iter().map(|o| o.expect("an edge added, not made")).collect();
    origins.sort_unstable();
    let mut want: Vec<u64> = FACES
        .iter()
        .flat_map(|(c, _, _)| (0..4).map(move |k| (c[k].min(c[(k + 1) % 4]), c[k].max(c[(k + 1) % 4]))))
        .map(|(a, z)| a as u64 * 8 + z as u64)
        .collect();
    want.sort_unstable();
    want.dedup();
    assert_eq!(origins, want);
}

/// The box with its top face (corners 4, 5, 7, 6 at z = 30) on a spline patch 40 mm square about
/// the face's centre c = (5, 10, 30), turned 30° about z and quadratic in u, its middle row of
/// control points 1 mm on along â: `p = c + s(u) â + 40 (v - 1/2) b̂`,
/// `s(u) = 40 u - 20 + 2 u (1 - u)`. No edge of the face runs along a line of its parameters,
/// no straight edge is a straight line in them, and there is no kernel to fit a pcurve. Where
/// `given`, each top use's pcurve is given as the straight (u, v) line between its ends (`off`
/// from them in u), which lies off its edge by at most a quarter of the 1 mm bulge.
fn box_with_spline_top(given: bool, off: f64) -> R<Solid> {
    let (sin, cos) = std::f64::consts::FRAC_PI_6.sin_cos();
    let (a, b) = ([cos, sin], [-sin, cos]);
    let c = [5.0, 10.0];
    // u where s(u) = t: the root in [0, 1] of -2u² + 42u - (20 + t).
    let u_of = |t: f64| (42.0 - (42.0f64 * 42.0 - 8.0 * (20.0 + t)).sqrt()) / 4.0;
    let uv = |p: [f64; 3]| {
        let d = [p[0] - c[0], p[1] - c[1]];
        [u_of(d[0] * a[0] + d[1] * a[1]) + off, 0.5 + (d[0] * b[0] + d[1] * b[1]) / 40.0]
    };
    let at = |s: f64, v: f64| [c[0] + s * a[0] + 40.0 * (v - 0.5) * b[0], c[1] + s * a[1] + 40.0 * (v - 0.5) * b[1], 30.0];
    let mut asm = Assembly::new(1e-6, None);
    let mut vertices = Vec::new();
    for c in 0..8 {
        vertices.push(asm.add_vertex(corner(c))?);
    }
    let mut edges: HashMap<(usize, usize), u32> = HashMap::new();
    let mut faces = Vec::new();
    for (f, (corners, u_axis, v_axis)) in FACES.into_iter().enumerate() {
        let (mut uses, mut pcurves) = (Vec::new(), Vec::new());
        for k in 0..4 {
            let (i, j) = (corners[k], corners[(k + 1) % 4]);
            let key = (i.min(j), i.max(j));
            let (p, q) = (corner(key.0), corner(key.1));
            let e = match edges.get(&key) {
                Some(e) => *e,
                None => {
                    let (e, _) = asm.add_edge(
                        key.0 as u64 * 8 + key.1 as u64,
                        BCurve::Line { start: p, end: q },
                        [0.0, 1.0],
                        [vertices[key.0], vertices[key.1]],
                    )?;
                    edges.insert(key, e);
                    e
                }
            };
            uses.push((e, i == key.0));
            pcurves.push(Some(Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![uv(p), uv(q)] }));
        }
        let face = if f == 1 {
            // Control rows at s = -20, 1 (the quadratic's middle: s(1/2) = 1/2 = (-20 + 2·1 + 20) / 4), 20.
            let surface = BSurface::Spline {
                u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![vec![at(-20.0, 0.0), at(-20.0, 1.0)], vec![at(1.0, 0.0), at(1.0, 1.0)], vec![at(20.0, 0.0), at(20.0, 1.0)]],
            };
            if given {
                asm.add_face_with_pcurves(f as u64, surface, vec![Bound::Edges(uses)], Some(0), true, vec![pcurves])?
            } else {
                asm.add_face(f as u64, surface, vec![Bound::Edges(uses)], Some(0), true)?
            }
        } else {
            asm.add_face(f as u64, BSurface::Plane { origin: corner(corners[0]), u_axis, v_axis }, vec![Bound::Edges(uses)], Some(0), true)?
        };
        faces.push((face, true));
    }
    asm.finish(0, "box".into(), vec![faces], Kind::Solid)
}

/// A spline face's pcurves given by its producer are its record's where the face has none in
/// closed form and no kernel is there to fit them, and without them the face is refused. Each
/// edge's tolerance covers its given pcurve's gap, up to a quarter of the 1 mm bulge (as a file's
/// pcurves are covered). Given off their edges by 0.1 in u (4 mm, past the 1 mm an edge's
/// tolerance may grow to), the record's check refuses them.
#[test]
fn a_spline_face_takes_the_pcurves_its_producer_gives() {
    assert!(box_with_spline_top(false, 0.0).is_err(), "no pcurves on the top without a kernel to fit them");
    let solid = box_with_spline_top(true, 0.0).unwrap();
    let brep = &solid.brep;
    assert_eq!((brep.faces.len(), brep.edges.len(), brep.vertices.len()), (6, 12, 8));
    let top = solid.face_origin.iter().position(|o| *o == 1).unwrap();
    assert!(matches!(brep.faces[top].surface, BSurface::Spline { .. }));
    let worst = brep.edges.iter().map(|e| e.tolerance).fold(0.0, f64::max);
    assert!(worst <= 0.25 * 1.5 + 1e-6, "the most tolerance an edge takes, {worst} mm");
    assert!(box_with_spline_top(true, 0.1).is_err(), "pcurves off their edges");
}

/// The top face's cubic edge lies off its plane only between the 16 even gap samples (#11652).
#[test]
fn a_spline_edge_off_its_plane_between_the_even_samples_keeps_its_face() {
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.95, 0.96, 0.97, 0.98, 0.99, 1.0, 1.0, 1.0, 1.0];
    let lift = 1e-3;
    let bent = |from: usize, to: usize| {
        if (from, to) != (4, 5) {
            return Ok(BCurve::Line { start: corner(from), end: corner(to) });
        }
        let points: Vec<[f64; 3]> = (0..9)
            .map(|j| {
                let x = 10.0 * knots[j + 1..j + 4].iter().sum::<f64>() / 3.0;
                [x, 0.0, if j == 5 { 30.0 + lift } else { 30.0 }]
            })
            .collect();
        spline_curve(3, knots.clone(), &points, None)
    };
    let (solid, _) = assembled_box_with(Kind::Solid, bent).unwrap();
    assert!(!solid.sheet, "{:?}", solid.notes);
    let brep = &solid.brep;
    assert_eq!((brep.faces.len(), brep.edges.len(), brep.vertices.len()), (6, 12, 8));
    let spline = brep.edges.iter().find(|e| matches!(e.curve, BCurve::Spline { .. })).unwrap();
    assert!(spline.tolerance >= 0.667 * lift, "the tolerance {} covers the 0.667 µm the edge lies off its plane", spline.tolerance);
}

/// A solid's shell still closed when faces are left out bounds a region of its own: the
/// material inside it where it is the outer shell, a cavity where it is a void, and a cavity
/// still where it is a void left first because every face of the outer shell was left out.
#[test]
fn a_closed_shell_of_an_open_solid_keeps_its_role() {
    for (voids, outer, void) in [(false, true, false), (true, true, false), (true, false, true)] {
        let (solid, _) = assembled_box_as(Kind::Open { voids, outer }).unwrap();
        assert!(solid.sheet);
        let bounded: Vec<bool> = solid.brep.regions.iter().skip(1).map(|r| r.void).collect();
        assert_eq!(bounded, vec![void], "voids {voids}, outer {outer}");
    }
}

/// Two diagonal 1 × 1 × 2 lobes touch along x = y = 0, 0 <= z <= 1. A unit
/// cube joins them above z = 1: volume 2 + 2 + 1 = 5. Two of the contact faces
/// continue above the contact and share another edge there. Face-list order is
/// deliberately different from radial order round the four-face contact edge.
#[test]
fn a_partial_edge_contact_is_ordered_by_geometry_instead_of_face_order() {
    let polygons: &[&[[f64; 3]]] = &[
        &[[0., 0., 0.], [0., 0., 1.], [0., 0., 2.], [1., 0., 2.], [1., 0., 0.]],
        &[[0., -1., 0.], [0., -1., 1.], [0., 0., 1.], [0., 0., 0.]],
        &[[0., 0., 0.], [0., 1., 0.], [0., 1., 2.], [0., 0., 2.], [0., 0., 1.]],
        &[[-1., 0., 0.], [0., 0., 0.], [0., 0., 1.], [-1., 0., 1.]],
        &[[0., -1., 0.], [0., 0., 0.], [1., 0., 0.], [1., -1., 0.]],
        &[[-1., 0., 0.], [-1., 1., 0.], [0., 1., 0.], [0., 0., 0.]],
        &[[-1., -1., 2.], [1., -1., 2.], [1., 0., 2.], [0., 0., 2.], [0., 1., 2.], [-1., 1., 2.]],
        &[[0., -1., 0.], [1., -1., 0.], [1., -1., 2.], [-1., -1., 2.], [-1., -1., 1.], [0., -1., 1.]],
        &[[-1., 0., 0.], [-1., 0., 1.], [-1., -1., 1.], [-1., -1., 2.], [-1., 1., 2.], [-1., 1., 0.]],
        &[[0., 1., 0.], [-1., 1., 0.], [-1., 1., 2.], [0., 1., 2.]],
        &[[1., -1., 0.], [1., 0., 0.], [1., 0., 2.], [1., -1., 2.]],
        &[[-1., -1., 1.], [-1., 0., 1.], [0., 0., 1.], [0., -1., 1.]],
    ];
    let mut asm = Assembly::new(1e-6, None);
    let mut points = Vec::new();
    let mut vertices = Vec::new();
    let mut edges = HashMap::new();
    let mut faces = Vec::new();
    for (f, polygon) in polygons.iter().enumerate() {
        let mut corners = Vec::new();
        for &p in *polygon {
            let c = points.iter().position(|&q| p == q).unwrap_or_else(|| {
                points.push(p);
                vertices.push(asm.add_vertex(p).unwrap());
                points.len() - 1
            });
            corners.push(c);
        }
        let mut uses = Vec::new();
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let key = (a.min(b), a.max(b));
            let e = *edges.entry(key).or_insert_with(|| {
                asm.add_edge(
                    (key.0 * 100 + key.1) as u64,
                    BCurve::Line { start: points[key.0], end: points[key.1] },
                    [0., 1.],
                    [vertices[key.0], vertices[key.1]],
                )
                .unwrap()
                .0
            });
            uses.push((e, a == key.0));
        }
        let p = |i: usize| {
            let [x, y, z] = polygon[i];
            solidcore::Vec3::new(x, y, z)
        };
        // The first side may contain the contact's collinear split vertex.
        let u = (p(1) - p(0)).unitize().unwrap();
        let n = u.cross(p(polygon.len() - 1) - p(0)).unitize().unwrap();
        let v = n.cross(u);
        let surface = BSurface::Plane { origin: polygon[0], u_axis: [u.x, u.y, u.z], v_axis: [v.x, v.y, v.z] };
        faces.push((asm.add_face(f as u64, surface, vec![Bound::Edges(uses)], Some(0), true).unwrap(), true));
    }
    let solid = asm.finish(0, "partial contact".into(), vec![faces], Kind::Solid).unwrap();
    assert_eq!(solid.brep.edges.iter().filter(|e| e.uses.len() == 4).count(), 1);
    let contact = solid.brep.edges.iter().find(|e| e.uses.len() == 4).unwrap();
    let ends = |e: &solidcore_record::brep::Edge| e.vertices.map(|v| solid.brep.vertices[v as usize].point);
    assert_eq!(ends(contact), [[0., 0., 0.], [0., 0., 1.]]);
    let continuation = solid.brep.edges.iter().find(|e| ends(e) == [[0., 0., 1.], [0., 0., 2.]]).unwrap();
    assert_eq!(continuation.uses.iter().map(|u| solid.face_origin[u.face as usize]).collect::<Vec<_>>(), [0, 2]);
    // About +z the inward rays are +x, +y, -x, -y, with material in the
    // southeast and northwest quadrants. The two faces ending at z = 1
    // must separate the two faces that continue above it.
    assert_eq!(contact.uses.iter().map(|u| solid.face_origin[u.face as usize]).collect::<Vec<_>>(), [0, 2, 3, 1]);
    // Restoring the file's face order reproduces the old export refusal on the
    // very same geometry. A material-alternation check cannot repair that order.
    let mut unordered = solid.brep.clone();
    unordered.edges.iter_mut().find(|e| e.uses.len() == 4).unwrap().uses.sort_unstable_by_key(|u| u.face);
    let (bad, _) = solidcore_ops::Body::from_record(&unordered, false).unwrap();
    let refused = bad.to_record().unwrap_err().to_string();
    assert!(refused.contains("faces do not alternate between material and space"), "{refused}");
    let (body, _) = solidcore_ops::Body::from_record(&solid.brep, false).unwrap();
    let (record, _) = body.to_record().unwrap();
    record.validate_built().unwrap();
    assert_eq!(record.faces.len(), 12);
    assert!((body.volume(1e-9).unwrap() - 5.).abs() < 1e-9);
}

#[test]
fn an_edge_written_against_its_vertices_is_turned_round_and_noted() {
    let (solid, alongs) = assembled_box().unwrap();
    assert_eq!(alongs.iter().filter(|a| !**a).count(), 1, "{alongs:?}");
    assert!(
        solid.notes.iter().any(|n| n.contains("1 edges whose vertices lie at the other ends of their intervals turned round")),
        "{:?}",
        solid.notes
    );
    // Every edge's curve starts at its first vertex and ends at its second.
    for e in &solid.brep.edges {
        let BCurve::Line { start, end } = &e.curve else { panic!("{:?}", e.curve) };
        let [a, z] = e.vertices.map(|v| solid.brep.vertices[v as usize].point);
        let at = |t: f64| [0, 1, 2].map(|k| start[k] + t * (end[k] - start[k]));
        assert_eq!((at(e.interval[0]), at(e.interval[1])), (a, z));
    }
}

#[test]
fn a_vertex_far_off_its_edge_refuses_the_edge() {
    let mut asm = Assembly::new(1e-6, None);
    let a = asm.add_vertex([0.0, 0.0, 0.0]).unwrap();
    let z = asm.add_vertex([10.0, 5.0, 0.0]).unwrap();
    let line = BCurve::Line { start: [0.0, 0.0, 0.0], end: [10.0, 0.0, 0.0] };
    let e = asm.add_edge(7, line, [0.0, 1.0], [a, z]).unwrap_err();
    assert!(e.starts_with("edge #7: a vertex lies 5 mm off its curve"), "{e}");
}

/// A sphere face bounded by its two poles alone, added through the API, is the whole ball:
/// one face whose seam is a meridian, and the ball's volume.
#[test]
fn a_sphere_bounded_by_its_poles_is_the_whole_ball() {
    let r = 7.0;
    let mut asm = Assembly::new(1e-6, None);
    let south = asm.add_vertex([1.0, 2.0, 3.0 - r]).unwrap();
    let north = asm.add_vertex([1.0, 2.0, 3.0 + r]).unwrap();
    let frame = solidcore_record::brep::Frame { origin: [1.0, 2.0, 3.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
    let face = asm.add_face(0, BSurface::Sphere { frame, radius: r }, vec![Bound::Vertex(south), Bound::Vertex(north)], None, true).unwrap();
    let solid = asm.finish(0, "ball".into(), vec![vec![(face, true)]], Kind::Solid).unwrap();
    assert_eq!((solid.brep.faces.len(), solid.brep.edges.len()), (1, 1), "one face, its meridian seam");
    let (body, _) = solidcore_ops::Body::from_record(&solid.brep, false).unwrap();
    let v = body.volume(1e-9).unwrap();
    let ball = 4.0 / 3.0 * std::f64::consts::PI * r * r * r;
    assert!((v - ball).abs() < 1e-6 * ball, "{v} vs {ball}");
}

/// The spline constructors refuse what no spline is: knots that decrease or leave no domain,
/// coordinates that are not numbers, weights that are not positive.
#[test]
fn malformed_splines_are_refused() {
    let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    assert!(spline_curve(1, vec![0.0, 0.0, 1.0, 1.0], &pts, None).is_ok());
    assert!(spline_curve(1, vec![0.0; 4], &pts, None).unwrap_err().contains("domain is empty"));
    assert!(spline_curve(1, vec![0.0, 1.0, 0.5, 1.0], &pts, None).unwrap_err().contains("increasing"));
    assert!(spline_curve(1, vec![0.0, 0.0, f64::NAN, 1.0], &pts, None).unwrap_err().contains("increasing"));
    assert!(spline_curve(1, vec![0.0, 0.0, 1.0, 1.0], &[[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0]], None).unwrap_err().contains("finite"));
    assert!(spline_curve(1, vec![0.0, 0.0, 1.0, 1.0], &pts, Some(&[1.0, 0.0])).unwrap_err().contains("weight"));
    let grid = vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]], vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]];
    let k = || vec![0.0, 0.0, 1.0, 1.0];
    assert!(spline_surface([1, 1], [k(), k()], &grid, None).is_ok());
    assert!(spline_surface([1, 1], [vec![0.0; 4], k()], &grid, None).unwrap_err().contains("domain is empty"));
    let mut bad = grid.clone();
    bad[1][1][2] = f64::INFINITY;
    assert!(spline_surface([1, 1], [k(), k()], &bad, None).unwrap_err().contains("finite"));
    assert!(spline_surface([1, 1], [k(), k()], &grid, Some(&[vec![1.0, 1.0], vec![1.0, -1.0]])).unwrap_err().contains("weight"));
}

/// Finite coordinates and weights whose products overflow are refused, by both constructors.
#[test]
fn weighted_control_points_that_overflow_are_refused() {
    let big = [[f64::MAX, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let e = spline_curve(1, vec![0.0, 0.0, 1.0, 1.0], &big, Some(&[2.0, 1.0])).unwrap_err();
    assert!(e.contains("weighted control points"), "{e}");
    let grid = vec![vec![[f64::MAX, 0.0, 0.0], [0.0, 1.0, 0.0]], vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]];
    let k = || vec![0.0, 0.0, 1.0, 1.0];
    let e = spline_surface([1, 1], [k(), k()], &grid, Some(&[vec![2.0, 1.0], vec![1.0, 1.0]])).unwrap_err();
    assert!(e.contains("weighted control points"), "{e}");
}

/// A face added on a surface given by its definition ([`Assembly::swept_surface`]), as a reader
/// adds one: its bounds run counter-clockwise about the face's side, which is the definition's
/// own normal when `own`; they and the side are then taken relative to the record surface's
/// natural normal.
fn add_swept(asm: &mut Assembly<'_>, origin: u64, definition: &SweptSurface, mut bounds: Vec<Bound>, own: bool) -> R<(u32, BSurface)> {
    let (surface, reversed) = asm.swept_surface(definition, &bounds)?;
    let side = own != reversed;
    if !side {
        for b in &mut bounds {
            if let Bound::Edges(uses) = b {
                reverse(uses);
            }
        }
    }
    Ok((asm.add_face(origin, surface.clone(), bounds, Some(0), side)?, surface))
}

/// The record's volume.
fn volume(solid: &Solid) -> f64 {
    let (body, _) = solidcore_ops::Body::from_record(&solid.brep, false).unwrap();
    body.volume(1e-9).unwrap()
}

/// The 10 × 20 × 30 box with face `f` of [`FACES`] on `definition` (`own`: the definition's own
/// normal points out of the box there), and the record surface that face got.
fn box_with(f: usize, definition: &SweptSurface, own: bool) -> R<(Solid, BSurface)> {
    let mut asm = Assembly::new(1e-6, None);
    let mut vertices = Vec::new();
    for c in 0..8 {
        vertices.push(asm.add_vertex(corner(c))?);
    }
    let mut edges: HashMap<(usize, usize), u32> = HashMap::new();
    let mut faces = Vec::new();
    let mut got = None;
    for (k, (corners, u_axis, v_axis)) in FACES.into_iter().enumerate() {
        let mut uses = Vec::new();
        for n in 0..4 {
            let (i, j) = (corners[n], corners[(n + 1) % 4]);
            let key = (i.min(j), i.max(j));
            let e = match edges.get(&key) {
                Some(e) => *e,
                None => {
                    let line = BCurve::Line { start: corner(key.0), end: corner(key.1) };
                    let (e, _) = asm.add_edge(key.0 as u64 * 8 + key.1 as u64, line, [0.0, 1.0], [vertices[key.0], vertices[key.1]])?;
                    edges.insert(key, e);
                    e
                }
            };
            uses.push((e, i == key.0));
        }
        let bounds = vec![Bound::Edges(uses)];
        if k == f {
            let (face, surface) = add_swept(&mut asm, k as u64, definition, bounds, own)?;
            got = Some(surface);
            faces.push((face, asm.faces[face as usize].forward));
        } else {
            let plane = BSurface::Plane { origin: corner(corners[0]), u_axis, v_axis };
            faces.push((asm.add_face(k as u64, plane, bounds, Some(0), true)?, true));
        }
    }
    let solid = asm.finish(0, "box".into(), vec![faces], Kind::Solid)?;
    Ok((solid, got.ok_or("no swept face")?))
}

/// A prism over the segment of the ellipse `(10 cos t, 6 sin t)` cut off by the chord x = 5
/// (the ellipse from −60° round its parameter's origin to +60°), 5 mm tall, its curved side the
/// ellipse swept up (or down, the definition's own normal then pointing in): the side covers
/// the narrow segment only, not the rest of the ellipse, and the prism has the segment's
/// volume, the circle's segment (r = 10, 120°) scaled by 6/10 across the chord.
#[test]
fn an_ellipse_swept_across_its_parameter_origin_bounds_only_its_face() {
    let (a, b, h) = (10.0, 6.0, 5.0);
    let s = (std::f64::consts::PI / 3.0).sin();
    for up in [true, false] {
        let mut asm = Assembly::new(1e-6, None);
        let v: Vec<u32> =
            [[5.0, -b * s, 0.0], [5.0, b * s, 0.0], [5.0, b * s, h], [5.0, -b * s, h]].into_iter().map(|p| asm.add_vertex(p).unwrap()).collect();
        let ellipse =
            |z: f64| BCurve::Ellipse { frame: Frame { origin: [0.0, 0.0, z], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radii: [a, b] };
        let arc = [-std::f64::consts::FRAC_PI_3, std::f64::consts::FRAC_PI_3];
        let line = |p: usize, q: usize| (BCurve::Line { start: asm.point(v[p]).unwrap(), end: asm.point(v[q]).unwrap() }, [v[p], v[q]]);
        let lines = [line(1, 0), line(2, 3), line(1, 2), line(0, 3)];
        let low = asm.add_edge(0, ellipse(0.0), arc, [v[0], v[1]]).unwrap().0;
        let high = asm.add_edge(1, ellipse(h), arc, [v[3], v[2]]).unwrap().0;
        let [chord_low, chord_high, rise_plus, rise_minus] = lines.map(|(c, ends)| asm.add_edge(2, c, [0.0, 1.0], ends).unwrap().0);
        let plane = |o: [f64; 3], u: [f64; 3], w: [f64; 3]| BSurface::Plane { origin: o, u_axis: u, v_axis: w };
        let bottom = asm
            .add_face(
                0,
                plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
                vec![Bound::Edges(vec![(low, false), (chord_low, false)])],
                Some(0),
                true,
            )
            .unwrap();
        let top = asm
            .add_face(
                1,
                plane([0.0, 0.0, h], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                vec![Bound::Edges(vec![(high, true), (chord_high, true)])],
                Some(0),
                true,
            )
            .unwrap();
        let flat = asm
            .add_face(
                2,
                plane([5.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                vec![Bound::Edges(vec![(chord_low, true), (rise_minus, true), (chord_high, false), (rise_plus, false)])],
                Some(0),
                true,
            )
            .unwrap();
        let sweep = SweptSurface::LinearSweep { profile: ellipse(0.0), direction: [0.0, 0.0, if up { h } else { -1.0 }] };
        let bounds = vec![Bound::Edges(vec![(low, true), (rise_plus, true), (high, false), (rise_minus, false)])];
        let (side, surface) = add_swept(&mut asm, 3, &sweep, bounds, up).unwrap();
        assert!(matches!(surface, BSurface::RationalSpline { .. }), "{surface:?}");
        let side = (side, asm.faces[side as usize].forward);
        let solid = asm.finish(0, "segment".into(), vec![vec![(bottom, true), (top, true), (flat, true), side]], Kind::Solid).unwrap();
        assert_eq!((solid.brep.faces.len(), solid.brep.edges.len(), solid.brep.vertices.len()), (4, 6, 4), "up: {up}");
        let theta = 2.0 * std::f64::consts::FRAC_PI_3;
        let expected = b / a * 0.5 * a * a * (theta - theta.sin()) * h;
        let got = volume(&solid);
        assert!((got - expected).abs() < 1e-6 * expected, "up: {up}: {got} vs {expected}");
    }
}

/// A quarter of the cylinder of radius 10 about z, 5 tall, bounded by two arcs and two lines,
/// on the line x = 10 (in y = 0) revolved about z: a revolution whose profile reaches the
/// face's height and on which every point of the face lies, its natural normal the
/// definition's own (∂angle × ∂profile): out of the axis turning about +z, into it about −z.
#[test]
fn a_profile_revolved_through_the_entry_covers_its_face() {
    let (r, h) = (10.0, 5.0);
    for (axis, out) in [([0.0, 0.0, 1.0], true), ([0.0, 0.0, -1.0], false)] {
        let mut asm = Assembly::new(1e-6, None);
        let v: Vec<u32> = [[r, 0.0, 0.0], [0.0, r, 0.0], [0.0, r, h], [r, 0.0, h]].into_iter().map(|p| asm.add_vertex(p).unwrap()).collect();
        let arc = |z: f64| BCurve::Circle { frame: Frame { origin: [0.0, 0.0, z], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
        let quarter = [0.0, std::f64::consts::FRAC_PI_2];
        let low = asm.add_edge(0, arc(0.0), quarter, [v[0], v[1]]).unwrap().0;
        let high = asm.add_edge(1, arc(h), quarter, [v[3], v[2]]).unwrap().0;
        let rise = |asm: &mut Assembly<'_>, a: u32, z: u32| {
            let line = BCurve::Line { start: asm.point(a).unwrap(), end: asm.point(z).unwrap() };
            asm.add_edge(2, line, [0.0, 1.0], [a, z]).unwrap().0
        };
        let (left, right) = (rise(&mut asm, v[1], v[2]), rise(&mut asm, v[0], v[3]));
        let uses = vec![(low, true), (left, true), (high, false), (right, false)];
        let profile = BCurve::Line { start: [r, 0.0, 0.0], end: [r, 0.0, 1.0] };
        let revolution = SweptSurface::Revolution { profile, origin: [0.0; 3], axis };
        let (surface, reversed) = asm.swept_surface(&revolution, &[Bound::Edges(uses)]).unwrap();
        assert!(matches!(surface, BSurface::PlacedRevolution { forward: false, .. }) && !reversed, "{surface:?}");
        let s = SurfaceEval::new(&surface).unwrap();
        for p in [[r, 0.0, 0.0], [0.0, r, 0.0], [0.0, r, h], [r, 0.0, h], [r * 0.6, r * 0.8, 0.5 * h]] {
            let p = Vec3::from(p);
            let uv = s.project(p, None, 1e-9).unwrap();
            assert!(s.point(uv).unwrap().dist(p) < 1e-9, "{axis:?} {p:?}");
            let (_, du, dv) = s.derivs(uv[0], uv[1]).unwrap();
            let radial = Vec3::new(p.x, p.y, 0.0);
            assert_eq!(du.cross(dv).dot(radial) > 0.0, out, "{axis:?} {p:?}");
        }
    }
}

/// The box's top (z = 30) as a cone whose half angle is 7e-12 short of a right angle, of
/// radius 1 at the face's middle: the plane across its axis, within the tolerance over the
/// face, so the record has that plane and the box's volume. A cone that opens at 45° is a cone.
#[test]
fn a_cone_of_nearly_a_right_angle_is_its_plane_and_a_steeper_one_a_cone() {
    let frame = Frame { origin: [5.0, 10.0, 30.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
    let flat = SweptSurface::Cone { frame: frame.clone(), radius: 1.0, half_angle: std::f64::consts::FRAC_PI_2 - 7e-12 };
    // Its own normal (the record cone's) points down the axis, into the box.
    let (solid, surface) = box_with(1, &flat, false).unwrap();
    assert!(matches!(surface, BSurface::Plane { .. }), "{surface:?}");
    assert!(solid.turned.is_empty(), "{:?}", solid.turned);
    assert!((volume(&solid) - 6000.0).abs() < 1e-6, "{}", volume(&solid));
    let mut asm = Assembly::new(1e-6, None);
    let corners: Vec<u32> = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]].into_iter().map(|p| asm.add_vertex(p).unwrap()).collect();
    let (steep, reversed) =
        asm.swept_surface(&SweptSurface::Cone { frame, radius: 1.0, half_angle: std::f64::consts::FRAC_PI_4 }, &[Bound::Vertex(corners[0])]).unwrap();
    assert!(matches!(steep, BSurface::Cone { .. }) && !reversed, "{steep:?}");
}

/// An edge on a periodic spline whose interval runs past the spline's range and across its seam
/// (a producer evaluating it modulo its period) is the same piece of curve: a degree-one square
/// on knots -1..5 is at (5, 0, 0) at parameter 4.5, a period on from 0.5.
#[test]
fn an_edge_across_a_periodic_splines_seam_keeps_its_points() {
    let square = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]];
    let curve = spline_curve(1, vec![-1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0], &square, None).unwrap();
    for (interval, ends) in [([3.5, 4.5], [[0.0, 5.0, 0.0], [5.0, 0.0, 0.0]]), ([4.25, 4.75], [[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]])] {
        let (c, [t0, t1]) = periodic_edge(curve.clone(), interval).unwrap();
        let eval = crate::kernel::CurveEval::new(&c).unwrap();
        for (t, want) in [(t0, ends[0]), (t1, ends[1])] {
            let p = eval.point(t).unwrap();
            assert!(p.dist(Vec3::new(want[0], want[1], want[2])) < 1e-12, "{interval:?} at {t}: {p:?}");
        }
    }
    assert!(periodic_edge(curve, [0.0, 4.5]).is_err(), "longer than one period");
}

/// An edge a rounding from the seam: one a whole number of periods from it, where moving it into
/// the range by whole periods rounds (1.2 / 0.4 is 2.999…), keeps its points; one all the way
/// round whose end is a rounding past the range's end is the curve as it is.
#[test]
fn an_edge_a_rounding_from_the_seam_starts_at_the_seam() {
    let square = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]];
    let curve = spline_curve(1, vec![-0.1, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5], &square, None).unwrap();
    let (c, [t0, t1]) = periodic_edge(curve, [1.2, 1.25]).unwrap();
    let eval = crate::kernel::CurveEval::new(&c).unwrap();
    for (t, want) in [(t0, [0.0, 0.0, 0.0]), (t1, [5.0, 0.0, 0.0])] {
        let p = eval.point(t).unwrap();
        assert!(p.dist(Vec3::new(want[0], want[1], want[2])) < 1e-9, "at {t}: {p:?}");
    } // An edge all the way round whose end the producer wrote a rounding past the range's end (a
    // closed spline from its start to its end, as one of the Prusa Core One's is): the curve as it
    // is, not a restart at its start, which no spline can be restarted at.
    let curve = spline_curve(1, vec![-0.1, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5], &square, None).unwrap();
    let (c, [t0, t1]) = periodic_edge(curve.clone(), [0.0, 0.4 + 1e-16]).unwrap();
    assert_eq!(c, curve, "not restarted");
    assert_eq!([t0, t1], [0.0, 0.4]);
    // A short edge a rounding short of the range's end, ending before it: where it is, in order.
    let (c, [t0, t1]) = periodic_edge(curve.clone(), [0.4 - 1e-11, 0.4 - 5e-12]).unwrap();
    assert_eq!(c, curve, "not restarted");
    assert!(t0 < t1 && t1 <= 0.4, "{t0} {t1}");
}

/// A parabola edge is the exact spline over its interval, its ends where the parabola is at the
/// interval's ends (`f t² x + 2 f t y` from its vertex); a hyperbola's likewise.
#[test]
fn parabola_and_hyperbola_edges_lie_on_their_conics() {
    let (o, z, x) = ([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let (f, [t0, t1]) = (2.0, [-1.5, 0.5]);
    let c = parabola_curve(o, z, x, f, [t0, t1]);
    let eval = crate::kernel::CurveEval::new(&c).unwrap();
    for t in [t0, t1] {
        let want = Vec3::new(1.0 + f * t * t, 2.0 + 2.0 * f * t, 3.0);
        assert!(eval.point(t).unwrap().dist(want) < 1e-9, "parabola at {t}");
    }
    let (r, [t0, t1]) = ([3.0, 2.0], [-0.5, 1.0]);
    let c = hyperbola_curve(o, z, x, r, [t0, t1]);
    let eval = crate::kernel::CurveEval::new(&c).unwrap();
    for t in [t0, t1] {
        let want = Vec3::new(1.0 + r[0] * f64::cosh(t), 2.0 + r[1] * f64::sinh(t), 3.0);
        assert!(eval.point(t).unwrap().dist(want) < 1e-9, "hyperbola at {t}");
    }
}

/// An edge bounded by its vertices alone (`add_edge_on`) on a closed spline whose vertex is not
/// where the spline starts runs all the way round from that vertex: no gap at its ends. So does
/// one between two vertices at that one point.
#[test]
fn a_closed_spline_edge_starts_at_its_vertex() {
    let square = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]];
    let curve = spline_curve(1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0], &square, None).unwrap();
    for two in [false, true] {
        let mut asm = Assembly::new(1e-6, None);
        let v = asm.add_vertex([10.0, 10.0, 0.0]).unwrap();
        let w = if two { asm.add_vertex([10.0, 10.0, 0.0]).unwrap() } else { v };
        let (e, _) = asm.add_edge_on(1, curve.clone(), [v, w]).unwrap_or_else(|m| panic!("two vertices {two}: {m}"));
        let edge = &asm.edges[e as usize];
        let eval = crate::kernel::CurveEval::new(&edge.curve).unwrap();
        for t in edge.interval {
            assert!(eval.point(t).unwrap().dist(Vec3::new(10.0, 10.0, 0.0)) < 1e-12, "two vertices {two}: at {t}");
        }
        assert!((edge.interval[1] - edge.interval[0] - 4.0).abs() < 1e-12, "two vertices {two}: the whole square, {:?}", edge.interval);
    }
}

/// A line edge bounded by its vertices alone is as long as the vertices are apart, whatever the
/// line's parameter scale: a 1 mm edge on a line 1e13 mm long between its points is taken.
#[test]
fn a_line_edges_length_does_not_depend_on_its_parameter_scale() {
    for end in [1.0, 1e13] {
        let mut asm = Assembly::new(1e-6, None);
        let (a, b) = (asm.add_vertex([0.0, 0.0, 0.0]).unwrap(), asm.add_vertex([1.0, 0.0, 0.0]).unwrap());
        let line = BCurve::Line { start: [0.0, 0.0, 0.0], end: [end, 0.0, 0.0] };
        assert!(asm.add_edge_on(1, line, [a, b]).is_ok(), "a line to {end}");
    }
}

/// A half cylinder of radius 1 (two rational quarter arcs along u, z along v from 0 to 1).
fn half_cylinder() -> BSurface {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = [([1.0, 0.0], 1.0), ([1.0, 1.0], w), ([0.0, 1.0], 1.0), ([-1.0, 1.0], w), ([-1.0, 0.0], 1.0)];
    let homogeneous = arc.iter().map(|&([x, y], w)| vec![[x * w, y * w, 0.0, w], [x * w, y * w, w, w]]).collect();
    BSurface::RationalSpline { u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], homogeneous }
}

/// A bicubic patch with a single interior knot along each direction, its points off any plane.
fn bicubic() -> BSurface {
    let points =
        (0..5).map(|i| (0..5).map(|j| [f64::from(i), f64::from(j), (f64::from(i) * 0.7).sin() + (f64::from(j) * 1.3).cos()]).collect()).collect();
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0];
    BSurface::Spline { u_knots: knots.clone(), v_knots: knots, points }
}

fn knot_range(s: &BSurface) -> [[f64; 2]; 2] {
    let (u, v, n, m) = match s {
        BSurface::Spline { u_knots, v_knots, points } => (u_knots, v_knots, points.len(), points.first().map_or(0, Vec::len)),
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => (u_knots, v_knots, homogeneous.len(), homogeneous.first().map_or(0, Vec::len)),
        _ => panic!("a spline"),
    };
    let (p, q) = (u.len() - n - 1, v.len() - m - 1);
    [[u[p], u[n]], [v[q], v[m]]]
}

/// `a` and `b` agree on a 21 × 21 grid of parameters over `range`: a sampled regression check
/// that a continuation left the surface as it was there, not a proof of it.
fn same_over(a: &BSurface, b: &BSurface, range: [[f64; 2]; 2]) {
    let (ea, eb) = (SurfaceEval::new(a).unwrap(), SurfaceEval::new(b).unwrap());
    for i in 0..=20 {
        for j in 0..=20 {
            let u = range[0][0] + (range[0][1] - range[0][0]) * f64::from(i) / 20.0;
            let v = range[1][0] + (range[1][1] - range[1][0]) * f64::from(j) / 20.0;
            let (p, q) = (ea.point([u, v]).unwrap(), eb.point([u, v]).unwrap());
            assert!(crate::vec::Vec3Geom::len(p - q) < 1e-12, "({u}, {v}): {p:?} against {q:?}");
        }
    }
}

#[test]
fn a_patch_whose_trims_run_past_it_is_continued_to_them_unchanged_on_its_range() {
    let s = half_cylinder();
    let need = [[-0.03, 2.04], [-0.01, 1.02]];
    let longer = extend::extended(&s, need, 1e-6).expect("continued");
    let range = knot_range(&longer);
    assert!(range[0][0] <= -0.03 && range[0][1] >= 2.04 && range[1][0] <= -0.01 && range[1][1] >= 1.02, "{range:?}");
    same_over(&s, &longer, [[0.0, 2.0], [0.0, 1.0]]);
    // Past the old range the rational arcs continue on the circle: still on the cylinder.
    let e = SurfaceEval::new(&longer).unwrap();
    for uv in [[-0.03, 0.5], [2.04, -0.01], [2.02, 1.02]] {
        let p = e.point(uv).unwrap();
        assert!(((p.x * p.x + p.y * p.y).sqrt() - 1.0).abs() < 1e-12, "{uv:?}: {p:?}");
    }
    // A patch with single interior knots: each end span made one segment first.
    let s = bicubic();
    let longer = extend::extended(&s, [[-0.01, 1.02], [0.0, 1.025]], 1e-6).expect("continued");
    let range = knot_range(&longer);
    assert!(range[0][0] <= -0.01 && range[0][1] >= 1.02 && range[1][1] >= 1.025, "{range:?}");
    same_over(&s, &longer, [[0.0, 1.0], [0.0, 1.0]]);
}

#[test]
fn a_patch_is_not_continued_far_round_where_it_closes_or_where_its_trims_are_on_it() {
    let s = bicubic();
    // On the range: nothing to do.
    assert!(extend::extended(&s, [[0.1, 0.9], [0.0, 1.0]], 1e-6).is_none());
    // Past by more than MAX_EXTENSION of the end span (0.6 long here): refused.
    assert!(extend::extended(&s, [[0.0, 1.0 + 0.6 * extend::MAX_EXTENSION * 1.5], [0.0, 1.0]], 1e-6).is_none());
    // A full cylinder closes along u: a trim past that range wraps round, it is not continued.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = [
        ([1.0, 0.0], 1.0),
        ([1.0, 1.0], w),
        ([0.0, 1.0], 1.0),
        ([-1.0, 1.0], w),
        ([-1.0, 0.0], 1.0),
        ([-1.0, -1.0], w),
        ([0.0, -1.0], 1.0),
        ([1.0, -1.0], w),
        ([1.0, 0.0], 1.0),
    ];
    let homogeneous = ring.iter().map(|&([x, y], w)| vec![[x * w, y * w, 0.0, w], [x * w, y * w, w, w]]).collect();
    let cylinder = BSurface::RationalSpline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        homogeneous,
    };
    assert!(extend::extended(&cylinder, [[0.0, 4.01], [0.0, 1.0]], 1e-6).is_none());
    // Along v it does not close: continued there.
    assert!(extend::extended(&cylinder, [[0.0, 4.0], [0.0, 1.01]], 1e-6).is_some());
}

#[test]
fn a_patch_continuation_keeps_its_shape_on_an_unclamped_start_and_its_limits_on_the_given_spans() {
    // A quadratic along u whose knots are not clamped at its start: its one span [0, 1] is no
    // Bézier segment as given, so it is made one before it is continued, and the patch is the
    // same on [0, 1].
    let s = BSurface::Spline {
        u_knots: vec![-2.0, -1.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]], vec![[1.0, 1.0, 0.0], [1.0, 1.0, 1.0]], vec![[2.0, 0.0, 0.0], [2.0, 0.0, 1.0]]],
    };
    let longer = extend::extended(&s, [[0.0, 1.04], [0.0, 1.0]], 1e-6).expect("continued");
    assert!(knot_range(&longer)[0][1] >= 1.04, "{:?}", knot_range(&longer));
    same_over(&s, &longer, [[0.0, 1.0], [0.0, 1.0]]);
    // A single span [0, 1]: each end may be continued by 5% of it as given, both together; a
    // low end past that is refused whether or not the high end is continued first.
    let flat = BSurface::Spline {
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]], vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0]]],
    };
    assert!(extend::extended(&flat, [[-0.049, 1.049], [0.0, 1.0]], 1e-6).is_some());
    assert!(extend::extended(&flat, [[-0.052, 1.0], [0.0, 1.0]], 1e-6).is_none());
    assert!(extend::extended(&flat, [[-0.052, 1.049], [0.0, 1.0]], 1e-6).is_none());
}

#[test]
fn a_rational_patch_whose_boundaries_share_control_points_but_not_weights_is_open() {
    // Rows 0 and 2 have the same points, (0, 0, 0) and (0, 10, 0), but weights [1, 1] and [1, 9]:
    // half way along v the first boundary is at y = 5 and the last at y = 9. The patch does not
    // close along u, so it is continued there.
    let h = |[x, y, z]: [f64; 3], w: f64| [x * w, y * w, z * w, w];
    let s = BSurface::RationalSpline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        homogeneous: vec![
            vec![h([0.0, 0.0, 0.0], 1.0), h([0.0, 10.0, 0.0], 1.0)],
            vec![h([5.0, 0.0, 5.0], 1.0), h([5.0, 10.0, 5.0], 1.0)],
            vec![h([0.0, 0.0, 0.0], 1.0), h([0.0, 10.0, 0.0], 9.0)],
        ],
    };
    let longer = extend::extended(&s, [[0.0, 1.02], [0.0, 1.0]], 1e-6).expect("continued: it does not close");
    same_over(&s, &longer, [[0.0, 1.0], [0.0, 1.0]]);
}

/// A wedge of a cylinder (radius 10, height 5) from angle −δ to π/2, its curved face's bottom and
/// top arcs each two edges split at angle 0: the arcs from −δ to 0 lie wholly within the
/// tolerance past the quarter line the face's form ends on, so they have no closed form on it.
/// Their closed forms are taken on the form not cut short and put on its range: the wedge is a
/// closed solid of the sector's volume, with no kernel to make trims.
#[test]
fn an_arc_wholly_past_the_quarter_line_its_faces_form_ends_on_takes_its_closed_form() {
    let (r, h, tol) = (10.0, 5.0, 1e-3);
    let delta = 0.8 * tol / r;
    let mut asm = Assembly::new(tol, None);
    let rim = |a: f64, z: f64| [r * a.cos(), r * a.sin(), z];
    let half = std::f64::consts::FRAC_PI_2;
    let (cb, ct) = (asm.add_vertex([0.0; 3]).unwrap(), asm.add_vertex([0.0, 0.0, h]).unwrap());
    let mut at = |a: f64, z: f64| asm.add_vertex(rim(a, z)).unwrap();
    let (bm, b0, bq) = (at(-delta, 0.0), at(0.0, 0.0), at(half, 0.0));
    let (tm, t0, tq) = (at(-delta, h), at(0.0, h), at(half, h));
    let mut line = |o: u64, a: [f64; 3], b: [f64; 3], va: u32, vb: u32| {
        let (e, along) = asm.add_edge(o, BCurve::Line { start: a, end: b }, [0.0, 1.0], [va, vb]).unwrap();
        assert!(along);
        e
    };
    let axis = line(1, [0.0; 3], [0.0, 0.0, h], cb, ct);
    let (b_m, b_q) = (line(2, [0.0; 3], rim(-delta, 0.0), cb, bm), line(3, [0.0; 3], rim(half, 0.0), cb, bq));
    let (t_m, t_q) = (line(4, [0.0, 0.0, h], rim(-delta, h), ct, tm), line(5, [0.0, 0.0, h], rim(half, h), ct, tq));
    let (up_m, up_q) = (line(6, rim(-delta, 0.0), rim(-delta, h), bm, tm), line(7, rim(half, 0.0), rim(half, h), bq, tq));
    let circle = |z: f64| BCurve::Circle { frame: Frame { origin: [0.0, 0.0, z], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
    let mut arc = |o: u64, z: f64, i: [f64; 2], v: [u32; 2]| {
        let (e, along) = asm.add_edge(o, circle(z), i, v).unwrap();
        assert!(along);
        e
    };
    let (ba, bb) = (arc(8, 0.0, [-delta, 0.0], [bm, b0]), arc(9, 0.0, [0.0, half], [b0, bq]));
    let (ta, tb) = (arc(10, h, [-delta, 0.0], [tm, t0]), arc(11, h, [0.0, half], [t0, tq]));
    let cylinder = BSurface::Cylinder { frame: Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
    let plane = |origin: [f64; 3], u_axis: [f64; 3], v_axis: [f64; 3]| BSurface::Plane { origin, u_axis, v_axis };
    let d = [(-delta).cos(), (-delta).sin(), 0.0];
    let faces = [
        (cylinder, vec![(ba, true), (bb, true), (up_q, true), (tb, false), (ta, false), (up_m, false)]),
        (plane([0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), vec![(b_q, true), (bb, false), (ba, false), (b_m, false)]),
        (plane([0.0, 0.0, h], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), vec![(t_m, true), (ta, true), (tb, true), (t_q, false)]),
        (plane([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), vec![(axis, true), (t_q, true), (up_q, false), (b_q, false)]),
        (plane([0.0; 3], d, [0.0, 0.0, 1.0]), vec![(b_m, true), (up_m, true), (t_m, false), (axis, false)]),
    ];
    let mut ids = Vec::new();
    for (k, (surface, uses)) in faces.into_iter().enumerate() {
        ids.push((asm.add_face(k as u64, surface, vec![Bound::Edges(uses)], Some(0), true).unwrap(), true));
    }
    let solid = asm.finish(0, "wedge".into(), vec![ids], Kind::Solid).unwrap_or_else(|e| panic!("{e}"));
    let want = 0.5 * r * r * (half + delta) * h;
    // As the assembly made it (the healer would merge the arcs shorter than the tolerance).
    let (body, _) = solidcore_ops::Body::from_record(&solid.brep, true).unwrap();
    let got = body.volume(1e-9).unwrap();
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}");
}

/// A unit cylinder of height 1 as one rational patch closed in u: four quarter arcs over u in
/// [0, 4], its seam at u = 0.
fn whole_cylinder() -> BSurface {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = [
        ([1.0, 0.0], 1.0),
        ([1.0, 1.0], w),
        ([0.0, 1.0], 1.0),
        ([-1.0, 1.0], w),
        ([-1.0, 0.0], 1.0),
        ([-1.0, -1.0], w),
        ([0.0, -1.0], 1.0),
        ([1.0, -1.0], w),
        ([1.0, 0.0], 1.0),
    ];
    let homogeneous = ring.iter().map(|&([x, y], w)| vec![[x * w, y * w, 0.0, w], [x * w, y * w, w, w]]).collect();
    BSurface::RationalSpline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        homogeneous,
    }
}

/// [`whole_cylinder`] of radius `r`.
fn whole_cylinder_of(r: f64) -> BSurface {
    match whole_cylinder() {
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => {
            let homogeneous = homogeneous.into_iter().map(|row| row.into_iter().map(|[x, y, z, w]| [r * x, r * y, z, w]).collect()).collect();
            BSurface::RationalSpline { u_knots, v_knots, homogeneous }
        }
        other => other,
    }
}

/// Seating a trim that lies a sliver across a closed patch's seam is edited only under an enclosed
/// bound: the unit cylinder's trim 2e-3 across its seam (a period of 4) moves its surface by about
/// 2.83e-3 mm, which the bound holds and is accepted against 5e-2; the cylinder 50 times wider moves
/// 50 times as far over the same sliver (0.14 mm), its partials make the bound past 5e-2, and the
/// trim is declined; so is a trim past the sliver, one not finite, a weight that is not positive
/// and a surface that is not a spline.
#[test]
fn a_trim_is_seated_only_under_an_enclosed_bound_within_the_tolerance() {
    let trim = |x: f64| Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[x, 0.1], [x, 0.9]] };
    let shift = |surface: &BSurface, pcurve: &Curve2, which: Option<bool>| {
        let eval = SurfaceEval::new(surface).unwrap();
        let dom = eval.domain();
        let (Some(period), Some((lo, hi))) = (dom.period[0], dom.range[0]) else { panic!("{dom:?}") };
        assert_eq!((period, lo, hi), (4.0, 0.0, 4.0));
        seating_shift(&eval, 0, [lo, hi], period, pcurve, which, 0.0)
    };
    // The move of the surface for 2e-3 of u at the seam: a quarter turn is one unit of u, and the
    // rational arc runs sqrt(2) per unit of u at its ends (a bit over 1.4), so 2.83e-3 mm.
    let moved = 2e-3 * 1.4;
    let narrow = shift(&whole_cylinder(), &trim(-2e-3), None).expect("a bound");
    assert!(narrow >= moved && narrow < 5e-2, "{narrow}");
    // The same from the other side of the line, and with only the end that moves counted.
    assert!(shift(&whole_cylinder(), &trim(2e-3), None).is_some_and(|s| s >= moved && s < 5e-2));
    assert!(shift(&whole_cylinder(), &trim(-2e-3), Some(true)).is_some_and(|s| s >= moved && s < 5e-2));
    let wide = shift(&whole_cylinder_of(50.0), &trim(-2e-3), None).expect("a bound");
    assert!(wide >= 50.0 * moved && wide > 5e-2, "{wide}");
    // A trim past the sliver (4e-3), or not finite, is not bounded.
    assert!(shift(&whole_cylinder(), &trim(-5e-3), None).is_none());
    assert!(shift(&whole_cylinder(), &trim(f64::NAN), None).is_none());
    let rational = Curve2::RationalSpline { knots: vec![0.0, 0.0, 1.0, 1.0], homogeneous: vec![[-2e-3, 0.1, 1.0], [-2e-3, -0.9, -1.0]] };
    assert!(shift(&whole_cylinder(), &rational, None).is_none());
    // A surface that is not a spline is not decided: its trim stays where it is.
    let plane = BSurface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
    let eval = SurfaceEval::new(&plane).unwrap();
    assert!(seating_shift(&eval, 0, [0.0, 4.0], 4.0, &trim(-2e-3), None, 0.0).is_none());
}

/// A closed patch started elsewhere is the same surface: from its new start to the old seam at
/// the same parameters, and past the old seam one period on. Its seam moves inside a span or onto
/// a knot; a patch that does not close, or a start outside its range, is refused.
#[test]
fn a_closed_patch_started_elsewhere_is_the_same_surface_a_period_on() {
    let s = whole_cylinder();
    for (d, at) in [(0, 1.5), (0, 2.0), (0, 3.25)] {
        let moved = reseam::reseamed(&s, d, at).expect("started elsewhere");
        let range = knot_range(&moved);
        assert!((range[0][0] - at).abs() < 1e-15 && (range[0][1] - (at + 4.0)).abs() < 1e-15, "{range:?}");
        assert_eq!(range[1], [0.0, 1.0]);
        let (old, new) = (SurfaceEval::new(&s).unwrap(), SurfaceEval::new(&moved).unwrap());
        for i in 0..=40 {
            let u = at + 4.0 * f64::from(i) / 40.0;
            for v in [0.0, 0.3, 1.0] {
                let back = if u > 4.0 { u - 4.0 } else { u };
                let (p, q) = (new.point([u, v]).unwrap(), old.point([back, v]).unwrap());
                assert!(crate::vec::Vec3Geom::len(p - q) < 1e-12, "at {at}, ({u}, {v}): {p:?} against {q:?}");
            }
        }
    }
    // Along v the cylinder does not close; its start, or a parameter past its range, is no seam.
    assert!(reseam::reseamed(&s, 1, 0.5).is_none());
    assert!(reseam::reseamed(&s, 0, 0.0).is_none());
    assert!(reseam::reseamed(&s, 0, 4.5).is_none());
    assert!(reseam::reseamed(&half_cylinder(), 0, 1.0).is_none());
}

/// The seam goes in the middle of the widest gap a face's boundary leaves round the closed
/// direction, the gap across the old seam counted; none where that gap already holds the seam.
#[test]
fn a_seam_is_moved_into_the_widest_gap_round_the_face() {
    // A face across the seam at 0 (of [0, 4]): from 3.6 round to 0.4, the gap 0.4 to 3.6.
    assert_eq!(reseam::gap_middle_of(&[[3.6, 3.6], [3.8, 3.8], [4.0, 4.0], [0.0, 0.0], [0.2, 0.2], [0.4, 0.4], [-0.1, -0.1]], [0.0, 4.0]), Some(2.0));
    // Values a period off are taken round it.
    assert_eq!(reseam::gap_middle_of(&[[7.6, 7.6], [4.4, 4.4]], [0.0, 4.0]), Some(2.0));
    // A face away from the seam: the widest gap is across it, so the seam stays.
    assert_eq!(reseam::gap_middle_of(&[[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]], [0.0, 4.0]), None);
    assert_eq!(reseam::gap_middle_of(&[], [0.0, 4.0]), None);
    // Pcurves are taken by their control hulls, which enclose them: one whose ends lie at 0.4
    // and 3.6 but whose hull reaches 2.2 between them leaves no gap round 2, and the seam goes in
    // the gap no hull reaches into. A hull across the seam counts on both sides of it.
    let near = |x: Option<f64>, y: f64| x.is_some_and(|x| (x - y).abs() < 1e-12);
    assert!(near(reseam::gap_middle_of(&[[0.0, 0.4], [0.4, 2.2], [3.6, 4.0]], [0.0, 4.0]), 2.9));
    assert!(near(reseam::gap_middle_of(&[[3.6, 4.4], [1.0, 1.5]], [0.0, 4.0]), 2.55));
    // Hulls that cover the whole period leave no gap.
    assert_eq!(reseam::gap_middle_of(&[[0.5, 4.5]], [0.0, 4.0]), None);
    assert_eq!(reseam::gap_middle_of(&[[0.0, 2.1], [2.0, 4.0]], [0.0, 4.0]), None);
}

/// A face on a plane whose pcurves the kernel makes is trimmed within its own boundary's box,
/// a quarter wider on every side (at least ten times the tolerance): a plane bounds nothing.
#[test]
fn a_plane_faces_trims_are_made_within_its_boundarys_box() {
    let plane = BSurface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
    assert_eq!(bounded_by(&plane, [[0.0, 10.0], [-2.0, 2.0]], 1e-3), Some([[-2.5, 12.5], [-3.0, 3.0]]));
    // A boundary of no width along a direction is still given room.
    assert_eq!(bounded_by(&plane, [[1.0, 1.0], [0.0, 4.0]], 1e-3), Some([[0.99, 1.01], [-1.0, 5.0]]));
    assert_eq!(bounded_by(&plane, [[0.0, f64::INFINITY], [0.0, 1.0]], 1e-3), None);
    assert_eq!(bounded_by(&plane, [[1.0, 0.0], [0.0, 1.0]], 1e-3), None);
}

/// A record whose geometry disagrees on a face names the file's face too, so a reader can read
/// the solid again without it; one that disagrees on an edge alone names no face.
#[test]
fn a_disagreement_on_a_face_names_the_files_face() {
    use solidcore_ops::record_check::{Mismatch, Problem, Record};
    let origins = [7, 41, 9];
    let on = |record: Record| Mismatch { record, problem: Problem::Unevaluable("a test".into()) };
    let m = disagreement(&on(Record::Loop { face: 1, r#loop: 0 }), &origins);
    assert!(m.ends_with(": face #41"), "{m}");
    assert_eq!(crate::topology::named_face(&m), Some(41));
    let m = disagreement(&on(Record::Use { face: 2, r#loop: 0, position: 3 }), &origins);
    assert_eq!(crate::topology::named_face(&m), Some(9), "{m}");
    let m = disagreement(&on(Record::Edge(1)), &origins);
    assert_eq!(crate::topology::named_face(&m), None, "{m}");
    // A face past the origins (no file face known) names none.
    assert_eq!(crate::topology::named_face(&disagreement(&on(Record::Face(5)), &origins)), None);
}

/// The box with the bottom face and the y = 0 face meeting across a sliver: the bottom face's
/// edge from corner 0 to corner 1, the side's from corners 0 and 1 raised by `width`, and a face
/// of the y = 0 plane between them, bounded by the two and the short edges joining their ends
/// (which the x = 0 and x = 10 faces use too).
fn box_with_sliver(width: f64) -> R<Solid> {
    box_with_slivers(width, None)
}

/// [`box_with_sliver`], and where `top` is given a second sliver of that width at the opposite
/// edge: between the top face's edge from corner 6 to corner 7 and the y = 20 face's from
/// corners 6 and 7 lowered by `top`.
fn box_with_slivers(width: f64, top: Option<f64>) -> R<Solid> {
    let tol = 1e-6;
    let mut asm = Assembly::new(tol, None);
    let raised = |c: usize| {
        let mut p = corner(c);
        p[2] += width;
        p
    };
    let lowered = |c: usize| {
        let mut p = corner(c);
        p[2] -= top.unwrap_or(0.0);
        p
    };
    // Corners 0..8, then corner 0 and 1 raised (8 and 9), and with a second sliver corners 6 and
    // 7 lowered (10 and 11).
    let mut vertices = Vec::new();
    for c in 0..8 {
        vertices.push(asm.add_vertex(corner(c))?);
    }
    vertices.push(asm.add_vertex(raised(0))?);
    vertices.push(asm.add_vertex(raised(1))?);
    if top.is_some() {
        vertices.push(asm.add_vertex(lowered(6))?);
        vertices.push(asm.add_vertex(lowered(7))?);
    }
    let point = |v: usize| match v {
        0..8 => corner(v),
        8 | 9 => raised(v - 8),
        _ => lowered(v - 4),
    };
    let mut edges: HashMap<(usize, usize), u32> = HashMap::new();
    let mut edge = |asm: &mut Assembly<'_>, a: usize, z: usize| -> R<(u32, bool)> {
        let key = (a.min(z), a.max(z));
        let e = match edges.get(&key) {
            Some(e) => *e,
            None => {
                let line = BCurve::Line { start: point(key.0), end: point(key.1) };
                let (e, _) = asm.add_edge(key.0 as u64 * 16 + key.1 as u64, line, [0.0, 1.0], [vertices[key.0], vertices[key.1]])?;
                edges.insert(key, e);
                e
            }
        };
        Ok((e, a < z))
    };
    // Each face's corners counter-clockwise about its outward normal, and its plane's axes.
    let mut faces: Vec<(Vec<usize>, [f64; 3], [f64; 3])> = vec![
        (vec![0, 2, 3, 1], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        (vec![4, 5, 7, 6], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (vec![8, 9, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        (vec![2, 6, 7, 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        (vec![8, 4, 6, 2, 0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        (vec![1, 3, 7, 5, 9], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        // The sliver.
        (vec![0, 1, 9, 8], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    if top.is_some() {
        faces[3].0 = vec![2, 10, 11, 3];
        faces[4].0 = vec![8, 4, 6, 10, 2, 0];
        faces[5].0 = vec![1, 3, 11, 7, 5, 9];
        // The second sliver.
        faces.push((vec![11, 10, 6, 7], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    }
    let mut shell = Vec::new();
    for (f, (corners, u_axis, v_axis)) in faces.into_iter().enumerate() {
        let mut uses = Vec::new();
        for k in 0..corners.len() {
            uses.push(edge(&mut asm, corners[k], corners[(k + 1) % corners.len()])?);
        }
        let plane = BSurface::Plane { origin: point(corners[0]), u_axis, v_axis };
        shell.push((asm.add_face(f as u64, plane, vec![Bound::Edges(uses)], Some(0), true)?, true));
    }
    asm.finish(0, "box".into(), vec![shell], Kind::Solid)
}

/// A face narrower than the tolerance between two faces, of no area (its pcurves meet), is left
/// out, its two long sides made one edge and its short ones' vertices one, so the faces either
/// side meet: the box is the box. One narrower than the tolerance that reads is kept, as is one
/// ten tolerances wide.
#[test]
fn a_face_narrower_than_the_tolerance_that_cannot_be_read_is_left_out_its_sides_made_one() {
    let solid = box_with_sliver(0.0).unwrap();
    let brep = &solid.brep;
    assert_eq!((brep.faces.len(), brep.edges.len(), brep.vertices.len()), (6, 12, 8), "{:?}", solid.notes);
    assert!(!solid.sheet);
    for e in &brep.edges {
        assert_eq!(e.uses.len(), 2, "every edge bounds two faces");
    }
    assert!(!solid.face_origin.contains(&6), "{:?}", solid.face_origin);
    assert!(solid.notes.iter().any(|n| n.contains("1 faces narrower than")), "{:?}", solid.notes);
    assert!((volume(&solid) - 6000.0).abs() < 1e-3, "{}", volume(&solid));

    for width in [1e-7, 1e-5] {
        let kept = box_with_sliver(width).unwrap();
        assert_eq!((kept.brep.faces.len(), kept.brep.edges.len(), kept.brep.vertices.len()), (7, 15, 10), "{width}");
        assert!(!kept.notes.iter().any(|n| n.contains("narrower than")), "{:?}", kept.notes);
    }

    // Both in one body: the sliver that cannot be read is left out, the one that reads is kept.
    // Which is left out is decided by each face's own refusal, not by the body's.
    let both = box_with_slivers(0.0, Some(1e-7)).unwrap();
    assert_eq!(both.brep.faces.len(), 7, "{:?}", both.notes);
    assert!(!both.face_origin.contains(&6) && both.face_origin.contains(&7), "{:?}", both.face_origin);
    assert!(both.notes.iter().any(|n| n.contains("1 faces narrower than")), "{:?}", both.notes);
}

/// A degree-1 spline whose control polygon runs out 1 mm and back within the first 1/64 of its
/// span, and then stays at its start: every one of the 33 evenly spaced points lies at the start,
/// so a path summed between them is 0 long, but the curve's own length is 1 + 1 = 2 mm. Its
/// length is enclosed as 2 mm, not summed, so it is not a short edge of a sliver.
#[test]
fn a_splines_excursion_between_samples_is_not_a_short_edge() {
    let tol = 0.01;
    let mut asm = Assembly::new(tol, None);
    let (a, b) = (asm.add_vertex([0.0, 0.0, 0.0]).unwrap(), asm.add_vertex([0.0, 0.0, 0.0]).unwrap());
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
    let knots = vec![0.0, 0.0, 1.0 / 128.0, 1.0 / 64.0, 1.0, 1.0];
    let curve = spline_curve(1, knots, &points, None).unwrap();
    let (e, _) = asm.add_edge(1, curve, [0.0, 1.0], [a, b]).unwrap();
    let eval = crate::kernel::CurveEval::new(&asm.edges[e as usize].curve).unwrap();
    for k in 0..=32 {
        let p = eval.point(f64::from(k) / 32.0).unwrap();
        assert!(p.dist(Vec3::from([0.0, 0.0, 0.0])) < 1e-9, "sample {k} is at {p:?}");
    }
    assert!(!asm.edge_length_within(e, tol).unwrap(), "the 2 mm excursion is past {tol} mm");
}

/// A straight long edge from 0 to 10 mm, and a long edge that follows it except for a 1 mm
/// excursion within the first 1/64 of its span: every sample of the second lies on the first, so
/// a separation measured at samples is 0, but the curves are 1 mm apart. The two are not proved
/// within the tolerance, either way round, so the sliver is not collapsed.
#[test]
fn a_splines_excursion_between_samples_is_not_within_the_long_edges_tolerance() {
    let tol = 0.01;
    let mut asm = Assembly::new(tol, None);
    let (a, b) = (asm.add_vertex([0.0, 0.0, 0.0]).unwrap(), asm.add_vertex([10.0, 0.0, 0.0]).unwrap());
    let line = BCurve::Line { start: [0.0, 0.0, 0.0], end: [10.0, 0.0, 0.0] };
    let (straight, _) = asm.add_edge(1, line, [0.0, 1.0], [a, b]).unwrap();
    let points = [[0.0, 0.0, 0.0], [0.01, 1.0, 0.0], [0.02, 0.0, 0.0], [10.0, 0.0, 0.0]];
    let knots = vec![0.0, 0.0, 1.0 / 128.0, 1.0 / 64.0, 1.0, 1.0];
    let curve = spline_curve(1, knots, &points, None).unwrap();
    let (excursion, _) = asm.add_edge(2, curve, [0.0, 1.0], [a, b]).unwrap();
    let eval = crate::kernel::CurveEval::new(&asm.edges[excursion as usize].curve).unwrap();
    for k in 1..32 {
        let p = eval.point(f64::from(k) / 32.0).unwrap();
        assert!(p.y.abs() < 1e-9 && p.z.abs() < 1e-9, "sample {k} of the excursion is at {p:?}");
    }
    assert!(asm.edges_together(excursion, straight, tol).unwrap().is_none(), "the 1 mm excursion is past {tol} mm");
    assert!(asm.edges_together(straight, excursion, tol).unwrap().is_none(), "the enclosure holds both ways");
}

/// A side of a spline face 5 µm long that the file made one vertex, at its middle: a vertex
/// whose tolerance takes in the whole side is raised to reach along it from its end, where the
/// record check measures it; one whose tolerance does not take it in is left as it is.
#[test]
fn a_side_within_its_vertexs_tolerance_is_reached_from_its_end() {
    let points = vec![vec![[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]], vec![[0.005, 0.0, 0.0], [0.005, 10.0, 0.0]]];
    let strip = BSurface::Spline { u_knots: vec![0.0, 0.0, 1.0, 1.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], points };
    let eval = SurfaceEval::new(&strip).unwrap();
    let vertex = Vec3::from([0.0025, 0.0, 0.0]);
    let r = side_reach(&eval, [0.0, 0.0], 0, [0.0, 1.0], vertex, 0.003).expect("the side lies within the vertex's tolerance");
    assert!((0.005..=0.0051).contains(&r), "{r}");
    assert!(eval.pole_within([0.0, 0.0], r).unwrap()[0]);
    assert_eq!(side_reach(&eval, [0.0, 0.0], 0, [0.0, 1.0], vertex, 0.002), None);
    // Along v the side is 10 mm long, past any vertex.
    assert_eq!(side_reach(&eval, [0.0, 0.0], 1, [0.0, 1.0], vertex, 0.003), None);
}

/// Two unit cubes meeting along the line x = y = 1, the line written as two edges on its two
/// vertices; `crossed`, each edge pairs a face of one cube with a face of the other (as a file
/// may write a heatsink's touching fins), else each cube's two faces there.
fn cubes_on_one_line(crossed: bool) -> R<Solid> {
    let mut asm = Assembly::new(1e-6, None);
    let mut vertices: HashMap<[i64; 3], u32> = HashMap::new();
    let mut edges: HashMap<([u32; 2], u8), u32> = HashMap::new();
    let mut shell = Vec::new();
    // Each cube's faces, corners counter-clockwise about the outward normal.
    let unit: [[[i64; 3]; 4]; 6] = [
        [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
        [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
        [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
        [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
        [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
        [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
    ];
    for (cube, offset) in [[0, 0, 0], [1, 1, 0]].into_iter().enumerate() {
        for (side, corners) in unit.iter().enumerate() {
            let at = |c: [i64; 3]| [c[0] + offset[0], c[1] + offset[1], c[2] + offset[2]];
            let point = |c: [i64; 3]| c.map(|x| x as f64);
            let mut uses = Vec::new();
            for k in 0..4 {
                let (a, z) = (at(corners[k]), at(corners[(k + 1) % 4]));
                let (lo, hi) = if a < z { (a, z) } else { (z, a) };
                let mut id = |c: [i64; 3]| -> R<u32> {
                    if let Some(v) = vertices.get(&c) {
                        return Ok(*v);
                    }
                    let v = asm.add_vertex(point(c))?;
                    vertices.insert(c, v);
                    Ok(v)
                };
                let pair = [id(lo)?, id(hi)?];
                // The shared line's two edges: by cube, or crossed, by the side of x = 1 or y = 1
                // the face lies on (cube 0's x = 1 face with cube 1's x = 1 face).
                let on_line = lo[0] == 1 && lo[1] == 1 && hi[0] == 1 && hi[1] == 1;
                let tag = match (on_line, crossed) {
                    (false, _) => 0,
                    (true, false) => 1 + cube as u8,
                    (true, true) => 1 + u8::from(corners.iter().all(|c| c[1] == corners[0][1])),
                };
                let e = match edges.get(&(pair, tag)) {
                    Some(e) => *e,
                    None => {
                        let line = BCurve::Line { start: point(lo), end: point(hi) };
                        let (e, _) = asm.add_edge(edges.len() as u64, line, [0.0, 1.0], pair)?;
                        edges.insert((pair, tag), e);
                        e
                    }
                };
                uses.push((e, a == lo));
            }
            let [c0, c1, _, c3] = corners.map(|c| point(at(c)));
            let axis = |p: [f64; 3]| [p[0] - c0[0], p[1] - c0[1], p[2] - c0[2]];
            let plane = BSurface::Plane { origin: c0, u_axis: axis(c1), v_axis: axis(c3) };
            let origin = (cube * 6 + side) as u64;
            shell.push((asm.add_face(origin, plane, vec![Bound::Edges(uses)], Some(0), true)?, true));
        }
    }
    asm.finish(0, "cubes".into(), vec![shell], Kind::Solid)
}

/// A line where four faces meet, written as two edges, keeps the shell's sides whichever way
/// the file pairs the faces across them: their uses cancel over the line, so the shell needs no
/// repair and is not refused as contradictory.
#[test]
fn a_line_of_four_faces_written_as_two_edges_keeps_the_shells_sides_however_paired() {
    for crossed in [false, true] {
        let solid = cubes_on_one_line(crossed).unwrap_or_else(|e| panic!("crossed {crossed}: {e:?}"));
        assert!(!solid.sheet, "crossed {crossed}");
        assert!((volume(&solid) - 2.0).abs() < 1e-9, "crossed {crossed}: {}", volume(&solid));
    }
}

/// solidcore#3292 ([T-24](https://tickets.solidcraft.io/t/24)): a wall no wider than the
/// tolerance, its two sides one line written twice, its ends closed edges that stay within the
/// tolerance of their vertex, is left out. The 10 x 20 x 30 box at 0.01 mm with such a wall at
/// its edge from corner 1 to corner 5: a cylinder 0.002 in radius touching that edge, bounded by
/// the edge written twice (the y = 0 side uses one, the x = 10 side the other) and its two end
/// circles, each closed at the edge's end and part of the top's and the bottom's loops. Kept,
/// the face has coincident edges and the body fails the check whatever is done with it; left
/// out, the circles drop out of the top and the bottom, the edge is one, and the record is the
/// box: 6 faces, 12 edges, 8 vertices, 6000 mm³.
#[test]
fn a_wall_no_wider_than_the_tolerance_is_left_out() {
    let tol = 0.01;
    let mut asm = Assembly::new(tol, None);
    let mut v = Vec::new();
    for c in 0..8 {
        v.push(asm.add_vertex(corner(c)).unwrap());
    }
    let line = || BCurve::Line { start: corner(1), end: corner(5) };
    let (side_y, y_along) = asm.add_edge(100, line(), [0.0, 1.0], [v[1], v[5]]).unwrap();
    let (side_x, x_along) = asm.add_edge(101, line(), [0.0, 1.0], [v[1], v[5]]).unwrap();
    assert!(y_along && x_along);
    let r = 0.002;
    let frame = |z: f64| Frame { origin: [10.0 + r, 0.0, z], x_axis: [-1.0, 0.0, 0.0], y_axis: [0.0, -1.0, 0.0] };
    let (bottom_ring, _) = asm.add_edge(102, BCurve::Circle { frame: frame(0.0), radius: r }, [0.0, TAU], [v[1], v[1]]).unwrap();
    let (top_ring, _) = asm.add_edge(103, BCurve::Circle { frame: frame(30.0), radius: r }, [0.0, TAU], [v[5], v[5]]).unwrap();
    let mut edges: HashMap<(usize, usize), u32> = HashMap::new();
    let mut faces = Vec::new();
    for (f, (corners, u_axis, v_axis)) in FACES.into_iter().enumerate() {
        let mut uses = Vec::new();
        for k in 0..4 {
            let (i, j) = (corners[k], corners[(k + 1) % 4]);
            let key = (i.min(j), i.max(j));
            let e = if key == (1, 5) {
                if f == 2 { side_y } else { side_x }
            } else if let Some(e) = edges.get(&key) {
                *e
            } else {
                let (e, _) = asm
                    .add_edge(
                        key.0 as u64 * 8 + key.1 as u64,
                        BCurve::Line { start: corner(key.0), end: corner(key.1) },
                        [0.0, 1.0],
                        [v[key.0], v[key.1]],
                    )
                    .unwrap();
                edges.insert(key, e);
                e
            };
            uses.push((e, i < j));
            // the rings, out at the edge's ends and back
            if j == 1 && f == 0 {
                uses.push((bottom_ring, true));
            }
            if j == 5 && f == 1 {
                uses.push((top_ring, true));
            }
        }
        let plane = BSurface::Plane { origin: corner(corners[0]), u_axis, v_axis };
        faces.push((asm.add_face(f as u64, plane, vec![Bound::Edges(uses)], Some(0), true).unwrap(), true));
    }
    let wall = BSurface::Cylinder { frame: frame(0.0), radius: r };
    let bound = Bound::Edges(vec![(side_y, true), (top_ring, true), (side_x, false), (bottom_ring, false)]);
    faces.push((asm.add_face(6, wall, vec![bound], Some(0), true).unwrap(), true));
    let solid = asm.finish(0, "box".into(), vec![faces], Kind::Solid).unwrap();
    let brep = &solid.brep;
    assert_eq!((brep.faces.len(), brep.edges.len(), brep.vertices.len()), (6, 12, 8));
    assert!(!solid.sheet);
    assert!(solid.notes.iter().any(|n| n.contains("1 walls no wider than 0.01 mm left out")), "{:?}", solid.notes);
    let mut origins = solid.face_origin.clone();
    origins.sort_unstable();
    assert_eq!(origins, vec![0, 1, 2, 3, 4, 5]);
    let vol = volume(&solid);
    assert!((vol - 6000.0).abs() <= 1e-9 * 6000.0, "{vol}");
}

/// A circle of radius `r` about the origin as four rational quadratic quarter arcs, each with
/// weights `[1, k/sqrt 2, k^2]`: the same circle for every `k > 0`, its parameter crowded towards
/// the arcs' ends as `k` grows.
fn rational_circle(r: f64, k: f64) -> Vec<Curve2> {
    let corners = [[r, 0.0], [r, r], [0.0, r], [-r, r], [-r, 0.0], [-r, -r], [0.0, -r], [r, -r], [r, 0.0]];
    let weights = [1.0, k * std::f64::consts::FRAC_1_SQRT_2, k * k];
    (0..4)
        .map(|q| Curve2::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: (0..3)
                .map(|i| {
                    let ([x, y], w) = (corners[2 * q + i], weights[i]);
                    [x * w, y * w, w]
                })
                .collect(),
        })
        .collect()
}

/// #20514: of two nested loops of nearly one size, the larger is told whatever the loops'
/// parameterisation: a ring 3.1 m across and 0.05 mm wide, its inner circle's arcs parameterised
/// fifty times more unevenly than its outer circle's. Each area is the circle's to well within the
/// ring's own area, with an error bound to match, so the outer loop is certainly the largest.
#[test]
fn nested_loops_of_nearly_one_size_are_told_apart_whatever_their_parameterisation() {
    let (ro, ri) = (1550.0_f64, 1549.95_f64);
    let uses: Vec<(u32, bool)> = (0..4).map(|e| (e, true)).collect();
    let pi = std::f64::consts::PI;
    let outer = loop_area_integrated(&uses, &rational_circle(ro, 1.0), pi * ro * ro).unwrap();
    let inner = loop_area_integrated(&uses, &rational_circle(ri, 50.0), pi * ri * ri).unwrap();
    let ring = pi * (ro * ro - ri * ri);
    for ((area, error), r) in [(outer, ro), (inner, ri)] {
        assert!((area - pi * r * r).abs() < 1e-3 * ring && error < 1e-3 * ring, "r {r}: area {area}, error {error}");
    }
    assert_eq!(largest_loop(&[Some(inner), Some(outer)]), Some(1));
    // Two loops the bounds can't tell apart leave the choice to the file.
    assert_eq!(largest_loop(&[Some((100.0, 1.0)), Some((100.5, 1.0))]), None);
}
