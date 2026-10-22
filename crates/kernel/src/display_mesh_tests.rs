//! The display mesh of a large body with small radii: bends and holes look round, the mesh
//! stays closed, and the cost stays bounded.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_geom::{Loop2, Mesh, Plane, Region2, Seg2, UnitVec3, Vec2, Vec3};

use super::{Body, DISPLAY_ANGLE};
use crate::{FaceSurface, extrude, face_surface};

/// The chord tolerance the application draws a body with.
fn display_tol(b: &Body) -> f64 {
    (b.size() * 1e-3).max(1e-3)
}

fn rect(w: f64, h: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)])
}

/// A `length` long angle of 2 mm sheet: a 100 mm leg along X, a 90 degree bend of inner
/// radius 1 (outer 3) about the line x = y = 3, and a 40 mm leg along Y, extruded along Z.
fn bent_sheet(length: f64) -> Body {
    let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
    let c = Vec2::new(3.0, 3.0);
    let outline = Loop2 {
        segs: vec![
            line((100.0, 0.0), (100.0, 2.0)),
            line((100.0, 2.0), (3.0, 2.0)),
            Seg2::Arc { center: c, radius: 1.0, start: 1.5 * PI, sweep: -FRAC_PI_2 },
            line((2.0, 3.0), (2.0, 40.0)),
            line((2.0, 40.0), (0.0, 40.0)),
            line((0.0, 40.0), (0.0, 3.0)),
            Seg2::Arc { center: c, radius: 3.0, start: PI, sweep: FRAC_PI_2 },
            line((3.0, 0.0), (100.0, 0.0)),
        ],
    };
    assert!(outline.signed_area() > 0.0);
    extrude(&Plane::XY, &[Region2 { outer: outline, holes: vec![] }], 0.0, length).unwrap().pop().unwrap()
}

/// A 3000 x 1000 x 2 plate with round holes of `radius` at `centres`.
fn plate_with_holes(centres: &[Vec2], radius: f64) -> Body {
    let holes = centres.iter().map(|c| Loop2::circle(*c, radius).reversed()).collect();
    extrude(&Plane::XY, &[Region2 { outer: rect(3000.0, 1000.0), holes }], 0.0, 2.0).unwrap().pop().unwrap()
}

/// The faces of a body on cylinders of this radius.
fn cylinder_faces(b: &Body, radius: f64) -> Vec<(usize, Vec3, UnitVec3)> {
    (0..b.face_count())
        .filter_map(|i| match face_surface(b, i)? {
            FaceSurface::Cylinder { origin, axis, radius: r } if (r - radius).abs() < 1e-6 => Some((i, origin, axis)),
            _ => None,
        })
        .collect()
}

/// Distance of a point from an axis.
fn radial(p: Vec3, origin: Vec3, axis: Vec3) -> f64 {
    let d = p - origin;
    (d - axis * d.dot(axis)).len()
}

/// How a face on a cylinder is divided around its axis: the angles (radians, sorted, from the
/// first) at which it has vertices.
fn angles_around(m: &Mesh, face: usize, origin: Vec3, axis: Vec3) -> Vec<f64> {
    let u = axis.any_perp();
    let v = axis.cross(u);
    let mut a: Vec<f64> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| **f as usize == face)
        .flat_map(|(t, _)| t.iter())
        .filter_map(|k| m.positions.get(*k as usize))
        .map(|p| (*p - origin).dot(v).atan2((*p - origin).dot(u)))
        .collect();
    a.sort_by(f64::total_cmp);
    // Start at the widest gap, so an arc that crosses ±π stays in one piece.
    let n = a.len();
    let gap = |i: usize| (a[(i + 1) % n] - a[i]).rem_euclid(TAU);
    let start = (0..n).max_by(|i, j| gap(*i).total_cmp(&gap(*j))).map_or(0, |i| (i + 1) % n);
    let first = a[start];
    let mut out: Vec<f64> = (0..n).map(|i| (a[(start + i) % n] - first).rem_euclid(TAU)).collect();
    out.dedup_by(|x, y| (*x - *y).abs() < 1e-7);
    out
}

/// The most a triangle of `face` lies inside the cylinder it is on (its corners are on it).
fn sag(m: &Mesh, face: usize, origin: Vec3, axis: Vec3, radius: f64) -> f64 {
    let mut worst = 0.0f64;
    for (t, _) in m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == face) {
        let [a, b, c] = m.tri(t).unwrap();
        for p in [a, b, c] {
            assert!((radial(p, origin, axis) - radius).abs() < 1e-6, "a corner off the cylinder");
        }
        for p in [(a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5, (a + b + c) / 3.0] {
            worst = worst.max((radius - radial(p, origin, axis)).abs());
        }
    }
    worst
}

/// Triangle sides that are not shared by exactly two triangles once corners closer than `eps`
/// are joined (a closed mesh has none).
fn open_sides(m: &Mesh, eps: f64) -> usize {
    let cell = |p: &Vec3| ((p.x / eps).floor() as i64, (p.y / eps).floor() as i64, (p.z / eps).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    let mut points: Vec<Vec3> = Vec::new();
    let ids: Vec<usize> = m
        .positions
        .iter()
        .map(|p| {
            let (cx, cy, cz) = cell(p);
            for (dx, dy, dz) in (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| (x, y, z)))) {
                if let Some(i) = grid.get(&(cx + dx, cy + dy, cz + dz)).and_then(|v| v.iter().find(|i| points[**i].dist(*p) < eps)) {
                    return *i;
                }
            }
            points.push(*p);
            grid.entry((cx, cy, cz)).or_default().push(points.len() - 1);
            points.len() - 1
        })
        .collect();
    let mut sides: HashMap<(usize, usize), i32> = HashMap::new();
    for t in &m.triangles {
        let [a, b, c] = t.map(|k| ids[k as usize]);
        // A triangle at a pole has two corners in one point: no area, no sides.
        if a == b || b == c || c == a {
            continue;
        }
        for (p, q) in [(a, b), (b, c), (c, a)] {
            *sides.entry((p.min(q), p.max(q))).or_default() += 1;
        }
    }
    sides.values().filter(|n| **n != 2).count()
}

/// The polyline segments around a circle of `radius` about `centre` in the plane z = `z`,
/// and the widest angle one of them spans.
fn rim(m: &Mesh, centre: Vec2, radius: f64, z: f64) -> (usize, f64) {
    let on = |p: &Vec3| (p.z - z).abs() < 1e-6 && (Vec2::new(p.x, p.y).dist(centre) - radius).abs() < 1e-6;
    let (mut segments, mut widest) = (0, 0.0f64);
    for e in m.edges.iter().filter(|e| e.len() >= 2 && e.iter().all(on)) {
        for w in e.windows(2) {
            segments += 1;
            widest = widest.max(2.0 * (w[0].dist(w[1]) / (2.0 * radius)).clamp(0.0, 1.0).asin());
        }
    }
    (segments, widest)
}

#[test]
fn a_small_bend_on_a_large_sheet_is_meshed_round() {
    let b = bent_sheet(3000.0);
    let tol = display_tol(&b);
    // The tolerance is larger than either side's sagitta: 1 - cos 45° of the radius.
    assert!(tol > 3.0 * (1.0 - (PI / 4.0).cos()));
    let coarse = b.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
    // A quarter turn in facets of at most DISPLAY_ANGLE.
    let facets = (FRAC_PI_2 / DISPLAY_ANGLE).ceil() as usize;
    assert_eq!(facets, 5);
    for radius in [1.0, 3.0] {
        let faces = cylinder_faces(&b, radius);
        assert_eq!(faces.len(), 1, "one bend face of radius {radius}");
        let (face, origin, axis) = faces[0];
        // The chord tolerance alone: one flat strip from tangent line to tangent line.
        assert_eq!(angles_around(&coarse, face, origin, *axis).len(), 2, "radius {radius}");
        assert!(sag(&coarse, face, origin, *axis, radius) > 0.29 * radius);
        let a = angles_around(&round, face, origin, *axis);
        assert!(a.len() > facets, "radius {radius}: {} facets across the bend", a.len() - 1);
        assert!((a[a.len() - 1] - FRAC_PI_2).abs() < 1e-6, "the bend is a quarter turn: {a:?}");
        let widest = a.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        assert!(widest <= DISPLAY_ANGLE * 1.001, "radius {radius}: a facet spans {} degrees", widest.to_degrees());
        // Within the chord tolerance of the cylinder, and by much: a facet that turns no more
        // than the angle sags no more than 1 - cos(angle / 2) of the radius.
        let s = sag(&round, face, origin, *axis, radius);
        assert!(s <= radius * (1.0 - (DISPLAY_ANGLE / 2.0).cos()) * 1.001 && s < tol, "radius {radius}: sag {s}");
    }
    // The flat faces along the sheet meet the bends in straight lines and need nothing more:
    // only the bends, and the two ends where their arcs now have more points, gain triangles.
    let count = |m: &Mesh, face: usize| m.tri_face.iter().filter(|f| **f as usize == face).count();
    let mut along = 0;
    for face in 0..b.face_count() {
        if let Some(FaceSurface::Plane { normal, .. }) = face_surface(&b, face)
            && normal.dot(Vec3::Z).abs() < 0.5
        {
            along += 1;
            assert_eq!(count(&round, face), count(&coarse, face), "face {face}");
        }
    }
    assert_eq!(along, 6);
}

#[test]
fn the_round_display_mesh_stays_closed_and_keeps_its_edges_and_bounds() {
    let b = bent_sheet(3000.0);
    let tol = display_tol(&b);
    let coarse = b.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
    // Neighbouring faces share the points of the edge between them.
    assert_eq!(open_sides(&round, 1e-6), 0);
    // The same edges in the same order, between the same faces, with the same ends and flags.
    assert_eq!(round.edges.len(), coarse.edges.len());
    assert_eq!(round.edge_faces, coarse.edge_faces);
    assert_eq!((&round.seams, &round.smooth_edges, &round.sharp_edges), (&coarse.seams, &coarse.smooth_edges, &coarse.sharp_edges));
    for (fine, rough) in round.edges.iter().zip(&coarse.edges) {
        assert!(fine.len() >= rough.len());
        assert!(fine[0].dist(rough[0]) < 1e-9 && fine[fine.len() - 1].dist(rough[rough.len() - 1]) < 1e-9);
    }
    // The body's box: flat faces and tangent lines bound it, so nothing moves.
    let (r, c) = (round.bounds(), coarse.bounds());
    assert!(r.min.dist(c.min) < 1e-9 && r.max.dist(c.max) < 1e-9);
    // Closer to the solid: the flat strips cut the outer bend's corner off and fill the inner one.
    let exact = crate::measure(&b).unwrap().volume;
    assert!((round.measure().volume - exact).abs() < (coarse.measure().volume - exact).abs());
}

#[test]
fn a_small_hole_on_a_large_plate_has_a_round_rim() {
    let centre = Vec2::new(30.0, 20.0);
    let b = plate_with_holes(&[centre], 2.0);
    let tol = display_tol(&b);
    assert!(tol > 2.0, "the hole's radius is below the chord tolerance");
    let coarse = b.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
    let segments = (TAU / DISPLAY_ANGLE).ceil() as usize;
    assert_eq!(segments, 18);
    for z in [0.0, 2.0] {
        // The chord tolerance alone leaves each half circle one segment, the rim a line no face
        // can be bounded by: its edges are divided again, and the rim stays a coarse polygon.
        let (n, _) = rim(&coarse, centre, 2.0, z);
        assert!((3..segments).contains(&n), "{n} segments around the rim at the chord tolerance");
        let (n, widest) = rim(&round, centre, 2.0, z);
        assert!(n >= segments, "{n} segments around the rim at z = {z}");
        assert!(widest <= DISPLAY_ANGLE * 1.001, "a segment spans {} degrees", widest.to_degrees());
    }
    // The hole's wall follows its rims.
    for (face, origin, axis) in cylinder_faces(&b, 2.0) {
        let a = angles_around(&round, face, origin, *axis);
        assert!(a.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max) <= DISPLAY_ANGLE * 1.001);
        assert!(sag(&round, face, origin, *axis, 2.0) <= 2.0 * (1.0 - (DISPLAY_ANGLE / 2.0).cos()) * 1.001);
    }
    assert_eq!(open_sides(&round, 1e-6), 0);
}

#[test]
fn an_arc_the_chord_tolerance_already_divides_finely_is_left_alone() {
    // A 40 mm cylinder at a tolerance of 0.01 mm: 64 facets around, far inside the angle.
    let b = crate::cylinder(Vec3::ZERO, Vec3::Z, 20.0, 30.0).unwrap();
    let coarse = b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(DISPLAY_ANGLE) }).unwrap();
    assert_eq!(round.positions, coarse.positions);
    assert_eq!(round.triangles, coarse.triangles);
    assert_eq!(round.edges, coarse.edges);
    // And a body with nothing curved at any tolerance.
    let b = crate::box_solid(Vec3::ZERO, Vec3::new(3000.0, 1000.0, 2.0)).unwrap();
    let tol = display_tol(&b);
    assert_eq!(
        b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap().positions,
        b.mesh(&crate::MeshTolerance::chord(tol)).unwrap().positions
    );
}

#[test]
fn a_radius_far_below_the_tolerance_keeps_its_chord_facets() {
    // A half circle of radius 0.002 in a 3 m plate sags less than a thousandth of the chord
    // tolerance: below that floor nothing is divided for the angle, however far the body
    // outgrows its smallest radius.
    let centre = Vec2::new(30.0, 20.0);
    let b = plate_with_holes(&[centre], 0.002);
    let tol = display_tol(&b);
    let coarse = b.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
    assert_eq!(round.triangles.len(), coarse.triangles.len());
    assert_eq!(round.edges, coarse.edges);
    // Just above the floor the rim is divided, but coarsely: the angle stops where the sag
    // falls below the floor, not where the facets reach the angle.
    let b = plate_with_holes(&[centre], 0.01);
    let round = b.mesh(&crate::MeshTolerance { chord: display_tol(&b), angle: Some(DISPLAY_ANGLE) }).unwrap();
    let (n, _) = rim(&round, centre, 0.01, 2.0);
    assert!((3..18).contains(&n), "{n} segments");
}

#[test]
fn a_face_crowded_with_holes_gets_a_coarser_angle_and_bounded_work() {
    // 1000 holes of diameter 4 in a 3 m plate. At the full angle the top and bottom faces
    // would each be bounded by more than 20 000 segments, and the work of triangulating a
    // face grows faster than its boundary.
    let centres: Vec<Vec2> = (0..50).flat_map(|i| (0..20).map(move |j| Vec2::new(30.0 + 60.0 * i as f64, 25.0 + 50.0 * j as f64))).collect();
    let b = plate_with_holes(&centres, 2.0);
    let tol = display_tol(&b);
    let coarse = b.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
    let round = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
    let boundary = |m: &Mesh, z: f64| m.edges.iter().filter(|e| e.iter().all(|p| (p.z - z).abs() < 1e-6)).map(|e| e.len() - 1).sum::<usize>();
    let budget = crate::tess::size::MAX_ANGLE_BOUNDARY;
    for z in [0.0, 2.0] {
        // At the chord tolerance alone each rim is divided only as far as a face can be bounded
        // by it (two segments, a line, are divided again).
        assert!(boundary(&coarse, z) < 6 * centres.len(), "{}", boundary(&coarse, z));
        let n = boundary(&round, z);
        let (rounder, chord) = (rim(&round, centres[0], 2.0, z).0, rim(&coarse, centres[0], 2.0, z).0);
        assert!(rounder >= chord, "the rims are at least as round as the chord tolerance leaves them: {rounder} < {chord}");
        assert!(n <= budget, "{n} segments bound the face at z = {z}");
    }
    // Every rim got the same share, and the walls follow their rims.
    let per_rim: Vec<usize> = centres.iter().map(|c| rim(&round, *c, 2.0, 2.0).0).collect();
    assert!(per_rim.iter().all(|n| *n == per_rim[0] && *n >= 3), "{:?}", &per_rim[..4]);
    assert!(
        round.triangles.len() < 4 * coarse.triangles.len(),
        "{} triangles, {} at the chord tolerance",
        round.triangles.len(),
        coarse.triangles.len()
    );
    assert_eq!(open_sides(&round, 1e-6), 0);
    // A plate with few holes keeps the full angle.
    let few = plate_with_holes(&centres[..40], 2.0);
    let round = few.mesh(&crate::MeshTolerance { chord: display_tol(&few), angle: Some(DISPLAY_ANGLE) }).unwrap();
    assert!(rim(&round, centres[0], 2.0, 2.0).0 >= 18);
}

#[test]
fn round_faces_of_every_kind_are_divided_by_the_angle() {
    // A sphere, a torus and a cone a few millimetres across, drawn at the 3 mm tolerance of a
    // body 3 m long: more than any of their sagittas.
    let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, 4.0), holes: vec![] };
    let bodies = [
        ("sphere", crate::sphere(Vec3::ZERO, 4.0).unwrap()),
        ("torus", crate::torus(Vec3::ZERO, 6.0, 2.0).unwrap()),
        ("cone", crate::extrude_tapered(&Plane::XY, &disc, 6.0, 1.0, -0.3, crate::DraftCorner::default()).unwrap()),
    ];
    // The most the surface normal turns along one side of a triangle of a curved face.
    let turn = |b: &Body, m: &Mesh| {
        let flat: Vec<bool> = (0..b.face_count()).map(|i| matches!(face_surface(b, i), Some(FaceSurface::Plane { .. }))).collect();
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, f)| !flat[**f as usize])
            .flat_map(|(t, _)| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
            .filter(|(a, b)| m.positions[*a as usize].dist(m.positions[*b as usize]) > 1e-9)
            .map(|(a, b)| m.normals[a as usize].dot(m.normals[b as usize]).clamp(-1.0, 1.0).acos())
            .fold(0.0, f64::max)
    };
    let check = |name: &str, b: &Body| {
        let coarse = b.mesh(&crate::MeshTolerance::chord(3.0)).unwrap();
        let round = b.mesh(&crate::MeshTolerance { chord: 3.0, angle: Some(DISPLAY_ANGLE) }).unwrap();
        // The chord tolerance alone leaves facets coarser than the angle (the size rules' longest
        // side and aspect limit already keep them near 40°), so the angle has work to do.
        assert!(turn(b, &coarse) > 1.5 * DISPLAY_ANGLE, "{name}: the chord tolerance leaves facets of {} degrees", turn(b, &coarse).to_degrees());
        // A side crosses at most one facet in each direction.
        assert!(turn(b, &round) <= 1.5 * DISPLAY_ANGLE, "{name}: a side turns {} degrees", turn(b, &round).to_degrees());
        assert_eq!(open_sides(&round, 1e-6), 0, "{name}");
        // Bounded: no more than a full turn of facets each way on every face.
        assert!(round.triangles.len() <= b.face_count() * 2 * 36 * 36, "{name}: {} triangles", round.triangles.len());
    };
    for (name, b) in &bodies {
        check(name, b);
    }
}

/// The signed volume a closed mesh encloses.
fn mesh_volume(m: &Mesh) -> f64 {
    m.triangles.iter().filter_map(|t| m.tri(t)).map(|[a, b, c]| a.dot(b.cross(c)) / 6.0).sum()
}

#[test]
fn a_cylinder_wall_split_along_its_seam_meshes_round() {
    // A cup: a Ø100 × 10 disc with a Ø96 pocket 8 deep cut into its top, as Shell leaves it.
    // The cut splits the outer wall's straight seam edges at the pocket's floor (z = 2); the
    // wall used to be meshed as fans from those two vertices to the far rims, triangles that cut
    // through the disc (the shelled Bolted Flange sample showed its rim cut off).
    let disc = |r: f64, z0: f64, z1: f64| {
        extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::ZERO, r), holes: vec![] }], z0, z1).unwrap().pop().unwrap()
    };
    let cup = crate::ops::boolean(&disc(50.0, 0.0, 10.0), &disc(48.0, 2.0, 20.0), crate::BoolOp::Cut).unwrap().unwrap();
    let exact = PI * (50.0 * 50.0 * 10.0 - 48.0 * 48.0 * 8.0);
    let outer: Vec<bool> = (0..cup.face_count())
        .map(|i| matches!(face_surface(&cup, i), Some(FaceSurface::Cylinder { radius, .. }) if (radius - 50.0).abs() < 1e-6))
        .collect();
    assert_eq!(outer.iter().filter(|o| **o).count(), 2);
    for tol in [display_tol(&cup), 0.1, 0.01] {
        let m = cup.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
        assert_eq!(open_sides(&m, 1e-6), 0);
        let v = mesh_volume(&m);
        assert!((v - exact).abs() < exact * 0.01, "tolerance {tol}: mesh volume {v:.1}, exact {exact:.1}");
        // Every vertex of the outer wall's triangles lies on it, and none spans its height
        // from one of the split vertices across the rim.
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            if !outer.get(*f as usize).copied().unwrap_or(false) {
                continue;
            }
            let [a, b, c] = m.tri(t).unwrap();
            let centre = (a + b + c) * (1.0 / 3.0);
            let r = Vec2::new(centre.x, centre.y).len();
            assert!(r > 50.0 - 10.0 * tol.max(0.05), "tolerance {tol}: an outer wall triangle passes {r:.2} mm from the axis");
        }
    }
}

/// A body's outline box, made without meshing, holds its display mesh and is not much larger.
#[test]
fn outline_bounds_hold_the_display_mesh() {
    let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let rounded = crate::fillet(&cube, &[Vec3::new(20., 0., 10.)], 3.).unwrap();
    let rod = crate::cylinder(Vec3::new(5., 5., 0.), Vec3::Z, 4., 25.).unwrap();
    for body in [cube, rounded, rod] {
        let outline = body.outline_bounds();
        let mesh = body.mesh(&crate::MeshTolerance { chord: body.size() * 1e-3, angle: Some(crate::DISPLAY_ANGLE) }).unwrap().bounds();
        let slack = body.size() * 1e-6;
        for (lo, hi, m_lo, m_hi) in [
            (outline.min.x, outline.max.x, mesh.min.x, mesh.max.x),
            (outline.min.y, outline.max.y, mesh.min.y, mesh.max.y),
            (outline.min.z, outline.max.z, mesh.min.z, mesh.max.z),
        ] {
            assert!(lo <= m_lo + slack && hi >= m_hi - slack, "outline {outline:?} holds the mesh {mesh:?}");
        }
        assert!(outline.diagonal() <= mesh.diagonal() * 1.5, "and is not much larger");
    }
}
