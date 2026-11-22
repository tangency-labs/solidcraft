//! Records assembled through [`Assembly`]'s own entry points, with no file behind them: a box of
//! six planes and twelve lines, one of them written against its vertices, and an edge whose
//! vertex lies far off its curve. Expected values are the box's own.

use std::collections::HashMap;

use super::*;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve as BCurve, Frame, Surface as BSurface};

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
                    let line = BCurve::Line { start: corner(from), end: corner(to) };
                    let (e, along) = asm.add_edge(key.0 as u64 * 8 + key.1 as u64, line, [0.0, 1.0], [vertices[a], vertices[z]])?;
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
