//! A sheet turned over through ops (`Body::turn_over`, the kernel's `SmFace::SwapUV` on each
//! face): its record has each face's surface with u and v swapped, the face still used as it was
//! in its shell, so its normal at each point is the other way; its edges are the same edges,
//! renumbered by the record's walk, which the returned map follows; turned over twice it is the
//! sheet again; an analytic sheet keeps its label; a closed body is refused and left as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::face_surface::FaceSurface;
use solidcore_ops::surface::ruled;
use solidcore_ops::{make_box, Body, Error, TurnedOver};
use solidcore_record::brep::{Brep, Curve, Edge, Loop, OrientedEdge, Shell, Surface};

/// A bumped sheet: the ruled sheet between two cubics.
fn sheet() -> Brep {
    let row = |y: f64, z: f64| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, y, 0.0], [3.0, y, z], [6.0, y, -z], [9.0, y, 0.0]],
    };
    ruled(&row(0.0, 2.0), &row(8.0, 1.0), false).unwrap()
}

/// A quarter cylinder sheet labelled a cylinder: the kernel rig's
/// `surfaces/cov-extend-quarter-cylinder-along-its-axis` body `b2` (radius 10 about the z axis,
/// 20 high), from its corpus record, as the converter writes it back. (An extruded arc records
/// as its rational spline form, not a cylinder.)
fn quarter_cylinder() -> Brep {
    let gz = std::fs::File::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../solidcore/tests/kernel_cases/surfaces/cov-extend-quarter-cylinder-along-its-axis.bodies.json.gz",
        ),
    )
    .unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(gz), &mut text).unwrap();
    let bodies: serde_json::Value = serde_json::from_str(&text).unwrap();
    let corpus: Brep = serde_json::from_value(bodies["b2"].clone()).unwrap();
    // as the converter writes it, so the turned record is compared with its own kind of record
    Body::from_record(&corpus, true)
        .unwrap()
        .0
        .to_record()
        .unwrap()
        .0
}

/// Each face's side in its shell: `(face, forward)`, in shell order.
fn sides(r: &Brep) -> Vec<(u32, bool)> {
    r.shells
        .iter()
        .flat_map(|s| match s {
            Shell::Faces { faces, .. } => faces.iter().map(|f| (f.face, f.forward)).collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// The normal face `face` has in its shell at `uv` (the surface's natural normal, turned round
/// where the shell uses the face backward), and the point there.
fn shell_normal(r: &Brep, face: u32, uv: [f64; 2]) -> ([f64; 3], [f64; 3]) {
    let forward = sides(r)
        .iter()
        .find(|(f, _)| *f == face)
        .map(|(_, fw)| *fw)
        .unwrap();
    let s = FaceSurface::of(r, face as usize).unwrap();
    let n = s.normal(uv).unwrap();
    let n = if forward { n } else { [-n[0], -n[1], -n[2]] };
    (n, s.point(uv).unwrap())
}

/// The point of `box_` at the fractions `s` of its first range and `t` of its second.
fn at(box_: [[f64; 2]; 2], s: f64, t: f64) -> [f64; 2] {
    [
        (1.0 - s) * box_[0][0] + s * box_[0][1],
        (1.0 - t) * box_[1][0] + t * box_[1][1],
    ]
}

/// The two normals at the same point are opposite.
fn opposite(name: &str, (nb, pb): ([f64; 3], [f64; 3]), (na, pa): ([f64; 3], [f64; 3])) {
    for k in 0..3 {
        assert!((pb[k] - pa[k]).abs() < 1e-9, "{name}: {pb:?} {pa:?}");
        assert!(
            (nb[k] + na[k]).abs() < 1e-12,
            "{name}: the normal at {pb:?} is negated: {nb:?} then {na:?}"
        );
    }
}

/// Each face is used in its shell as it was, and its shell normal at the middle of its box
/// (the same point on either parametrisation) is the other way; with `transposed`, its box is
/// the other's with u and v swapped and the normals are opposite at three more points.
fn faces_turned(name: &str, before: &Brep, after: &Brep, transposed: bool) {
    assert_eq!(
        sides(after),
        sides(before),
        "{name}: each face used as it was"
    );
    assert_eq!(after.faces.len(), before.faces.len());
    for (i, (fb, fa)) in before.faces.iter().zip(&after.faces).enumerate() {
        let f = i as u32;
        opposite(
            name,
            shell_normal(before, f, at(fb.domain, 0.5, 0.5)),
            shell_normal(after, f, at(fa.domain, 0.5, 0.5)),
        );
        if !transposed {
            continue;
        }
        assert_eq!(
            fa.domain,
            [fb.domain[1], fb.domain[0]],
            "{name}: face {i}'s box"
        );
        for (s, t) in [(0.3, 0.6), (0.5, 0.5), (0.8, 0.1)] {
            let [u, v] = at(fb.domain, s, t);
            opposite(
                name,
                shell_normal(before, f, [u, v]),
                shell_normal(after, f, [v, u]),
            );
        }
    }
}

/// The record edges `map` takes `before`'s to are the same edges: the same curve, interval and
/// end points, one to one.
fn edges_mapped(name: &str, before: &Brep, after: &Brep, map: &[u32]) {
    assert_eq!(map.len(), before.edges.len(), "{name}");
    assert_eq!(after.edges.len(), before.edges.len(), "{name}");
    let mut seen: Vec<u32> = map.to_vec();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), map.len(), "{name}: the map is one to one");
    let ends = |r: &Brep, e: &Edge| e.vertices.map(|v| r.vertices[v as usize].point);
    for (i, &j) in map.iter().enumerate() {
        let (eb, ea) = (&before.edges[i], &after.edges[j as usize]);
        assert_eq!(ea.curve, eb.curve, "{name}: edge {i} -> {j}");
        assert_eq!(ea.interval, eb.interval, "{name}: edge {i} -> {j}");
        assert_eq!(ends(after, ea), ends(before, eb), "{name}: edge {i} -> {j}");
    }
}

/// `r` turned over through ops.
fn turned(r: &Brep) -> TurnedOver {
    let (body, order) = Body::from_record(r, true).unwrap();
    body.turn_over(&order).unwrap()
}

#[test]
fn a_turned_sheet_s_faces_are_swapped_and_face_the_other_way() {
    let before = sheet();
    let t = turned(&before);
    t.record.validate_built().unwrap();
    faces_turned("the bumped sheet", &before, &t.record, true);
    edges_mapped("the bumped sheet", &before, &t.record, &t.edges);
    // the spline's net transposed, bit for bit
    let (
        Surface::Spline {
            u_knots: ub,
            v_knots: vb,
            points: pb,
        },
        Surface::Spline {
            u_knots: ua,
            v_knots: va,
            points: pa,
        },
    ) = (&before.faces[0].surface, &t.record.faces[0].surface)
    else {
        panic!("{:?}", t.record.faces[0].surface);
    };
    assert_eq!((ua, va), (vb, ub));
    for (i, row) in pb.iter().enumerate() {
        for (j, p) in row.iter().enumerate() {
            assert_eq!(pa[j][i], *p);
        }
    }
    // the turned record goes round the kernel as it is
    let (body, _) = Body::from_record(&t.record, true).unwrap();
    assert_eq!(body.to_record().unwrap().0, t.record);
}

#[test]
fn turned_over_twice_a_sheet_is_itself() {
    let before = sheet();
    let once = turned(&before);
    let twice = turned(&once.record);
    // each edge comes back to its own number
    for (i, &j) in once.edges.iter().enumerate() {
        assert_eq!(twice.edges[j as usize] as usize, i);
    }
    let after = &twice.record;
    assert_eq!(sides(after), sides(&before));
    assert_eq!(after.edges, before.edges);
    let ids = |e: &[OrientedEdge]| e.iter().map(|o| (o.edge, o.forward)).collect::<Vec<_>>();
    for (fb, fa) in before.faces.iter().zip(&after.faces) {
        assert_eq!(
            (&fa.surface, &fa.nurbs, fa.domain),
            (&fb.surface, &fb.nurbs, fb.domain)
        );
        // the same loops (each pcurve mirrored about u = v twice, so compared by its edges)
        assert_eq!(fa.loops.len(), fb.loops.len());
        for (la, lb) in fa.loops.iter().zip(&fb.loops) {
            match (la, lb) {
                (Loop::Edges { edges: ea }, Loop::Edges { edges: eb }) => {
                    assert_eq!(ids(ea), ids(eb));
                }
                _ => assert_eq!(la, lb),
            }
        }
    }
}

/// An analytic sheet keeps its label turned over (the kernel's cylinder with its u and v
/// swapped): if this fails on the second label, the converter does not read the swap, and the
/// record carries the cylinder as its NURBS form.
#[test]
fn a_turned_cylinder_sheet_keeps_its_label() {
    let before = quarter_cylinder();
    assert!(
        matches!(before.faces[0].surface, Surface::Cylinder { .. }),
        "the corpus sheet is labelled a cylinder: {:?}",
        before.faces[0].surface
    );
    let t = turned(&before);
    t.record.validate_built().unwrap();
    assert!(
        matches!(t.record.faces[0].surface, Surface::Cylinder { .. }),
        "turned over, the cylinder sheet is still labelled a cylinder: {:?}",
        t.record.faces[0].surface
    );
    faces_turned("the quarter cylinder", &before, &t.record, false);
    edges_mapped("the quarter cylinder", &before, &t.record, &t.edges);
}

#[test]
fn a_closed_body_is_refused_and_left_as_it_was() {
    let body = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    let (before, order) = body.to_record().unwrap();
    assert!(matches!(body.turn_over(&order), Err(Error::Unsupported(_))));
    assert_eq!(body.to_record().unwrap().0, before);
}
