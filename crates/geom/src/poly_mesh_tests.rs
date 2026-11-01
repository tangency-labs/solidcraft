use super::*;
use crate::tess::FacePlan;

/// A planar face through `pts` (all fixed), on a plane whose normal is `normal`.
fn planar_face(plan: &mut TessPlan, face: u32, pts: &[Vec3], normal: Vec3, triangles: Vec<[u32; 3]>) {
    let du = normal.any_perp();
    let dv = normal.cross(du);
    let origin = pts.first().copied().unwrap_or(Vec3::ZERO);
    let s = plan.surfaces.len() as u32;
    plan.surfaces.push(EvalSurface::Plane { origin, du, dv });
    let uv = pts.iter().map(|p| [(*p - origin).dot(du), (*p - origin).dot(dv)]).collect();
    plan.faces.push(FacePlan { face, surface: Some(s), uv, fixed: pts.to_vec(), normals: vec![], flip: false, triangles });
}

/// A face with no evaluable surface: every position and normal given.
fn free_face(plan: &mut TessPlan, face: u32, pts: Vec<Vec3>, normals: Vec<Vec3>, triangles: Vec<[u32; 3]>) {
    let uv = vec![[0.0, 0.0]; pts.len()];
    plan.faces.push(FacePlan { face, surface: None, uv, fixed: pts, normals, flip: false, triangles });
}

/// The plan evaluated with every edge flagged as a crease, as a body would flag a box.
fn creased(plan: &TessPlan) -> Mesh {
    let mut m = plan.evaluate();
    let n = plan.edges.len();
    (m.seams, m.smooth_edges, m.sharp_edges) = (vec![false; n], vec![false; n], vec![false; n]);
    m
}

/// Every index in range, every polygon at least a triangle, the arrays in step.
fn check_invariants(p: &PolyMesh) {
    let total: u32 = p.poly_sizes.iter().sum();
    assert_eq!(total as usize, p.loops.len());
    assert_eq!(p.loops.len(), p.loop_normals.len());
    assert_eq!(p.poly_sizes.len(), p.poly_face.len());
    assert!(p.poly_sizes.iter().all(|&s| s >= 3));
    assert!(p.loops.iter().all(|&v| (v as usize) < p.positions.len()));
    assert!(p.positions.iter().all(|q| q.is_finite()));
    assert!(p.loop_normals.iter().all(|n| (n.len() - 1.0).abs() < 1e-9));
    assert_eq!(p.edges.len(), p.edge_flags.len());
    assert_eq!(p.edges.len(), p.edge_brep.len());
    assert!(p.edges.iter().flatten().all(|&v| (v as usize) < p.positions.len()));
}

/// A box `a` × `b` × `c` with each face two triangles, its twelve edges in the plan.
fn box_plan(a: f64, b: f64, c: f64) -> TessPlan {
    let v = |i: usize| Vec3::new(if i & 1 == 0 { 0.0 } else { a }, if i & 2 == 0 { 0.0 } else { b }, if i & 4 == 0 { 0.0 } else { c });
    // Corners of each face counter-clockwise seen from outside, with its normal.
    let faces = [
        ([0, 2, 3, 1], -Vec3::Z),
        ([4, 5, 7, 6], Vec3::Z),
        ([0, 1, 5, 4], -Vec3::Y),
        ([2, 6, 7, 3], Vec3::Y),
        ([0, 4, 6, 2], -Vec3::X),
        ([1, 3, 7, 5], Vec3::X),
    ];
    let mut plan = TessPlan::default();
    for (k, (corners, n)) in faces.iter().enumerate() {
        let pts: Vec<Vec3> = corners.iter().map(|&i| v(i)).collect();
        planar_face(&mut plan, k as u32, &pts, *n, vec![[0, 1, 2], [0, 2, 3]]);
    }
    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
        plan.edges.push(vec![v(i), v(j)]);
        plan.edge_faces.push(vec![]);
    }
    plan
}

#[test]
fn a_box_is_six_quads_on_eight_welded_corners() {
    let plan = box_plan(10.0, 20.0, 30.0);
    for topology in [PolyTopology::NGons, PolyTopology::Quads] {
        let p = PolyMesh::from_plan(&plan, &creased(&plan), &PolyOptions { topology, ..PolyOptions::default() }).unwrap();
        check_invariants(&p);
        assert_eq!((p.positions.len(), p.poly_sizes.len(), p.edge_count()), (8, 6, 12), "{topology:?}");
        assert_eq!(p.stats, PolyStats { triangles: 0, quads: 6, ngons: 0, open_edges: 0 });
        assert!((p.volume() - 6000.0).abs() < 1e-9, "{}", p.volume());
        assert_eq!(p.edges.len(), 12);
        assert!(p.edge_flags.iter().all(|&f| f == edge_flag::BREP | edge_flag::SHARP));
        // Each corner keeps its own face's normal.
        let mut at = 0;
        for (k, &size) in p.poly_sizes.iter().enumerate() {
            let n = p.loop_normals[at];
            assert!(p.loop_normals[at..at + size as usize].iter().all(|m| m.dist(n) < 1e-12), "polygon {k}");
            at += size as usize;
        }
    }
    let tris = PolyMesh::from_plan(&plan, &creased(&plan), &PolyOptions { topology: PolyTopology::Triangles, ..PolyOptions::default() }).unwrap();
    assert_eq!((tris.positions.len(), tris.stats.triangles, tris.stats.open_edges), (8, 12, 0));
    assert!((tris.volume() - 6000.0).abs() < 1e-9);
}

#[test]
fn a_box_from_its_display_mesh_is_the_same() {
    let plan = box_plan(10.0, 20.0, 30.0);
    let p = PolyMesh::from_mesh(&creased(&plan), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_eq!((p.positions.len(), p.stats.quads, p.edge_count(), p.stats.open_edges), (8, 6, 12, 0));
    assert!((p.volume() - 6000.0).abs() < 1e-9);
    assert!(p.edge_flags.iter().all(|&f| f == edge_flag::BREP | edge_flag::SHARP));
}

#[test]
fn edges_without_flags_are_judged_from_the_normals() {
    let plan = box_plan(1.0, 1.0, 1.0);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    assert_eq!(p.edges.len(), 12);
    assert!(p.edge_flags.iter().all(|&f| f == edge_flag::BREP | edge_flag::SHARP));
}

/// Two unit squares side by side along x = 1, the second turned up by `angle` about that line,
/// their shared edge a B-rep edge.
fn hinge(angle: f64) -> TessPlan {
    let (c, s) = (angle.cos(), angle.sin());
    let left = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
    let right = vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0 + c, 0.0, s), Vec3::new(1.0 + c, 1.0, s), Vec3::new(1.0, 1.0, 0.0)];
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, left, vec![Vec3::Z; 4], vec![[0, 1, 2], [0, 2, 3]]);
    free_face(&mut plan, 1, right, vec![Vec3::new(-s, 0.0, c); 4], vec![[0, 1, 2], [0, 2, 3]]);
    plan.edges.push(vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)]);
    plan.edge_faces.push(vec![0, 1]);
    plan
}

#[test]
fn faces_that_meet_smoothly_have_a_tangent_edge() {
    for (angle, flag) in [
        (0.0, edge_flag::TANGENT),
        (0.5f64.to_radians(), edge_flag::TANGENT),
        (10f64.to_radians(), edge_flag::SHARP),
        (90f64.to_radians(), edge_flag::SHARP),
    ] {
        let plan = hinge(angle);
        let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
        assert_eq!(p.edge_flags, vec![edge_flag::BREP | flag], "{}°", angle.to_degrees());
    }
    // What the body says wins: a seam, then a certified crease, then a certified tangent join.
    let plan = hinge(0.0);
    let mut m = plan.evaluate();
    for ((seam, smooth, sharp), flag) in
        [((true, false, true), edge_flag::SURFACE_SEAM), ((false, true, true), edge_flag::SHARP), ((false, true, false), edge_flag::TANGENT)]
    {
        (m.seams, m.smooth_edges, m.sharp_edges) = (vec![seam], vec![smooth], vec![sharp]);
        let p = PolyMesh::from_plan(&plan, &m, &PolyOptions::default()).unwrap();
        assert_eq!(p.edge_flags, vec![edge_flag::BREP | flag]);
    }
}

#[test]
fn points_along_straight_edges_are_dropped() {
    // A box whose edges carry a midpoint each, every face fanned around its centre.
    let (a, b, c) = (10.0, 20.0, 30.0);
    let v = |i: usize| Vec3::new(if i & 1 == 0 { 0.0 } else { a }, if i & 2 == 0 { 0.0 } else { b }, if i & 4 == 0 { 0.0 } else { c });
    let faces = [
        ([0, 2, 3, 1], -Vec3::Z),
        ([4, 5, 7, 6], Vec3::Z),
        ([0, 1, 5, 4], -Vec3::Y),
        ([2, 6, 7, 3], Vec3::Y),
        ([0, 4, 6, 2], -Vec3::X),
        ([1, 3, 7, 5], Vec3::X),
    ];
    let mut plan = TessPlan::default();
    for (k, (corners, n)) in faces.iter().enumerate() {
        let mut pts = Vec::new();
        for i in 0..4 {
            let (p, q) = (v(corners[i]), v(corners[(i + 1) % 4]));
            pts.extend([p, (p + q) * 0.5]);
        }
        let centre = pts.iter().fold(Vec3::ZERO, |s, p| s + *p) / 8.0;
        pts.push(centre);
        let tris = (0..8u32).map(|i| [8, i, (i + 1) % 8]).collect();
        planar_face(&mut plan, k as u32, &pts, *n, tris);
    }
    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
        plan.edges.push(vec![v(i), (v(i) + v(j)) * 0.5, v(j)]);
        plan.edge_faces.push(vec![]);
    }
    let p = PolyMesh::from_plan(&plan, &creased(&plan), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_eq!((p.positions.len(), p.stats.quads, p.edge_count(), p.stats.open_edges), (8, 6, 12, 0));
    assert_eq!(p.edges.len(), 12, "each edge one segment between its ends");
    assert!((p.volume() - 6000.0).abs() < 1e-9);
    // As quads the faces keep their fans, so the midpoints stay.
    let quads = PolyMesh::from_plan(&plan, &creased(&plan), &PolyOptions { topology: PolyTopology::Quads, ..PolyOptions::default() }).unwrap();
    check_invariants(&quads);
    assert_eq!(quads.stats.open_edges, 0);
    assert!(quads.positions.len() > 8);
}

#[test]
fn a_planar_face_with_inner_points_is_one_ngon() {
    // A hexagon fanned around its centre.
    let mut pts: Vec<Vec3> = (0..6).map(|k| f64::from(k) * std::f64::consts::TAU / 6.0).map(|t| Vec3::new(t.cos(), t.sin(), 0.0)).collect();
    pts.push(Vec3::ZERO);
    let tris = (0..6u32).map(|k| [6, k, (k + 1) % 6]).collect();
    let mut plan = TessPlan::default();
    planar_face(&mut plan, 3, &pts, Vec3::Z, tris);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_eq!(p.poly_sizes, vec![6]);
    assert_eq!(p.positions.len(), 6, "the centre is dropped");
    assert_eq!(p.poly_face, vec![3]);
    assert!(p.loop_normals.iter().all(|n| n.dist(Vec3::Z) < 1e-12));
    assert_eq!((p.stats.open_edges, p.stats.ngons), (6, 1));
}

/// Every polygon convex, turning about its first corner's normal.
fn assert_convex(p: &PolyMesh) {
    let mut at = 0;
    for &size in &p.poly_sizes {
        let size = size as usize;
        let n = p.loop_normals[at];
        let q = |k: usize| p.positions[p.loops[at + k % size] as usize];
        for k in 0..size {
            let (a, v, b) = (q(k + size - 1), q(k), q(k + 1));
            assert!((v - a).cross(b - v).dot(n) >= -1e-9 * a.dist(v) * v.dist(b), "polygon at loop {at} turns the wrong way at {k}");
        }
        at += size;
    }
}

/// Area of the polygons, each projected along its first corner's normal.
fn area(p: &PolyMesh) -> f64 {
    let mut at = 0;
    let mut sum = 0.0;
    for &size in &p.poly_sizes {
        let size = size as usize;
        let q = |k: usize| p.positions[p.loops[at + k % size] as usize];
        let twice = (0..size).fold(Vec3::ZERO, |s, k| s + q(k).cross(q(k + 1)));
        sum += twice.dot(p.loop_normals[at]) * 0.5;
        at += size;
    }
    sum
}

/// Points of the square ring between ±2 and ±1, as one side of it rotated `k` quarter turns:
/// outer corner, outer midpoint, next outer corner, next inner corner, inner corner, and a point
/// inside the band (as a Delaunay mesh seeds one).
fn ring_side(k: u32) -> [(f64, f64); 6] {
    let side = [(-2.0, -2.0), (0.0, -2.0), (2.0, -2.0), (1.0, -1.0), (-1.0, -1.0), (0.0, -1.5)];
    let (c, s) = match k % 4 {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    side.map(|(x, y)| (c * x - s * y, s * x + c * y))
}

/// The square ring's face at height `z` with its midpoints and band points; facing up, or down
/// with `down`.
fn ring_face(plan: &mut TessPlan, face: u32, z: f64, down: bool) {
    let mut pts = Vec::new();
    let mut tris = Vec::new();
    for k in 0..4u32 {
        let base = pts.len() as u32;
        pts.extend(ring_side(k).iter().map(|&(x, y)| Vec3::new(x, y, z)));
        // Points of this side: 0 corner, 1 mid, 2 next corner, 3 next inner, 4 inner, 5 band.
        for [a, b] in [[0, 1], [1, 2], [2, 3], [3, 4], [4, 0]] {
            let t = [base + a, base + b, base + 5];
            tris.push(if down { [t[1], t[0], t[2]] } else { t });
        }
    }
    planar_face(plan, face, &pts, if down { -Vec3::Z } else { Vec3::Z }, tris);
}

#[test]
fn a_planar_face_with_a_hole_is_a_few_convex_ngons() {
    let mut plan = TessPlan::default();
    ring_face(&mut plan, 0, 0.0, false);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_convex(&p);
    assert!((area(&p) - 12.0).abs() < 1e-9, "{}", area(&p));
    // The band points are dropped, and the midpoints too: they only continue straight sides.
    assert_eq!(p.positions.len(), 8);
    assert!(p.poly_sizes.len() <= 5, "{:?}", p.poly_sizes);
    assert!(p.poly_face.iter().all(|&f| f == 0));
    assert!(p.loop_normals.iter().all(|n| n.dist(Vec3::Z) < 1e-12));
    // As quads, the band points stay.
    let quads = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions { topology: PolyTopology::Quads, ..PolyOptions::default() }).unwrap();
    assert_eq!(quads.positions.len(), 16);
}

#[test]
fn a_frame_with_a_hole_closes_and_drops_its_edge_midpoints() {
    // A 4 × 4 × 1 block with a 2 × 2 hole through it; the outer edges of the top and bottom
    // carry midpoints, which the outer walls share.
    let mut plan = TessPlan::default();
    ring_face(&mut plan, 0, 1.0, false);
    ring_face(&mut plan, 1, 0.0, true);
    let at = |(x, y): (f64, f64), z: f64| Vec3::new(x, y, z);
    for k in 0..4u32 {
        let s = ring_side(k);
        let (c0, m, c1, i1, i0) = (s[0], s[1], s[2], s[3], s[4]);
        let out = Vec3::new(m.0, m.1, 0.0).normalized().unwrap();
        // The outer wall, fanned round its centre.
        let wall = vec![at(c0, 0.0), at(m, 0.0), at(c1, 0.0), at(c1, 1.0), at(m, 1.0), at(c0, 1.0), at(m, 0.5)];
        planar_face(&mut plan, 2 + k, &wall, out, (0..6u32).map(|i| [6, i, (i + 1) % 6]).collect());
        // The inner wall, facing into the hole.
        let inner = vec![at(i1, 0.0), at(i0, 0.0), at(i0, 1.0), at(i1, 1.0)];
        planar_face(&mut plan, 6 + k, &inner, -out, vec![[0, 1, 2], [0, 2, 3]]);
        for z in [0.0, 1.0] {
            plan.edges.push(vec![at(c0, z), at(m, z), at(c1, z)]);
            plan.edges.push(vec![at(i0, z), at(i1, z)]);
        }
        plan.edges.push(vec![at(c0, 0.0), at(c0, 1.0)]);
        plan.edges.push(vec![at(i0, 0.0), at(i0, 1.0)]);
    }
    plan.edge_faces = vec![vec![]; plan.edges.len()];
    let p = PolyMesh::from_plan(&plan, &creased(&plan), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_convex(&p);
    assert_eq!(p.stats.open_edges, 0, "{:?}", p.stats);
    assert!((p.volume() - 12.0).abs() < 1e-9, "{}", p.volume());
    assert_eq!(p.positions.len(), 16, "the corners only: band points and edge midpoints are gone");
    assert_eq!(p.edges.len(), 24, "each B-rep edge one segment");
    // Genus one: V - E + F = 0.
    assert_eq!(p.positions.len() as i64 - p.edge_count() as i64 + p.poly_sizes.len() as i64, 0);
}

/// An `n` × `n` grid of cells on z = 0, each cell two triangles; with `flip`, wound the other
/// way round and facing down.
fn grid_plan(n: u32, flip: bool) -> TessPlan {
    let mut pts = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            pts.push(Vec3::new(f64::from(i), f64::from(j), 0.0));
        }
    }
    let at = |i: u32, j: u32| j * (n + 1) + i;
    let mut tris = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            if flip {
                tris.extend([[a, c, b], [a, d, c]]);
            } else {
                tris.extend([[a, b, c], [a, c, d]]);
            }
        }
    }
    let normals = vec![if flip { -Vec3::Z } else { Vec3::Z }; pts.len()];
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, pts, normals, tris);
    plan
}

#[test]
fn a_structured_grid_becomes_its_cells() {
    for flip in [false, true] {
        let plan = grid_plan(3, flip);
        let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
        check_invariants(&p);
        assert_eq!(p.stats, PolyStats { triangles: 0, quads: 9, ngons: 0, open_edges: 12 }, "flip {flip}");
        assert_eq!(p.positions.len(), 16);
        let up = if flip { -1.0 } else { 1.0 };
        assert!((p.volume()).abs() < 1e-12);
        assert!(p.loop_normals.iter().all(|n| (n.z - up).abs() < 1e-12));
        // Wound as the triangles were.
        let (a, b, c) = (p.positions[p.loops[0] as usize], p.positions[p.loops[1] as usize], p.positions[p.loops[2] as usize]);
        assert!((b - a).cross(c - b).z * up > 0.0);
    }
    let plan = grid_plan(3, false);
    let tris = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions { topology: PolyTopology::Triangles, ..PolyOptions::default() }).unwrap();
    assert_eq!(tris.stats.triangles, 18);
}

#[test]
fn skewed_pairs_stay_triangles() {
    // A thin sliver pair: the quad's corners are far from square.
    let pts = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(11.0, 0.5, 0.0), Vec3::new(1.0, 0.5, 0.0)];
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, pts, vec![Vec3::Z; 4], vec![[0, 1, 2], [0, 2, 3]]);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    assert_eq!(p.stats.triangles, 2);
    // Folded pairs too.
    let pts = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.0, 1.0, 0.0)];
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, pts, vec![Vec3::Z; 4], vec![[0, 1, 2], [0, 2, 3]]);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    assert_eq!(p.stats.triangles, 2);
}

/// A tube of radius 1 and height 2: `seg` × `rows` cells, with the seam column computed at
/// 2π, so it lands a rounding error away from the first column, as a separate evaluation would.
fn tube(seg: u32, rows: u32) -> TessPlan {
    let mut pts = Vec::new();
    let mut normals = Vec::new();
    for j in 0..=rows {
        for i in 0..=seg {
            let t = std::f64::consts::TAU * f64::from(i) / f64::from(seg);
            pts.push(Vec3::new(t.cos(), t.sin(), 2.0 * f64::from(j) / f64::from(rows)));
            normals.push(Vec3::new(t.cos(), t.sin(), 0.0));
        }
    }
    let at = |i: u32, j: u32| j * (seg + 1) + i;
    let mut tris = Vec::new();
    for j in 0..rows {
        for i in 0..seg {
            tris.extend([[at(i, j), at(i + 1, j), at(i + 1, j + 1)], [at(i, j), at(i + 1, j + 1), at(i, j + 1)]]);
        }
    }
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, pts, normals, tris);
    plan
}

#[test]
fn a_tube_closes_across_its_seam() {
    let plan = tube(16, 3);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_eq!(p.stats, PolyStats { triangles: 0, quads: 48, ngons: 0, open_edges: 32 });
    assert_eq!(p.positions.len(), 16 * 4);
    // Normals point out, radially.
    for (k, n) in p.loop_normals.iter().enumerate() {
        let q = p.positions[p.loops[k] as usize];
        assert!((Vec3::new(q.x, q.y, 0.0).normalized().unwrap().dot(*n) - 1.0).abs() < 1e-9);
    }
}

#[test]
fn a_pole_is_a_fan_of_triangles() {
    // A cone: the top row of a grid is the apex, repeated once per column.
    let seg = 8u32;
    let mut pts = Vec::new();
    for j in 0..=1u32 {
        for i in 0..=seg {
            let t = std::f64::consts::TAU * f64::from(i % seg) / f64::from(seg);
            pts.push(if j == 0 { Vec3::new(t.cos(), t.sin(), 0.0) } else { Vec3::new(0.0, 0.0, 1.0) });
        }
    }
    let normals = vec![Vec3::Z; pts.len()];
    let at = |i: u32, j: u32| j * (seg + 1) + i;
    let mut tris = Vec::new();
    for i in 0..seg {
        tris.extend([[at(i, 0), at(i + 1, 0), at(i + 1, 1)], [at(i, 0), at(i + 1, 1), at(i, 1)]]);
    }
    let mut plan = TessPlan::default();
    free_face(&mut plan, 0, pts, normals, tris);
    let p = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    check_invariants(&p);
    assert_eq!(p.stats, PolyStats { triangles: 8, quads: 0, ngons: 0, open_edges: 8 });
    assert_eq!(p.positions.len(), 9);
}

#[test]
fn hostile_plans_never_panic() {
    let nan = f64::NAN;
    let mut plan = TessPlan::default();
    plan.surfaces.push(EvalSurface::Plane { origin: Vec3::ZERO, du: Vec3::X, dv: Vec3::Y });
    // Parameters that are not numbers, indices out of range, more fixed points than vertices,
    // a surface that is not there, triangles with no area and repeated corners.
    plan.faces.push(FacePlan {
        face: 0,
        surface: Some(0),
        uv: vec![[nan, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        fixed: vec![],
        normals: vec![],
        flip: false,
        triangles: vec![[0, 1, 2], [1, 2, 99], [2, 2, 3], [1, 2, 3], [u32::MAX, 0, 1]],
    });
    plan.faces.push(FacePlan {
        face: 1,
        surface: Some(7),
        uv: vec![[0.0, 0.0]; 2],
        fixed: vec![Vec3::ZERO; 5],
        normals: vec![Vec3::new(nan, 0.0, 0.0)],
        flip: true,
        triangles: vec![[0, 1, 0], [0, 0, 0]],
    });
    plan.faces.push(FacePlan {
        face: 2,
        surface: None,
        uv: vec![[0.0, 0.0]; 3],
        fixed: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
        normals: vec![Vec3::ZERO; 3],
        flip: false,
        triangles: vec![[0, 1, 2]],
    });
    // Planar faces with more than one loop that are not one outer loop around holes: two
    // squares side by side, a "hole" outside its outer loop, a hole crossing it.
    let square =
        |x: f64, y: f64, s: f64| vec![Vec3::new(x, y, 0.0), Vec3::new(x + s, y, 0.0), Vec3::new(x + s, y + s, 0.0), Vec3::new(x, y + s, 0.0)];
    let mut two = square(0.0, 0.0, 1.0);
    two.extend(square(3.0, 0.0, 1.0));
    planar_face(&mut plan, 3, &two, Vec3::Z, vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]]);
    for inner in [square(5.0, 5.0, 1.0), square(1.5, 1.5, 2.0), square(-1.0, -1.0, 6.0)] {
        let mut pts = square(0.0, 0.0, 4.0);
        pts.extend(inner);
        let mut tris = Vec::new();
        for k in 0..4u32 {
            let (o0, o1, i0, i1) = (k, (k + 1) % 4, 4 + k, 4 + (k + 1) % 4);
            tris.extend([[o0, o1, i1], [o0, i1, i0]]);
        }
        planar_face(&mut plan, 4, &pts, Vec3::Z, tris);
    }
    plan.edges = vec![vec![], vec![Vec3::new(nan, 0.0, 0.0), Vec3::X], vec![Vec3::X, Vec3::X]];
    plan.edge_faces = vec![vec![0]];
    for topology in [PolyTopology::Triangles, PolyTopology::Quads, PolyTopology::NGons] {
        for weld_eps in [0.0, nan, -1.0, f64::INFINITY, 1e300] {
            let o = PolyOptions { topology, weld_eps, max_corner_dev: nan, max_fold: nan, tangent_angle: nan };
            check_invariants(&PolyMesh::from_plan(&plan, &Mesh::default(), &o).unwrap());
            check_invariants(&PolyMesh::from_plan(&plan, &plan.evaluate(), &o).unwrap());
            check_invariants(&PolyMesh::from_mesh(&plan.evaluate(), &o).unwrap());
        }
    }
    let m = Mesh { positions: vec![Vec3::new(nan, 0.0, 0.0), Vec3::X, Vec3::Y], triangles: vec![[0, 1, 2], [5, 6, 7], [1, 2, 2]], ..Mesh::default() };
    check_invariants(&PolyMesh::from_mesh(&m, &PolyOptions::default()).unwrap());
    assert_eq!(PolyMesh::from_mesh(&Mesh::default(), &PolyOptions::default()).unwrap(), PolyMesh::default());
}

#[test]
fn the_same_plan_gives_the_same_mesh() {
    let plan = tube(12, 2);
    let a = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    let b = PolyMesh::from_plan(&plan, &plan.evaluate(), &PolyOptions::default()).unwrap();
    assert_eq!(a, b);
}
