//! Records assembled through [`Assembly`]'s own entry points, with no file behind them: a box of
//! six planes and twelve lines, one of them written against its vertices, and an edge whose
//! vertex lies far off its curve. Expected values are the box's own.

use std::collections::HashMap;

use super::*;
use solidcore_record::brep::{Curve as BCurve, Surface as BSurface};

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
    Ok((asm.finish(0, "box".into(), vec![faces], Kind::Solid)?, alongs))
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
