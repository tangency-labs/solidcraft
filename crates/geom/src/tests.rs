use crate::*;

fn cube_mesh(s: f64) -> Mesh {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x * s, y * s, z * s);
    let positions = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)];
    let triangles =
        vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
    let n = triangles.len();
    Mesh {
        normals: vec![Vec3::Z; 8],
        positions,
        triangles,
        tri_face: vec![0; n],
        edges: vec![],
        display_only_edges: false,
        seams: vec![],
        smooth_edges: vec![],
        sharp_edges: vec![],
        edge_faces: vec![],
    }
}

#[test]
fn display_edge_masks_keep_identity_and_break_only_certified_sharp_patches() {
    let mut mesh =
        Mesh { edges: vec![vec![Vec3::ZERO, Vec3::Z]; 2], edge_faces: vec![vec![0, 1], vec![1, 2]], seams: vec![true, true], ..Mesh::default() };
    let stored = serde_json::to_value(&mesh).unwrap();
    assert_eq!(mesh.surface_patch(0), vec![0, 1, 2]);
    mesh.smooth_edges = vec![true, false];
    assert_eq!(mesh.surface_patch(0), vec![0, 1, 2]);
    assert!(!mesh.shaded_edge_visible(0));
    mesh.smooth_edges[0] = false;
    mesh.sharp_edges = vec![true, false];
    assert!(mesh.shaded_edge_visible(0));
    assert!(mesh.edge_pickable(0));
    assert!(!mesh.edge_pickable(1));
    assert_eq!(mesh.surface_patch(0), vec![0]);
    assert_eq!(mesh.surface_patch(1), vec![1, 2]);
    assert_eq!(mesh.edges.len(), 2);
    assert_eq!(mesh.face_edges(1), vec![0, 1]);
    mesh.display_only_edges = true;
    assert!(mesh.edge_visible(0) && mesh.shaded_edge_visible(0));
    assert!(!mesh.edge_pickable(0));
    assert_eq!(serde_json::to_value(&mesh).unwrap(), stored);
    let reopened: Mesh = serde_json::from_value(stored).unwrap();
    assert!(reopened.smooth_edges.is_empty() && reopened.sharp_edges.is_empty());
    assert!(Mesh::default().shaded_edge_visible(0));
}

#[test]
fn cube_measure() {
    let m = cube_mesh(2.0).measure();
    assert!((m.volume - 8.0).abs() < 1e-12);
    assert!((m.area - 24.0).abs() < 1e-12);
    assert!(m.centroid.dist(Vec3::new(1.0, 1.0, 1.0)) < 1e-12);
}

#[test]
fn raycast_hits_front_face() {
    let m = cube_mesh(2.0);
    let (t, _) = m.raycast(Vec3::new(1.0, 1.0, 10.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
    assert!((t - 8.0).abs() < 1e-9);
    assert!(m.raycast(Vec3::new(5.0, 5.0, 10.0), Vec3::new(0.0, 0.0, -1.0)).is_none());
}

/// A ray through a vertex or edge that triangles share never slips between them: a grid of
/// triangles on a square, hit at every inner vertex and edge midpoint from oblique directions
/// (on the square's own border nothing is shared, and a ray grazing it may miss).
#[test]
fn raycast_never_slips_between_triangles() {
    let n = 6;
    let positions: Vec<Vec3> =
        (0..=n).flat_map(|j| (0..=n).map(move |i| Vec3::new(f64::from(i) * 40.0 / 6.0 + 100.0, f64::from(j) * 5.0, 20.0))).collect();
    let id = |i: u32, j: u32| j * (n + 1) + i;
    let mut triangles = Vec::new();
    for j in 0..n {
        for i in 0..n {
            triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    let tri_face = vec![0; triangles.len()];
    let m = Mesh { positions, triangles, tri_face, ..Default::default() };
    let dirs = [Vec3::new(-0.57735, 0.57735, -0.57735), Vec3::new(0.3, -0.7, -0.648), Vec3::new(-0.123456789, -0.2, -1.0), Vec3::new(0.0, 0.0, -1.0)];
    let mut targets = Vec::new();
    for j in 1..2 * n {
        for i in 1..2 * n {
            targets.push(Vec3::new(f64::from(i) * 20.0 / 6.0 + 100.0, f64::from(j) * 2.5, 20.0));
        }
    }
    for d in dirs {
        for p in &targets {
            let o = *p - d * 50.0;
            let hit = m.raycast(o, d);
            assert!(hit.is_some_and(|(t, _)| (t - 50.0).abs() < 1e-6), "{p:?} along {d:?}: {hit:?}");
        }
    }
}

#[test]
fn planes() {
    assert_eq!(Plane::XY.normal(), Vec3::Z);
    assert_eq!(Plane::XZ.normal(), Vec3::Y);
    assert_eq!(Plane::YZ.normal(), Vec3::X);
    let p = Plane::XZ.to_world(Vec2::new(3.0, 4.0));
    assert_eq!(p, Vec3::new(3.0, 0.0, -4.0));
    assert_eq!(Plane::XZ.to_local(p), Vec2::new(3.0, 4.0));
    let hit = Plane::XY.offset(5.0).intersect_ray(Vec3::new(1.0, 2.0, 10.0), Vec3::new(0.0, 0.0, -2.0)).unwrap();
    assert!(hit.dist(Vec3::new(1.0, 2.0, 5.0)) < 1e-12);
    assert!(Plane::new(Vec3::ZERO, Vec3::X, Vec3::X).is_none());
    let f = Plane::from_normal(Vec3::ZERO, Vec3::new(0.0, 0.0, 3.0)).unwrap();
    assert!((f.normal() - Vec3::Z).len() < 1e-12);
    assert!(Plane::from_normal(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0)).is_none());
}

#[test]
fn vec_ops_and_serde() {
    let a = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(a.cross(Vec3::X), Vec3::new(0.0, 3.0, -2.0));
    assert_eq!(Vec2::new(3.0, 4.0).len(), 5.0);
    assert!(Vec2::ZERO.normalized().is_none());
    assert!(Vec3::new(0.5, 0.0, 0.0).dist_to_segment(Vec3::ZERO, Vec3::X) < 1e-12);
    assert_eq!(serde_json::to_string(&a).unwrap(), "[1.0,2.0,3.0]");
    let b: Vec2 = serde_json::from_str("[1,2]").unwrap();
    assert_eq!(b, Vec2::new(1.0, 2.0));
}

#[test]
fn aabb() {
    let mut b = Aabb3::EMPTY;
    assert!(b.is_empty());
    b.add(Vec3::new(1.0, 2.0, 3.0));
    b.add(Vec3::new(f64::NAN, 0.0, 0.0));
    b.add(Vec3::new(-1.0, 0.0, 0.0));
    assert_eq!(b.size(), Vec3::new(2.0, 2.0, 3.0));
}

#[test]
fn tangent_chains_follow_smooth_edges() {
    // A line, then an arc leaving it tangentially, then a sharp corner.
    let mut m = Mesh { positions: vec![Vec3::ZERO, Vec3::new(30.0, 30.0, 0.0)], ..Default::default() };
    m.edges.push(vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)]);
    let arc: Vec<Vec3> = (0..=16)
        .map(|k| {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * k as f64 / 16.0;
            Vec3::new(10.0 + 5.0 * a.cos(), 5.0 + 5.0 * a.sin(), 0.0)
        })
        .collect();
    m.edges.push(arc);
    m.edges.push(vec![Vec3::new(15.0, 5.0, 0.0), Vec3::new(5.0, 5.0, 0.0)]);
    m.edge_faces = vec![vec![0], vec![0, 1], vec![1]];
    let mut c = m.tangent_chain(0, 5f64.to_radians());
    c.sort();
    assert_eq!(c, vec![0, 1]);
    assert_eq!(m.tangent_chain(2, 5f64.to_radians()), vec![2]);
    assert_eq!(m.face_edges(1), vec![1, 2]);
    // A smooth seam where two faces share the same normal along the edge is not a corner to round,
    // so tangent chaining stops at it.
    let seam = Mesh {
        positions: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), Vec3::new(15.0, 5.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 3], [1, 2, 3], [1, 2, 3]],
        tri_face: vec![0, 1, 2],
        edges: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)], vec![Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0)]],
        edge_faces: vec![vec![0], vec![1, 2]],
        ..Default::default()
    };
    assert_eq!(seam.tangent_chain(0, 5f64.to_radians()), vec![0]);
}

/// A helix of `pieces` edge pieces (a thread's crest), each five points long.
fn helix(pieces: usize) -> Mesh {
    let at = |t: f64| Vec3::new(4.0 * t.cos(), 4.0 * t.sin(), 0.2 * t);
    let step = 0.05;
    let mut m = Mesh::default();
    for k in 0..pieces {
        let t0 = k as f64 * step * 4.0;
        m.edges.push((0..=4).map(|j| at(t0 + j as f64 * step)).collect());
        m.edge_faces.push(vec![0]);
    }
    // The mesh's points (its size sets the join tolerance).
    m.positions = m.edges.iter().flatten().copied().collect();
    m
}

/// The tangent chain of a helix of `pieces` pieces: every piece of it joins, and how long finding
/// the chain took. The mesh is built before the clock starts.
fn helix_chain(pieces: usize) -> std::time::Duration {
    let m = helix(pieces);
    let began = std::time::Instant::now();
    let chain = m.tangent_chain(pieces / 2, 2f64.to_radians());
    let took = began.elapsed();
    let mut sorted = chain.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..pieces).collect::<Vec<_>>(), "every piece of the helix is in the chain");
    took
}

/// Whether a job four times the size took at most about twice linear: `pair` times the small job,
/// then the big one. Linear is 4x, the quadratic that the bug cost 16x or more, so eight times the
/// small time is the limit. The load of the other test binaries can rise between the two runs, so a
/// pair that misses is measured again, up to three times. Load only ever slows a run, so each size
/// keeps its fastest time and the ratio is taken of those: a retry cannot pass a regression on a
/// small run that a spike happened to slow, once an earlier small run was fast.
///
/// `slack` is the room the noise of a busy machine gets, on top of the ratio.
fn scales_linearly(mut pair: impl FnMut() -> (std::time::Duration, std::time::Duration), slack: std::time::Duration) -> Result<(), String> {
    let limit = |small: f64| 8.0 * small + slack.as_secs_f64();
    let (mut small, mut big) = (f64::INFINITY, f64::INFINITY);
    let mut runs = Vec::new();
    for _ in 0..3 {
        let (s, b) = pair();
        small = small.min(s.as_secs_f64());
        big = big.min(b.as_secs_f64());
        runs.push(format!("{s:?} then {b:?}"));
        if big < limit(small) {
            return Ok(());
        }
    }
    Err(format!("{}; fastest {small:.3} s and {big:.3} s, limit {:.3} s", runs.join(", "), limit(small)))
}

#[test]
fn scales_linearly_keeps_each_sizes_fastest_run() {
    let pairs = |p: Vec<(u64, u64)>| {
        let mut p = p.into_iter().map(|(s, b)| (std::time::Duration::from_millis(s), std::time::Duration::from_millis(b)));
        move || p.next().unwrap_or((std::time::Duration::from_secs(1), std::time::Duration::from_secs(1)))
    };
    let slack = std::time::Duration::from_millis(0);
    // Linear: four times the work, four times the time.
    assert!(scales_linearly(pairs(vec![(5, 20)]), slack).is_ok());
    // A big run slowed by load, then a quiet pair.
    assert!(scales_linearly(pairs(vec![(5, 80), (5, 20)]), slack).is_ok());
    // A quadratic cost (16x) whose later small runs a spike slowed: the fast first one still counts.
    let r = scales_linearly(pairs(vec![(5, 80), (15, 80), (15, 80)]), slack);
    assert!(r.as_ref().is_err_and(|e| e.contains("fastest 0.005 s and 0.080 s, limit 0.040 s")), "{r:?}");
}

/// #3745: the tangent chain of a long helix (a thread's crest, in many edge pieces) is found
/// without scanning every edge for every edge of it: hovering a threaded part's edge looked for
/// its rim this way, and took seconds inside the frame. Every piece joins, in edge order.
///
/// A ratio and not a limit: a test must not depend on wall-clock timing (CONTRIBUTING.md), the
/// machine is busy. Four times the pieces must not cost sixteen times the time.
#[test]
fn a_long_helical_tangent_chain_is_found_quickly() {
    /// The room the noise of a run on a busy machine gets, on top of the ratio: the chain of 750
    /// pieces takes a fraction of a second, so this is the noise of one run and not a change of
    /// scale.
    const SLACK: std::time::Duration = std::time::Duration::from_millis(20);
    let pieces = 3_000usize;
    if let Err(missed) = scales_linearly(|| (helix_chain(pieces / 4), helix_chain(pieces)), SLACK) {
        panic!("{pieces} pieces took {missed}");
    }
}

#[test]
fn circular_rims_group_only_closed_cocircular_edges() {
    let arc = |start: f64, sweep: f64, yscale: f64| -> Vec<Vec3> {
        (0..=32)
            .map(|i| {
                let a = start + sweep * i as f64 / 32.0;
                Vec3::new(10.0 * a.cos(), yscale * 10.0 * a.sin(), 5.0)
            })
            .collect()
    };
    let pi = std::f64::consts::PI;
    let mut m = Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 5.0), Vec3::new(10.0, 10.0, 5.0)],
        edges: vec![arc(0.0, pi, 1.0), arc(2.0 * pi, -pi, 1.0)],
        ..Default::default()
    };
    assert_eq!(m.circular_rim(0), vec![0, 1]);
    assert_eq!(m.circular_rim(1), vec![0, 1]);
    // A hidden seam cannot form a selectable rim.
    m.seams = vec![false, true];
    assert_eq!(m.circular_rim(0), vec![0]);
    m.seams.clear();
    m.edges[1] = arc(pi, pi / 2.0, 1.0);
    assert_eq!(m.circular_rim(0), vec![0]);
    // A closed, smooth ellipse is not a circle.
    m.edges = vec![arc(0.0, pi, 0.7), arc(pi, pi, 0.7)];
    assert_eq!(m.circular_rim(0), vec![0]);
    assert_eq!(m.circular_rim(99), vec![99]);
}

#[test]
fn surface_patches_cross_only_hidden_seams() {
    let mut m = Mesh { edge_faces: vec![vec![0, 1], vec![1, 2], vec![2, 3]], seams: vec![true, true, false], ..Default::default() };
    assert_eq!(m.surface_patch(0), vec![0, 1, 2]);
    assert_eq!(m.surface_patch(2), vec![0, 1, 2]);
    assert_eq!(m.surface_patch(3), vec![3]);
    m.seams.clear();
    assert_eq!(m.surface_patch(0), vec![0]);
}

#[test]
fn profile_circle_recognition_tracks_direction_and_one_turn() {
    use std::f64::consts::TAU;
    for (center, radius) in [(Vec2::ZERO, 2.0), (Vec2::ZERO, 1e12), (Vec2::new(1e12, -1e12), 1e9)] {
        for count in [1, 2, 3, 4] {
            for start in [0.0, 0.1, 0.3, 2.7, 100.0, -100.0] {
                let sweep = TAU / count as f64;
                let circle = Loop2 { segs: (0..count).map(|i| Seg2::Arc { center, radius, start: start + i as f64 * sweep, sweep }).collect() };
                for mut path in [circle.clone(), circle.reversed()] {
                    for _ in 0..count {
                        assert_eq!(path.as_circle(), Some((center, radius)), "count={count}, start={start}, {path:?}");
                        path.segs.rotate_left(1);
                    }
                }
            }
        }
    }
    let circle = Loop2::circle_from(Vec2::new(1e12, -1e12), 1e9, 0.3);
    assert_eq!(circle.as_circle(), Some((Vec2::new(1e12, -1e12), 1e9)));
    assert_eq!(circle.reversed().as_circle(), circle.as_circle());
    // Each piece may store an equivalent phase from a different turn.
    let mut circle = Loop2::circle(Vec2::ZERO, 2.0);
    if let Seg2::Arc { start, .. } = &mut circle.segs[1] {
        *start = 3.0 * std::f64::consts::PI;
    }
    for mut path in [circle.clone(), circle.reversed()] {
        for _ in 0..2 {
            assert_eq!(path.as_circle(), Some((Vec2::ZERO, 2.0)));
            path.segs.rotate_left(1);
        }
    }
}

#[test]
fn profile_circle_recognition_rejects_gaps_retracing_and_invalid_arcs() {
    use std::f64::consts::{PI, TAU};
    let arc = |start, sweep| Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start, sweep };
    for segs in [
        vec![],
        vec![arc(0.0, PI)],
        vec![arc(0.0, PI), arc(0.0, PI)],
        vec![arc(0.0, PI), arc(PI + 0.01, PI)],
        vec![arc(0.0, PI), arc(PI + 1e-8, PI)],
        vec![arc(0.0, PI), arc(PI, -PI)],
        vec![arc(0.0, PI), arc(PI, PI), arc(TAU, PI), arc(TAU + PI, PI)],
        vec![arc(0.0, TAU), arc(0.0, TAU)],
        vec![arc(0.0, PI), Seg2::Line { a: Vec2::new(-2.0, 0.0), b: Vec2::new(2.0, 0.0) }],
    ] {
        let path = Loop2 { segs };
        assert_eq!(path.as_circle(), None, "{path:?}");
        assert_eq!(path.reversed().as_circle(), None, "{path:?}");
    }
    for sweep in [0.0, TAU + 1e-13, -TAU - 1e-13, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(Loop2 { segs: vec![arc(0.0, sweep)] }.as_circle(), None);
    }
    for start in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e20, f64::MAX] {
        assert_eq!(Loop2::circle_from(Vec2::ZERO, 2.0, start).as_circle(), None);
    }
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(Loop2::circle(Vec2::ZERO, radius).as_circle(), None);
    }
    assert_eq!(Loop2::circle(Vec2::new(f64::NAN, 0.0), 2.0).as_circle(), None);
    let mut path = Loop2::circle(Vec2::ZERO, 2.0);
    path.segs[1] = Seg2::Arc { center: Vec2::ZERO, radius: 2.0_f64.next_up(), start: PI, sweep: PI };
    assert_eq!(path.as_circle(), None, "support equality is exact");
    path.segs[1] = Seg2::Arc { center: Vec2::new(1e-15, 0.0), radius: 2.0, start: PI, sweep: PI };
    assert_eq!(path.as_circle(), None);
    // The stored phase resolves an extra 0.125 radians; its rounding cell must not
    // close the resulting gap/overlap by independently widening both arc endpoints.
    let start = 2.0_f64.powi(49);
    let circle = Loop2 { segs: vec![arc(start, PI), arc((start + PI) + 0.125, PI)] };
    for mut path in [circle.clone(), circle.reversed()] {
        for _ in 0..2 {
            assert_eq!(path.as_circle(), None, "{path:?}");
            path.segs.rotate_left(1);
        }
    }
}

fn ellipse_quarters(center: Vec2, u: Vec2, v: Vec2) -> Loop2 {
    let axes = [u, v, -u, -v];
    Loop2 {
        segs: (0..4)
            .map(|i| Seg2::Conic {
                a: center + axes[i],
                apex: (center + axes[i]) + axes[(i + 1) % 4],
                b: center + axes[(i + 1) % 4],
                w: std::f64::consts::FRAC_1_SQRT_2,
            })
            .collect(),
    }
}

#[test]
fn profile_ellipse_recognition_preserves_quarters_and_orientation() {
    for (center, u, v) in
        [(Vec2::new(3.0, -2.0), Vec2::new(5.0, 0.0), Vec2::new(0.0, 2.0)), (Vec2::new(17.0, -11.0), Vec2::new(12.0, 9.0), Vec2::new(-4.8, 6.4))]
    {
        let ellipse = ellipse_quarters(center, u, v);
        for mut path in [ellipse.clone(), ellipse.reversed()] {
            for _ in 0..4 {
                let (c, major, ratio) = path.as_ellipse().expect("four original ellipse quarters");
                assert_eq!(c, center);
                assert!((major.len() - u.len()).abs() < 1e-12, "{major:?}");
                assert!((ratio - v.len() / u.len()).abs() < 1e-12, "{ratio}");
                for seg in &path.segs {
                    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                        let d = seg.point_at(t) - c;
                        let x = d.dot(major) / major.len2();
                        let y = d.dot(major.perp()) / (major.len2() * ratio);
                        assert!((x * x + y * y - 1.0).abs() < 1e-12);
                    }
                }
                path.segs.rotate_left(1);
            }
        }
    }
}

#[test]
fn profile_ellipse_recognition_does_not_replace_other_paths() {
    let ellipse = ellipse_quarters(Vec2::new(3.0, -2.0), Vec2::new(5.0, 0.0), Vec2::new(0.0, 2.0));
    let q = ellipse.segs[0];
    assert_eq!(Loop2 { segs: vec![q, q.reversed(), q, q.reversed()] }.as_ellipse(), None);
    assert_eq!(Loop2 { segs: vec![q; 4] }.as_ellipse(), None);
    assert_eq!(Loop2 { segs: ellipse.segs.repeat(2) }.as_ellipse(), None);
    assert_eq!(Loop2 { segs: ellipse.segs[..3].to_vec() }.as_ellipse(), None);
    assert_eq!(Loop2::circle(Vec2::ZERO, 2.0).as_ellipse(), None);
    assert_eq!(Loop2::default().as_ellipse(), None);
    for delta in [Vec2::new(0.001, 0.0), Vec2::new(f64::NAN, 0.0)] {
        let mut path = ellipse.clone();
        if let Seg2::Conic { a, .. } = &mut path.segs[1] {
            *a += delta;
        }
        assert_eq!(path.as_ellipse(), None);
    }
    for weight in [0.5, 1.0, 1.1, f64::NAN, f64::INFINITY] {
        let mut path = ellipse.clone();
        if let Seg2::Conic { w, .. } = &mut path.segs[1] {
            *w = weight;
        }
        assert_eq!(path.as_ellipse(), None);
    }
    // The world-coordinate midpoint rounds to c, but the x displacements are +4/-3
    // or +4/-3.5. Even when one offset reconstructs both rounded endpoints, the local
    // asymmetry is significant and must not be replaced by a full ellipse.
    let c = 2.0_f64.powi(52);
    for left in [3.0, 3.5] {
        let points = [Vec2::new(c + 4.0, 0.0), Vec2::new(c, 2.0), Vec2::new(c - left, 0.0), Vec2::new(c, -2.0)];
        let apices = [Vec2::new(c + 4.0, 2.0), Vec2::new(c - left, 2.0), Vec2::new(c - left, -2.0), Vec2::new(c + 4.0, -2.0)];
        let ellipse = Loop2 {
            segs: (0..4).map(|i| Seg2::Conic { a: points[i], apex: apices[i], b: points[(i + 1) % 4], w: std::f64::consts::FRAC_1_SQRT_2 }).collect(),
        };
        for mut path in [ellipse.clone(), ellipse.reversed()] {
            for _ in 0..4 {
                assert_eq!(path.as_ellipse(), None, "{path:?}");
                path.segs.rotate_left(1);
            }
        }
    }
}

#[test]
fn profile_conic_ellipse_keeps_directed_endpoints_and_unclamped_ratio() {
    let ellipse = ellipse_quarters(Vec2::new(3.0, -2.0), Vec2::new(5.0, 0.0), Vec2::new(0.0, 2.0));
    for seg in &ellipse.segs {
        for (piece, positive) in [(*seg, true), (seg.reversed(), false)] {
            let Seg2::Conic { a, apex, b, w } = piece else { panic!("fixture must be a conic") };
            let arc = conic_ellipse(a, apex, b, w).expect("elliptical quarter");
            assert_eq!(arc.end > arc.start, positive);
            for (t, expected) in [(arc.start, a), ((arc.start + arc.end) * 0.5, piece.point_at(0.5)), (arc.end, b)] {
                let actual = arc.center + arc.major * t.cos() + arc.major.perp() * (arc.ratio * t.sin());
                assert!(actual.dist(expected) < 1e-12, "{piece:?}: {actual:?} != {expected:?}");
            }
        }
    }
    let thin = ellipse_quarters(Vec2::ZERO, Vec2::new(1e12, 0.0), Vec2::new(0.0, 0.5));
    let (_, _, ratio) = thin.as_ellipse().expect("thin ellipse from its original axes");
    assert!((ratio - 5e-13).abs() < 1e-25);
    for seg in thin.segs {
        let Seg2::Conic { a, apex, b, w } = seg else { panic!("fixture must be a conic") };
        if let Some(arc) = conic_ellipse(a, apex, b, w) {
            assert!(arc.ratio < 1e-12, "a small ratio must not be clamped");
        }
    }
    for w in [0.2, 0.5, 0.8] {
        assert!(conic_ellipse(Vec2::X, Vec2::new(1.0, 1.0), Vec2::Y, w).is_some());
    }
    for w in [-1.0, 0.0, 1.0 - 1e-7, 1.0, 1.1, f64::NAN, f64::INFINITY] {
        assert!(conic_ellipse(Vec2::X, Vec2::new(1.0, 1.0), Vec2::Y, w).is_none());
    }
    assert!(conic_ellipse(Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0), 0.5).is_none());
    assert!(conic_ellipse(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0.5).is_none());
    assert!(conic_ellipse(Vec2::new(f64::INFINITY, 0.0), Vec2::X, Vec2::Y, 0.5).is_none());
}

#[test]
fn profile_conic_ellipse_recovers_nonquarter_analytic_parameters() {
    // For A=(0,-2), P=(4,0), B=(0,2), eliminating the rational parameter gives
    // (1-w²)x²/(16w²) + x/2 + y²/4 = 1. Completing its square gives these independent
    // centers and principal axes. At w=1/5 the longer axis is Y, not X.
    let cases = [
        (0.2, Vec2::new(-1.0 / 6.0, 0.0), Vec2::new(0.0, 5.0 / 6.0_f64.sqrt()), 1.0 / 6.0_f64.sqrt(), 2.0 / 3.0),
        (0.5, Vec2::new(-4.0 / 3.0, 0.0), Vec2::new(8.0 / 3.0, 0.0), 3.0_f64.sqrt() / 2.0, 4.0 / 3.0),
        (0.6, Vec2::new(-9.0 / 4.0, 0.0), Vec2::new(15.0 / 4.0, 0.0), 2.0 / 3.0, 1.5),
        (0.8, Vec2::new(-64.0 / 9.0, 0.0), Vec2::new(80.0 / 9.0, 0.0), 3.0 / 8.0, 16.0 / 9.0),
    ];
    let (a, apex, b) = (Vec2::new(0.0, -2.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 2.0));
    for (w, center, major, ratio, middle_x) in cases {
        for (start, end, direction) in [(a, b, 1.0), (b, a, -1.0)] {
            let arc = conic_ellipse(start, apex, end, w).expect("non-quarter elliptic conic");
            assert_profile_ellipse_arc(arc, center, major, ratio, direction * 2.0 * w.acos(), [start, Vec2::new(middle_x, 0.0), end]);
        }
    }
}

fn assert_profile_ellipse_arc(arc: EllipseArc, center: Vec2, major: Vec2, ratio: f64, sweep: f64, points: [Vec2; 3]) {
    assert!(arc.center.dist(center) < 1e-12, "center: {arc:?}, expected {center:?}");
    // Changing the major-axis sign and shifting both parameters describes the same arc.
    let aligned_major = if arc.major.dot(major) < 0.0 { -arc.major } else { arc.major };
    assert!(aligned_major.dist(major) < 1e-12, "major: {arc:?}, expected ±{major:?}");
    assert!((arc.ratio - ratio).abs() < 1e-12, "ratio: {arc:?}, expected {ratio}");
    assert!((arc.end - arc.start - sweep).abs() < 1e-12, "sweep: {arc:?}, expected {sweep}");
    // Only the symmetric rational midpoint corresponds to the angular midpoint. The
    // rational parameter is not generally a linear fraction of this angular interval.
    for (t, expected) in [arc.start, (arc.start + arc.end) * 0.5, arc.end].into_iter().zip(points) {
        let actual = arc.center + arc.major * t.cos() + arc.major.perp() * (arc.ratio * t.sin());
        assert!(actual.dist(expected) < 1e-12, "point: {actual:?}, expected {expected:?}, {arc:?}");
    }
}

#[test]
fn profile_conic_ellipse_preserves_transformed_partial_arcs() {
    // Apply T(x,y)=(7-2y,-5+2x) to the w=3/5 conic above: rotate 90°, scale 2,
    // translate. Its independent center is (7,-19/2), semiaxes 15/2 and 5.
    // Reflecting Y then reverses the orientation while keeping both axis lengths.
    for reflection in [1.0, -1.0] {
        let (a, apex, b) = (Vec2::new(11.0, -5.0 * reflection), Vec2::new(7.0, 3.0 * reflection), Vec2::new(3.0, -5.0 * reflection));
        for (start, end, direction) in [(a, b, reflection), (b, a, -reflection)] {
            let arc = conic_ellipse(start, apex, end, 0.6).expect("transformed non-quarter elliptic conic");
            assert_profile_ellipse_arc(
                arc,
                Vec2::new(7.0, -9.5 * reflection),
                Vec2::new(0.0, 7.5),
                2.0 / 3.0,
                direction * 2.0 * 0.6_f64.acos(),
                [start, Vec2::new(7.0, -2.0 * reflection), end],
            );
        }
    }
}

#[test]
fn profile_conic_ellipse_declines_nonelliptic_weights_and_conservative_boundaries() {
    let (a, apex, b) = (Vec2::new(0.0, -2.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 2.0));
    for w in [
        -f64::MAX,
        -1.0,
        -0.0,
        0.0,
        1e-9_f64.next_down(),
        1e-9,
        1.0 - 1e-6,
        1.0 - 1e-7,
        1.0_f64.next_down(),
        1.0,
        1.0_f64.next_up(),
        1.5,
        f64::MAX,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert!(conic_ellipse(a, apex, b, w).is_none(), "w={w}");
        assert!(conic_ellipse(b, apex, a, w).is_none(), "reversed w={w}");
    }
    // Small positive weights and weights just below 1 still describe ellipses, but the
    // inherited extractor guards conservatively retain their original rational curves.
    // At w=1 this noncollinear conic is parabolic; above 1 it is hyperbolic.
}

#[test]
fn profile_conic_ellipse_declines_nonfinite_controls_in_every_position() {
    let original = [Vec2::new(0.0, -2.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 2.0)];
    for position in 0..3 {
        for coordinate in 0..2 {
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut controls = original;
                if coordinate == 0 {
                    controls[position].x = invalid
                } else {
                    controls[position].y = invalid
                }
                let [a, apex, b] = controls;
                assert!(conic_ellipse(a, apex, b, 0.6).is_none(), "control={position}, coordinate={coordinate}, value={invalid}");
            }
        }
    }
}

#[test]
fn profile_conic_ellipse_declines_degeneracy_and_unrepresentable_extraction() {
    for [a, apex, b] in [
        [Vec2::new(3.0, -2.0); 3],
        [Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)],
        [Vec2::new(-3.0, -5.0), Vec2::new(1.0, 3.0), Vec2::new(5.0, 11.0)],
        [Vec2::new(0.0, -2.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, -2.0)],
        [Vec2::new(0.0, -2.0), Vec2::new(0.0, -2.0), Vec2::new(0.0, 2.0)],
        [Vec2::new(0.0, -2.0), Vec2::new(0.0, 2.0), Vec2::new(0.0, 2.0)],
        // Finite inputs can still overflow the derived dot products or chord midpoint.
        [Vec2::new(f64::MAX, 0.0), Vec2::new(f64::MAX, f64::MAX), Vec2::new(0.0, f64::MAX)],
        [Vec2::new(f64::MAX, 0.0), Vec2::new(f64::MAX, 1.0), Vec2::new(f64::MAX, 2.0)],
    ] {
        assert!(conic_ellipse(a, apex, b, 0.6).is_none(), "{a:?}, {apex:?}, {b:?}");
        assert!(conic_ellipse(b, apex, a, 0.6).is_none());
    }
    // A real but very small ellipse is also outside the inherited extraction guard;
    // this is a conservative recognition miss, not a claim of geometric degeneracy.
    assert!(conic_ellipse(Vec2::new(0.0, -2e-10), Vec2::new(4e-10, 0.0), Vec2::new(0.0, 2e-10), 0.6).is_none());
}

fn ellipse_unequal_pieces(three: bool) -> Loop2 {
    // Original integer controls on x²/225+y²/400=1. The three-piece neighboring
    // unit-ellipse dot products are -3/5, -7/25, -3/5; their weights are
    // sqrt((1+h)/2) = 1/sqrt(5), 3/5, 1/sqrt(5), rounded to nearest binary64.
    let pieces = if three {
        vec![
            (Vec2::new(15.0, 0.0), Vec2::new(15.0, 40.0), Vec2::new(-9.0, 16.0), 0.4472135954999579),
            (Vec2::new(-9.0, 16.0), Vec2::new(-25.0, 0.0), Vec2::new(-9.0, -16.0), 0.6),
            (Vec2::new(-9.0, -16.0), Vec2::new(15.0, -40.0), Vec2::new(15.0, 0.0), 0.4472135954999579),
        ]
    } else {
        vec![
            (Vec2::new(9.0, -16.0), Vec2::new(25.0, 0.0), Vec2::new(9.0, 16.0), 0.6),
            (Vec2::new(9.0, 16.0), Vec2::new(0.0, 25.0), Vec2::new(-9.0, 16.0), 0.8),
            (Vec2::new(-9.0, 16.0), Vec2::new(-25.0, 0.0), Vec2::new(-9.0, -16.0), 0.6),
            (Vec2::new(-9.0, -16.0), Vec2::new(0.0, -25.0), Vec2::new(9.0, -16.0), 0.8),
        ]
    };
    Loop2 { segs: pieces.into_iter().map(|(a, apex, b, w)| Seg2::Conic { a, apex, b, w }).collect() }
}

fn transform_ellipse_pieces(path: &Loop2, center: Vec2, scale: f64, rotated: bool, reflected: bool) -> Loop2 {
    let transform = |p: Vec2| {
        let p = if rotated { Vec2::new(-p.y, p.x) } else { p };
        let p = if reflected { Vec2::new(p.x, -p.y) } else { p };
        center + p * scale
    };
    Loop2 {
        segs: path
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: transform(a), apex: transform(apex), b: transform(b), w },
                _ => panic!("analytic fixture contains only conics"),
            })
            .collect(),
    }
}

#[test]
fn profile_ellipse_recognizes_unequal_integer_partitions_through_the_real_candidate() {
    let far = 2.0_f64.powi(52);
    for three in [false, true] {
        let original = ellipse_unequal_pieces(three);
        for (center, scale) in [(Vec2::ZERO, 1.0), (Vec2::new(7.0, -5.0), 2.0), (Vec2::new(far, far), 1.0)] {
            for rotated in [false, true] {
                for reflected in [false, true] {
                    let transformed = transform_ellipse_pieces(&original, center, scale, rotated, reflected);
                    let major = if rotated { Vec2::new(20.0 * scale, 0.0) } else { Vec2::new(0.0, 20.0 * scale) };
                    for mut path in [transformed.clone(), transformed.reversed()] {
                        for _ in 0..path.segs.len() {
                            assert_eq!(path.as_ellipse(), Some((center, major, 0.75)), "three={three}, {path:?}");
                            path.segs.rotate_left(1);
                        }
                    }
                }
            }
        }
        // T(x,y)=(3x-4y,4x+3y) preserves integer controls. Its support is
        // [[8425,-2100],[-2100,7200]], with eigenvalues 10000 and 5625.
        // This exercises both the non-diagonal candidate and the support cross term.
        let transform = |p: Vec2| Vec2::new(3.0 * p.x - 4.0 * p.y, 4.0 * p.x + 3.0 * p.y);
        let inclined = Loop2 {
            segs: original
                .segs
                .iter()
                .map(|seg| match *seg {
                    Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: transform(a), apex: transform(apex), b: transform(b), w },
                    _ => panic!("analytic fixture contains only conics"),
                })
                .collect(),
        };
        assert!(inclined.is_closed(0.0));
        for mut path in [inclined.clone(), inclined.reversed()] {
            for _ in 0..path.segs.len() {
                assert_eq!(path.as_ellipse(), Some((Vec2::ZERO, Vec2::new(80.0, -60.0), 0.75)), "three={three}, {path:?}");
                path.segs.rotate_left(1);
            }
        }
    }
}

#[test]
fn profile_ellipse_rejects_distinct_supports_even_with_shared_endpoints() {
    for three in [false, true] {
        let path = ellipse_unequal_pieces(three);
        let mut different_apex = path.clone();
        if let Seg2::Conic { apex, .. } = &mut different_apex.segs[0] {
            apex.x += 1e-8;
        }
        let mut different_weight = path.clone();
        if let Seg2::Conic { w, .. } = &mut different_weight.segs[0] {
            *w += 1e-8;
        }
        let mut different_vertex = path.clone();
        if let Seg2::Conic { b, .. } = &mut different_vertex.segs[0] {
            b.x += 1e-8;
        }
        if let Seg2::Conic { a, .. } = &mut different_vertex.segs[1] {
            a.x += 1e-8;
        }
        let mut different_half = path.clone();
        if let Seg2::Conic { apex, .. } = &mut different_half.segs[1] {
            apex.y += 0.25;
        }
        for changed in [different_apex, different_weight, different_vertex, different_half] {
            assert!(changed.is_closed(0.0));
            for mut ordered in [changed.clone(), changed.reversed()] {
                for _ in 0..ordered.segs.len() {
                    assert_eq!(ordered.as_ellipse(), None, "{ordered:?}");
                    ordered.segs.rotate_left(1);
                }
            }
        }
    }
}

#[test]
fn profile_ellipse_rejects_gaps_retracing_and_repeated_unequal_turns() {
    let ellipse = ellipse_unequal_pieces(false);
    let q = ellipse.segs[0];
    let mut one_step_gap = ellipse.clone();
    if let Seg2::Conic { a, .. } = &mut one_step_gap.segs[1] {
        a.x = a.x.next_up();
    }
    let mut reordered = ellipse.clone();
    reordered.segs.swap(1, 2);
    for path in [
        one_step_gap,
        reordered,
        Loop2 { segs: vec![q, q.reversed(), q, q.reversed()] },
        Loop2 { segs: vec![q; 4] },
        Loop2 { segs: ellipse.segs.repeat(2) },
        Loop2 { segs: ellipse.segs[..3].to_vec() },
        Loop2 { segs: vec![q, Seg2::Line { a: Vec2::X, b: Vec2::ZERO }] },
    ] {
        assert_eq!(path.as_ellipse(), None, "{path:?}");
        assert_eq!(path.reversed().as_ellipse(), None, "{path:?}");
    }
}

#[test]
fn profile_ellipse_uses_local_cells_at_large_world_coordinates() {
    let c = 2.0_f64.powi(52);
    let original = transform_ellipse_pieces(&ellipse_unequal_pieces(false), Vec2::new(c, c), 1.0, false, false);
    assert!(original.as_ellipse().is_some(), "large coordinates alone are not a refusal");
    let mut changed = original.clone();
    if let Seg2::Conic { b, .. } = &mut changed.segs[1] {
        b.x += 0.5;
    }
    if let Seg2::Conic { a, .. } = &mut changed.segs[2] {
        a.x += 0.5;
    }
    assert!(changed.is_closed(0.0));
    assert_eq!(changed.as_ellipse(), None, "a resolved local displacement cannot fit a world-coordinate allowance");
    // The canonical left chord still has length32. Changing its apex from -25 to -24
    // proposes a center with a fractional offset that cannot survive addition to c.
    let mut center_loss = original.clone();
    if let Seg2::Conic { apex, .. } = &mut center_loss.segs[2] {
        apex.x += 1.0;
    }
    assert_eq!(center_loss.as_ellipse(), None, "final-center loss must fit the local cell");
    let mut outlier = original;
    if let Seg2::Conic { apex, .. } = &mut outlier.segs[0] {
        apex.x = 1e100;
    }
    assert_eq!(outlier.as_ellipse(), None, "an apex cannot enlarge the vertex budget");
}

#[test]
fn profile_ellipse_declines_hostile_general_partitions() {
    let original = ellipse_unequal_pieces(false);
    for weight in [0.0, -1.0, 1e-9, 1.0 - 1e-6, 1.0, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut path = original.clone();
        if let Seg2::Conic { w, .. } = &mut path.segs[0] {
            *w = weight;
        }
        assert_eq!(path.as_ellipse(), None, "w={weight}");
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for control in 0..3 {
            for coordinate in 0..2 {
                let mut path = original.clone();
                if let Seg2::Conic { a, apex, b, .. } = &mut path.segs[0] {
                    let point = match control {
                        0 => a,
                        1 => apex,
                        _ => b,
                    };
                    if coordinate == 0 { point.x = value } else { point.y = value }
                }
                assert_eq!(path.as_ellipse(), None);
            }
        }
    }
    for scale in [0.0, 1e-200, 1e200] {
        assert_eq!(transform_ellipse_pieces(&original, Vec2::ZERO, scale, false, false).as_ellipse(), None);
    }
    let mut collinear = original.clone();
    for seg in &mut collinear.segs {
        if let Seg2::Conic { a, apex, b, .. } = seg {
            a.y = 0.0;
            apex.y = 0.0;
            b.y = 0.0;
        }
    }
    assert_eq!(collinear.as_ellipse(), None);
    // Keep exact joins and nonzero chords. The left chord of length 32 supplies
    // the known support; the first piece has antipodal endpoints on that support.
    let antipodal = Loop2 {
        segs: vec![
            Seg2::Conic { a: Vec2::new(15.0, 0.0), apex: Vec2::new(0.0, 20.0), b: Vec2::new(-15.0, 0.0), w: 0.6 },
            Seg2::Conic { a: Vec2::new(-15.0, 0.0), apex: Vec2::new(-15.0, -10.0), b: Vec2::new(-9.0, -16.0), w: 0.8944271909999159 },
            Seg2::Conic { a: Vec2::new(-9.0, -16.0), apex: Vec2::new(-25.0, 0.0), b: Vec2::new(-9.0, 16.0), w: 0.6 },
            Seg2::Conic { a: Vec2::new(-9.0, 16.0), apex: Vec2::new(15.0, 40.0), b: Vec2::new(15.0, 0.0), w: 0.4472135954999579 },
        ],
    };
    assert!(antipodal.is_closed(0.0));
    assert!(antipodal.segs.iter().all(|seg| matches!(*seg, Seg2::Conic { a, b, .. } if a != b)));
    assert_eq!(antipodal.as_ellipse(), None);
}

#[test]
fn profile_ellipse_arithmetic_retains_exact_dyadic_residuals() {
    use crate::profile::EllipseScalar as Number;
    let number = |x| Number::exact(x).expect("finite dyadic fixture");
    let large = 2.0_f64.powi(53);
    let sum = number(large).plus(number(1.0)).expect("finite sum");
    assert_eq!((sum.hi, sum.lo, sum.radius), (large, 1.0, 0.0));
    let cancellation = sum.minus(number(large)).expect("exact cancellation");
    assert_eq!((cancellation.hi, cancellation.lo, cancellation.radius), (1.0, 0.0, 0.0));
    let e = 2.0_f64.powi(-27);
    let product = number(1.0 + e).times(number(1.0 - e)).expect("finite product");
    // (1+2^-27)(1-2^-27) = 1-2^-54; the low term cannot be discarded.
    assert_eq!((product.hi, product.lo), (1.0, -e * e));
    assert!(product.radius < 2.0_f64.powi(-100));
    let third = 2.0_f64.powi(-108);
    let three_terms = number(1.0).plus(number(2.0_f64.powi(-54))).expect("pair").plus(number(third)).expect("three terms");
    assert!(three_terms.radius >= third, "a discarded third component remains in the radius");
}

#[test]
fn profile_ellipse_arithmetic_certifies_quotients_and_roots() {
    use crate::profile::EllipseScalar as Number;
    let number = |x| Number::exact(x).expect("finite fixture");
    let dyadic = number(1.0).plus(number(2.0_f64.powi(-53))).expect("exact two-term numerator");
    let quotient = dyadic.divided_by(number(2.0)).expect("dyadic quotient");
    assert_eq!((quotient.hi, quotient.lo), (0.5, 2.0_f64.powi(-54)));
    assert!(quotient.radius < 2.0_f64.powi(-100));
    let third = number(1.0).divided_by(number(3.0)).expect("one third");
    let correction = third.minus(number(1.0 / 3.0)).expect("retain correction");
    // The exact correction is 1/(3*2^54), strictly between these independent dyadics.
    assert_eq!(correction.minus(number(2.0_f64.powi(-56))).expect("lower comparison").sign(), Some(1));
    assert_eq!(number(2.0_f64.powi(-55)).minus(correction).expect("upper comparison").sign(), Some(1));
    assert!(third.radius < 2.0_f64.powi(-100));
    let e = 2.0_f64.powi(-27);
    let square = number(1.0 + 2.0 * e).plus(number(e * e)).expect("(1+e)^2 exactly");
    let root = square.square_root().expect("positive exact-dyadic square");
    assert!(root.minus(number(1.0 + e)).expect("compare analytic root").fits(2.0_f64.powi(-100)));
}

#[test]
fn profile_ellipse_arithmetic_refuses_uncertainty_and_preserves_subnormal_bounds() {
    use crate::profile::EllipseScalar as Number;
    let number = |x| Number::exact(x).expect("finite fixture");
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Number::exact(invalid).is_none());
    }
    assert!(number(f64::MAX).plus(number(f64::MAX)).is_none());
    assert!(number(f64::MAX).times(number(2.0)).is_none());
    assert!(number(1.0).divided_by(number(0.0)).is_none());
    assert!(number(-1.0).square_root().is_none());
    assert!(number(0.0).square_root().is_none());
    let minimum = f64::from_bits(1);
    let underflow = number(minimum).times(number(0.5)).expect("half a minimum subnormal is enclosed");
    assert_eq!((underflow.hi, underflow.lo), (0.0, 0.0));
    assert!(underflow.radius >= minimum);
    assert!(!underflow.fits(0.0));
    assert_eq!(underflow.sign(), None);
    let radius = 2.0_f64.powi(-53);
    assert!(number(radius).fits(radius));
    assert!(!number(radius.next_up()).fits(radius));
    let uncertain = Number { hi: 0.0, lo: 0.0, radius: 2.0 * radius };
    assert!(!uncertain.fits(radius), "intersection with the cell does not prove inclusion");
    assert!(number(1.0).divided_by(uncertain).is_none());
}

#[test]
fn profile_ellipse_weight_cells_are_asymmetric_and_local_extent_does_not_round_up() {
    use crate::profile::{EllipseScalar as Number, ellipse_local_resolution};
    let number = |x| Number::exact(x).expect("finite fixture");
    let center = number(0.5);
    let lower_midpoint = center.minus(number(2.0_f64.powi(-55))).expect("lower midpoint");
    let upper_midpoint = center.plus(number(2.0_f64.powi(-54))).expect("upper midpoint");
    assert_eq!(lower_midpoint.rounds_to(0.5), Some(false));
    assert_eq!(upper_midpoint.rounds_to(0.5), Some(false));
    assert_eq!(center.minus(number(2.0_f64.powi(-56))).expect("inside lower cell").rounds_to(0.5), Some(true));
    assert_eq!(center.plus(number(2.0_f64.powi(-55))).expect("inside upper cell").rounds_to(0.5), Some(true));
    assert_eq!(center.minus(number(3.0 * 2.0_f64.powi(-56))).expect("outside lower cell").rounds_to(0.5), Some(false));
    let local = 2.0_f64.powi(-52);
    let c = 2.0_f64.powi(52);
    assert_eq!(ellipse_local_resolution(-4.0, 4.0), Some(local));
    assert_eq!(ellipse_local_resolution(c - 4.0, c + 4.0), Some(local));
    assert_eq!(ellipse_local_resolution(-4.0, 4.0_f64.next_down()), Some(local), "rounded extent must not select a larger binade");
    assert!(ellipse_local_resolution(0.0, f64::from_bits(1)).is_none());
    assert!(ellipse_local_resolution(-f64::MAX, f64::MAX).is_none());
    assert!(ellipse_local_resolution(1.0, 1.0).is_none());
}

#[test]
fn cubic_and_conic_segments_measure_exactly() {
    use crate::{Loop2, Seg2, Vec2};
    // An ellipse (a = 5, b = 3) as four quarter conics: area π a b.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let (a, b) = (5.0, 3.0);
    let q = |p0: Vec2, c: Vec2, p1: Vec2| Seg2::Conic { a: p0, apex: c, b: p1, w };
    let e = Loop2 {
        segs: vec![
            q(Vec2::new(a, 0.0), Vec2::new(a, b), Vec2::new(0.0, b)),
            q(Vec2::new(0.0, b), Vec2::new(-a, b), Vec2::new(-a, 0.0)),
            q(Vec2::new(-a, 0.0), Vec2::new(-a, -b), Vec2::new(0.0, -b)),
            q(Vec2::new(0.0, -b), Vec2::new(a, -b), Vec2::new(a, 0.0)),
        ],
    };
    assert!((e.signed_area() - std::f64::consts::PI * a * b).abs() < 1e-9, "{}", e.signed_area());
    for s in &e.segs {
        let m = s.point_at(0.37);
        assert!(((m.x / a).powi(2) + (m.y / b).powi(2) - 1.0).abs() < 1e-12);
    }
    // A cubic on a straight line has the line's length; reversing flips the area sign.
    let c = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 0.0), p2: Vec2::new(2.0, 0.0), p3: Vec2::new(3.0, 0.0) };
    assert!((c.length() - 3.0).abs() < 1e-12);
    // Square with one bulging cubic side: area = 1 + the cubic's area over the chord.
    let bulge = Seg2::Cubic { p0: Vec2::new(1.0, 0.0), p1: Vec2::new(1.5, 1.0 / 3.0), p2: Vec2::new(1.5, 2.0 / 3.0), p3: Vec2::new(1.0, 1.0) };
    let sq = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(1.0, 0.0) },
            bulge,
            Seg2::Line { a: Vec2::new(1.0, 1.0), b: Vec2::new(0.0, 1.0) },
            Seg2::Line { a: Vec2::new(0.0, 1.0), b: Vec2::new(0.0, 0.0) },
        ],
    };
    // x(t) - 1 = 1.5 t(1-t) (Bernstein: 3·0.5·t(1-t)²+3·0.5·t²(1-t)), y = t: ∫ 1.5 t(1-t) dt = 0.25.
    assert!((sq.signed_area() - 1.25).abs() < 1e-12, "{}", sq.signed_area());
    let rev = bulge.reversed();
    assert!((rev.area_term() + bulge.area_term()).abs() < 1e-12);
    assert!(bulge.polyline(1e-4).len() > 4);
    // Halves cover the same curve.
    let (l, r) = bulge.split_half();
    assert!(l.point_at(0.5).dist(bulge.point_at(0.25)) < 1e-12);
    assert!(r.point_at(0.5).dist(bulge.point_at(0.75)) < 1e-12);
    // Conic halves are re-parameterised but stay on the ellipse.
    let (l2, r2) = e.segs[0].split_half();
    for t in [0.1, 0.5, 0.9] {
        for q in [l2.point_at(t), r2.point_at(t)] {
            assert!(((q.x / a).powi(2) + (q.y / b).powi(2) - 1.0).abs() < 1e-12, "{q:?}");
        }
    }
    for (seg, (l, r)) in [(bulge, (l, r)), (e.segs[0], (l2, r2))] {
        assert!(l.end().dist(r.start()) < 1e-12);
        assert!((l.area_term() + r.area_term() - seg.area_term()).abs() < 1e-12);
    }
}

#[test]
fn split_at_and_closest_param() {
    use crate::{Seg2, Vec2};
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = Seg2::Conic { a: Vec2::new(10.0, 0.0), apex: Vec2::new(10.0, 10.0), b: Vec2::new(0.0, 10.0), w };
    for t in [0.2, 0.5, 0.8] {
        let (l, r) = quarter.split_at(t);
        assert!(l.end().dist(quarter.point_at(t)) < 1e-12 && r.start().dist(l.end()) < 1e-12);
        for s in [0.3, 0.7] {
            assert!((l.point_at(s).len() - 10.0).abs() < 1e-12 && (r.point_at(s).len() - 10.0).abs() < 1e-12);
        }
        assert!((l.area_term() + r.area_term() - quarter.area_term()).abs() < 1e-12);
    }
    let c = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 2.0), p2: Vec2::new(3.0, 2.0), p3: Vec2::new(4.0, 0.0) };
    let q = c.point_at(0.37);
    assert!((c.closest_param(q + Vec2::new(0.0, 0.0)) - 0.37).abs() < 1e-9);
    let (l, r) = c.split_at(0.37);
    assert!(l.end().dist(q) < 1e-12 && r.start().dist(q) < 1e-12);
    assert!((quarter.closest_param(Vec2::new(20.0, 20.0)) - 0.5).abs() < 1e-9);
}

/// An axis-aligned box as a closed mesh, with `n` x `n` quads on each side.
fn box_mesh(min: Vec3, max: Vec3, n: usize) -> Mesh {
    let mut m = Mesh::default();
    let s = max - min;
    let at = |c: [f64; 3]| min + Vec3::new(s.x * c[0], s.y * c[1], s.z * c[2]);
    for axis in 0..3 {
        for side in [0.0, 1.0] {
            for i in 0..n {
                for j in 0..n {
                    let corner = |a: usize, b: usize| {
                        let mut c = [0.0; 3];
                        c[axis] = side;
                        c[(axis + 1) % 3] = a as f64 / n as f64;
                        c[(axis + 2) % 3] = b as f64 / n as f64;
                        at(c)
                    };
                    let k = m.positions.len() as u32;
                    m.positions.extend([corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1)]);
                    m.triangles.extend(if side > 0.0 { [[k, k + 1, k + 2], [k, k + 2, k + 3]] } else { [[k, k + 2, k + 1], [k, k + 3, k + 2]] });
                    // Faces: 2 * axis, plus one on the far side.
                    m.tri_face.extend([(2 * axis) as u32 + side as u32; 2]);
                }
            }
        }
    }
    m
}

#[test]
fn meshes_cross_only_where_their_surfaces_pass_through_each_other() {
    let cube = |x: f64, y: f64, n: usize| box_mesh(Vec3::new(x, y, 0.0), Vec3::new(x + 10.0, y + 10.0, 10.0), n);
    let a = cube(0.0, 0.0, 1);
    // A corner 0.15 deep in the other cube (0.02 % of its volume), and a deep overlap.
    for (x, y, n) in [(9.85, 3.0, 1), (9.85, 3.0, 7), (9.85, 9.85, 2), (5.0, 5.0, 3)] {
        assert!(a.crosses(&cube(x, y, n), 0.01), "{x} {y} {n}");
        assert!(cube(x, y, n).crosses(&a, 0.01), "{x} {y} {n}");
    }
    // Apart, touching on a face or an edge, and the same body.
    for (x, y, n) in [(10.5, 0.0, 1), (10.0, 0.0, 1), (10.0, 3.0, 5), (10.0, 10.0, 2), (0.0, 0.0, 1), (0.0, 0.0, 4)] {
        assert!(!a.crosses(&cube(x, y, n), 0.01), "{x} {y} {n}");
    }
    // One inside the other, standing on a face, and sharing a face plane from inside.
    let inner = box_mesh(Vec3::new(2.0, 2.0, 2.0), Vec3::new(8.0, 8.0, 8.0), 2);
    let on_top = box_mesh(Vec3::new(2.0, 2.0, 10.0), Vec3::new(8.0, 8.0, 14.0), 2);
    let flush = box_mesh(Vec3::new(2.0, 2.0, 4.0), Vec3::new(8.0, 8.0, 10.0), 2);
    assert!(!a.crosses(&inner, 0.01) && !inner.crosses(&a, 0.01));
    assert!(!a.crosses(&on_top, 0.01) && !a.crosses(&flush, 0.01));
    // A slab overlap between faces that are parallel or in one plane has no crossing.
    assert!(!a.crosses(&cube(9.85, 0.0, 1), 0.01));
    // Passing through by less than the tolerance doesn't count.
    assert!(!a.crosses(&cube(9.995, 3.0, 1), 0.01));
    assert!(!a.crosses(&cube(9.85, 3.0, 1), f64::NAN));
    // Crossings left out where the caller knows better: only those, by where they are.
    let corner = cube(9.85, 9.85, 2);
    assert!(!a.crosses_except(&corner, 0.01, |_, _, _| true));
    assert!(!a.crosses_except(&corner, 0.01, |p, _, _| p.x > 9.8 && p.y > 9.8));
    assert!(a.crosses_except(&corner, 0.01, |p, _, _| p.x < 9.8));
    // And by the faces that cross: `a`'s side at x = 10 through the corner's at y = 9.85, and
    // `a`'s at y = 10 through the corner's at x = 9.85.
    assert!(!a.crosses_except(&corner, 0.01, |_, f, g| matches!((f, g), (1, 2) | (3, 0))));
    assert!(a.crosses_except(&corner, 0.01, |_, f, g| (f, g) == (1, 2)));
    assert!(a.crosses_except(&corner, 0.01, |_, f, g| (f, g) == (3, 0)));
}

/// A closed box mesh `[x0, x0+a] x [y0, y0+b] x [z0, z0+c]`, outward-facing.
fn offset_box(o: Vec3, a: f64, b: f64, c: f64) -> Mesh {
    let p = |x: f64, y: f64, z: f64| o + Vec3::new(x * a, y * b, z * c);
    let positions = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)];
    let triangles =
        vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
    Mesh { positions, triangles, ..Default::default() }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-12)
}

#[test]
fn mass_properties_of_a_box_match_the_closed_form() {
    let (a, b, c) = (20.0, 30.0, 40.0);
    let m = offset_box(Vec3::ZERO, a, b, c).mass_properties().expect("a closed box has mass properties");
    let v = a * b * c;
    assert!(close(m.volume, v, 1e-12), "{m:?}");
    assert!((m.centroid - Vec3::new(10.0, 15.0, 20.0)).len() < 1e-9, "{m:?}");
    let i = m.inertia;
    assert!(close(i[0][0], v * (b * b + c * c) / 12.0, 1e-9), "{i:?}");
    assert!(close(i[1][1], v * (a * a + c * c) / 12.0, 1e-9), "{i:?}");
    assert!(close(i[2][2], v * (a * a + b * b) / 12.0, 1e-9), "{i:?}");
    for (r, cc) in [(0, 1), (0, 2), (1, 2)] {
        assert!(i[r][cc].abs() < 1e-6 && i[cc][r].abs() < 1e-6, "{i:?}");
    }
}

#[test]
fn mass_properties_of_a_cylinder_are_close() {
    // A cylinder of radius 10 and height 30 along z, 256 sides, capped by fans.
    let (r, h, n) = (10.0, 30.0, 256usize);
    let mut positions = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, h)];
    for k in 0..n {
        let t = std::f64::consts::TAU * k as f64 / n as f64;
        positions.push(Vec3::new(r * t.cos(), r * t.sin(), 0.0));
        positions.push(Vec3::new(r * t.cos(), r * t.sin(), h));
    }
    let mut triangles = Vec::new();
    for k in 0..n as u32 {
        let (b0, t0) = (2 + 2 * k, 3 + 2 * k);
        let (b1, t1) = (2 + 2 * ((k + 1) % n as u32), 3 + 2 * ((k + 1) % n as u32));
        triangles.push([0, b1, b0]);
        triangles.push([1, t0, t1]);
        triangles.push([b0, b1, t1]);
        triangles.push([b0, t1, t0]);
    }
    let m = Mesh { positions, triangles, ..Default::default() }.mass_properties().expect("closed");
    let v = std::f64::consts::PI * r * r * h;
    assert!(close(m.volume, v, 1e-3), "{m:?}");
    assert!(close(m.inertia[2][2], m.volume * r * r / 2.0, 1e-3), "{m:?}");
    assert!(close(m.inertia[0][0], m.volume * (3.0 * r * r + h * h) / 12.0, 1e-3), "{m:?}");
}

#[test]
fn mass_properties_are_translation_invariant_about_the_centroid() {
    let at_origin = offset_box(Vec3::ZERO, 20.0, 30.0, 40.0).mass_properties().unwrap();
    let moved = offset_box(Vec3::new(100.0, -50.0, 7.0), 20.0, 30.0, 40.0).mass_properties().unwrap();
    assert!((moved.centroid - Vec3::new(110.0, -35.0, 27.0)).len() < 1e-9, "{moved:?}");
    for r in 0..3 {
        for c in 0..3 {
            assert!((moved.inertia[r][c] - at_origin.inertia[r][c]).abs() <= 1e-9 * at_origin.inertia[0][0], "{r}{c}");
        }
    }
}

#[test]
fn mass_properties_of_an_inward_facing_box_are_positive() {
    let mut m = offset_box(Vec3::ZERO, 2.0, 3.0, 4.0);
    for t in &mut m.triangles {
        t.swap(1, 2);
    }
    let p = m.mass_properties().unwrap();
    assert!(close(p.volume, 24.0, 1e-12) && p.inertia[0][0] > 0.0, "{p:?}");
}

#[test]
fn mass_properties_of_an_open_or_empty_mesh_are_none() {
    let mut open = offset_box(Vec3::ZERO, 20.0, 30.0, 40.0);
    open.triangles.truncate(10);
    assert!(open.mass_properties().is_none(), "a box missing a face has no mass");
    assert!(Mesh::default().mass_properties().is_none());
}

#[test]
fn mass_properties_need_consistently_oriented_faces() {
    // Every edge still has two triangles, but one face of the box points inwards: its signed
    // volume would cancel part of the rest.
    let mut m = offset_box(Vec3::ZERO, 20.0, 30.0, 40.0);
    for t in m.triangles.iter_mut().take(2) {
        t.swap(1, 2);
    }
    assert!(m.mass_properties().is_none());
}

mod close_pairs {
    use super::*;

    /// A small deterministic generator, so the random cases below repeat from run to run.
    struct Lcg(u64);

    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        fn point(&mut self) -> Vec3 {
            Vec3::new(self.unit(), self.unit(), self.unit())
        }
    }

    fn boxed(tri: [Vec3; 3]) -> BoxedTriangle {
        let mut bounds = Aabb3::EMPTY;
        for p in tri {
            bounds.add(p);
        }
        (tri, bounds)
    }

    /// `count` small triangles scattered over a cube of side `spread` at `origin`. On the lattice their
    /// corners sit on whole numbers, so many touch exactly, share edges or lie in one plane.
    fn scatter(rng: &mut Lcg, count: usize, origin: Vec3, spread: f64, size: f64, lattice: bool) -> Vec<BoxedTriangle> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let centre = origin + rng.point() * spread;
            let mut tri = [Vec3::ZERO; 3];
            for corner in &mut tri {
                let mut p = centre + (rng.point() - Vec3::new(0.5, 0.5, 0.5)) * (2.0 * size);
                if lattice {
                    p = Vec3::new(p.x.round(), p.y.round(), p.z.round());
                }
                *corner = p;
            }
            out.push(boxed(tri));
        }
        out
    }

    /// A pair that "meets" for these tests: two corners within `gap` of each other.
    fn corners_meet(t: &[Vec3; 3], u: &[Vec3; 3], gap: f64) -> bool {
        t.iter().any(|p| u.iter().any(|q| p.dist(*q) <= gap))
    }

    fn boxes_close(x: &Aabb3, y: &Aabb3, gap: f64) -> bool {
        x.min.x <= y.max.x + gap
            && y.min.x <= x.max.x + gap
            && x.min.y <= y.max.y + gap
            && y.min.y <= x.max.y + gap
            && x.min.z <= y.max.z + gap
            && y.min.z <= x.max.z + gap
    }

    /// Every pair tried, which is how the contact tests worked before they used a grid.
    fn every_pair(a: &[BoxedTriangle], b: &[BoxedTriangle], gap: f64) -> bool {
        a.iter().any(|(x, xb)| b.iter().any(|(y, yb)| boxes_close(xb, yb, gap) && corners_meet(x, y, gap)))
    }

    /// A flat grid of `n` x `n` squares, two triangles each, at height `z`.
    fn flat_grid(n: usize, z: f64) -> Vec<BoxedTriangle> {
        let p = |i: usize, j: usize| Vec3::new(i as f64, j as f64, z);
        let mut out = Vec::with_capacity(2 * n * n);
        for i in 0..n {
            for j in 0..n {
                out.push(boxed([p(i, j), p(i + 1, j), p(i + 1, j + 1)]));
                out.push(boxed([p(i, j), p(i + 1, j + 1), p(i, j + 1)]));
            }
        }
        out
    }

    /// The grid hands `test` the pairs that trying every pair finds: on random triangles, on
    /// triangles that touch exactly (whole-number corners) and far from the origin.
    #[test]
    fn the_grid_finds_the_pairs_that_trying_every_pair_finds() {
        let mut rng = Lcg(0x232);
        let (mut met, mut apart) = (0, 0);
        for case in 0..600usize {
            let lattice = case % 3 == 0;
            let origin = if case % 5 == 0 { Vec3::new(1.0e6, -2.0e6, 3.0e6) } else { Vec3::ZERO };
            let gap = [1e-9, 0.01, 0.2][(case / 3) % 3];
            let (na, nb) = (1 + (rng.unit() * 150.0) as usize, 1 + (rng.unit() * 150.0) as usize);
            let shift = (rng.point() - Vec3::new(0.5, 0.5, 0.5)) * 8.0;
            let size = 0.3 + rng.unit();
            let a = scatter(&mut rng, na, origin, 5.0, size, lattice);
            let b = scatter(&mut rng, nb, origin + shift, 5.0, size, lattice);
            let want = every_pair(&a, &b, gap);
            let mut budget = usize::MAX;
            let got = find_close_pair(&a, &b, gap, &mut budget, |(x, _), (y, _)| corners_meet(x, y, gap));
            assert_eq!(got, Some(want), "case {case}: {na} x {nb} triangles, gap {gap}, lattice {lattice}");
            if want {
                met += 1;
            } else {
                apart += 1;
            }
        }
        assert!(met > 50 && apart > 50, "the cases should cover both answers: {met} meet, {apart} apart");
        let mut budget = usize::MAX;
        assert_eq!(find_close_pair(&[], &flat_grid(2, 0.0), 0.1, &mut budget, |_, _| true), Some(false));
        assert_eq!(find_close_pair(&flat_grid(2, 0.0), &[], 0.1, &mut budget, |_, _| true), Some(false));
    }

    /// Two flat meshes of 16,200 triangles each, one above the other: all their boxes overlap, which
    /// is 262 million pairs and far past a budget of 40 million.
    #[test]
    fn big_meshes_with_overlapping_boxes_are_searched_without_trying_every_pair() {
        let (low, high) = (flat_grid(90, 0.0), flat_grid(90, 0.5));
        assert!(low.len() * high.len() > 200_000_000);
        let mut budget = 40_000_000;
        assert_eq!(find_close_pair(&low, &high, 0.1, &mut budget, |(x, _), (y, _)| corners_meet(x, y, 0.1)), Some(false));
        let mut budget = 40_000_000;
        assert_eq!(find_close_pair(&low, &high, 0.6, &mut budget, |(x, _), (y, _)| corners_meet(x, y, 0.6)), Some(true));
    }

    /// A pile of triangles in one place is still a product of pairs: the budget refuses it.
    #[test]
    fn a_pile_of_triangles_in_one_place_runs_out_of_budget() {
        let pile = |height: f64| {
            let up = Vec3::Z * height;
            vec![boxed([up, Vec3::X + up, Vec3::Y + up]); 7000]
        };
        let (low, high) = (pile(0.0), pile(0.5));
        let mut budget = 40_000_000;
        // Their boxes are 0.5 apart, so within a gap of 1 every pair is a candidate: 49 million tests.
        assert_eq!(find_close_pair(&low, &high, 1.0, &mut budget, |_, _| false), None);
    }

    /// The grid's own work counts: a budget too small to file a big mesh is refused even when no
    /// pair is ever tested.
    #[test]
    fn filing_and_visiting_cells_count_against_the_budget() {
        let (low, high) = (flat_grid(30, 0.0), flat_grid(30, 5.0));
        let mut budget = 100;
        assert_eq!(find_close_pair(&low, &high, 0.1, &mut budget, |_, _| false), None);
        let mut budget = 10_000_000;
        assert_eq!(find_close_pair(&low, &high, 0.1, &mut budget, |_, _| false), Some(false));
        assert!(budget < 10_000_000, "work was counted");
    }
}

mod blend {
    use super::*;

    /// `j!` for the small orders the Hermite and quotient identities use.
    fn fact(j: usize) -> f64 {
        (1..=j).fold(1.0, |a, b| a * b as f64)
    }

    /// The derivatives 1…`order` a Bézier's control polygon carries at its end `at_end`: the j-th
    /// difference of the controls, scaled by `n!/(n − j)!`. This is the textbook endpoint identity
    /// of a Bézier, written here independently of the library's.
    fn control_derivatives(ctrl: &[Vec2], at_end: bool, order: usize) -> Vec<Vec2> {
        let c: Vec<Vec2> = if at_end { ctrl.iter().rev().copied().collect() } else { ctrl.to_vec() };
        let n = c.len() - 1;
        let (mut diffs, mut scale) = (c, 1.0);
        let mut out = Vec::new();
        for j in 1..=order {
            diffs = diffs.windows(2).map(|w| w[1] - w[0]).collect();
            scale *= (n + 1 - j) as f64;
            out.push(diffs.first().copied().unwrap_or_default() * scale);
        }
        // The reversed controls describe `P(1 − s)`, so reversing again to read the original
        // parameterisation negates the odd orders.
        if at_end {
            for (i, v) in out.iter_mut().enumerate() {
                if i % 2 == 0 {
                    *v = -*v;
                }
            }
        }
        out
    }

    #[test]
    fn hermite_bezier_meets_the_derivative_data_at_both_ends() {
        let (p0, p1) = (Vec2::new(1.0, 2.0), Vec2::new(-3.0, 4.5));
        let d0 = [Vec2::new(2.0, 1.0), Vec2::new(-0.5, 3.0), Vec2::new(1.5, 1.0), Vec2::new(0.25, -2.0)];
        let d1 = [Vec2::new(-1.0, 0.5), Vec2::new(2.0, -1.5), Vec2::new(-0.75, 0.5), Vec2::new(3.0, 1.25)];
        for k in 0..=MAX_DERIVATIVE_ORDER {
            let cps = hermite_bezier(p0, p1, &d0[..k], &d1[..k]).unwrap();
            assert_eq!(cps.len(), 2 * k + 2, "degree 2k+1 has 2k+2 controls");
            assert_eq!(cps[0], p0);
            assert_eq!(*cps.last().unwrap(), p1);
            for (got, want) in control_derivatives(&cps, false, k).iter().zip(&d0[..k]) {
                assert!(got.dist(*want) < 1e-11, "k={k} at t=0: {got:?} vs {want:?}");
            }
            for (got, want) in control_derivatives(&cps, true, k).iter().zip(&d1[..k]) {
                assert!(got.dist(*want) < 1e-11, "k={k} at t=1: {got:?} vs {want:?}");
            }
            // The piece is exactly the single Bézier `bezier` hands back, and its ends are the two
            // joints.
            let seg = bezier(&cps).unwrap();
            assert!(seg.start().dist(p0) < 1e-12 && seg.end().dist(p1) < 1e-12);
            for (got, want) in seg.derivatives(0.0, k).unwrap().iter().zip(&d0[..k]) {
                assert!(got.dist(*want) < 1e-10, "k={k} at t=0: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn hermite_bezier_refuses_hostile_data() {
        let (p0, p1) = (Vec2::ZERO, Vec2::X);
        let d = [Vec2::Y, Vec2::X];
        assert!(hermite_bezier(p0, p1, &d, &d).is_some());
        assert!(hermite_bezier(p0, p1, &d, &d[..1]).is_none(), "the ends must agree on the order");
        assert!(hermite_bezier(p0, p1, &[Vec2::ZERO; 5], &[Vec2::ZERO; 5]).is_none(), "order above G4");
        assert!(hermite_bezier(Vec2::new(f64::NAN, 0.0), p1, &[], &[]).is_none());
        assert!(hermite_bezier(p0, p1, &[Vec2::new(f64::INFINITY, 0.0)], &[Vec2::Y]).is_none());
        assert!(hermite_bezier(p0, p1, &[], &[Vec2::Y]).is_none());
    }

    #[test]
    fn seg_derivatives_are_exact_for_lines_and_polynomial_pieces() {
        let line = Seg2::Line { a: Vec2::new(-1.0, 2.0), b: Vec2::new(3.0, 5.0) };
        let d = line.derivatives(0.0, MAX_DERIVATIVE_ORDER).unwrap();
        assert!(d[0].dist(Vec2::new(4.0, 3.0)) < 1e-12);
        assert_eq!(d[1].len() + d[2].len() + d[3].len(), 0.0, "a line has no curvature");
        let cubic = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(3.0, 9.0), p2: Vec2::new(7.0, -4.0), p3: Vec2::new(11.0, 2.0) };
        let ctrl = [Vec2::new(0.0, 0.0), Vec2::new(3.0, 9.0), Vec2::new(7.0, -4.0), Vec2::new(11.0, 2.0)];
        for (t, at_end) in [(0.0, false), (1.0, true)] {
            for (got, want) in cubic.derivatives(t, MAX_DERIVATIVE_ORDER).unwrap().iter().zip(control_derivatives(&ctrl, at_end, 4)) {
                assert!(got.dist(want) < 1e-9, "t={t}: {got:?} vs {want:?}");
            }
        }
        for t in [0.0, 0.35, 1.0] {
            assert!(cubic.derivatives(t, 1).unwrap()[0].dist(cubic.derivative(t)) < 1e-9, "t={t}");
        }
        // A degree-9 piece (the G4 blend's own shape) at both ends.
        let pts: Vec<Vec2> = (0..10).map(|i| Vec2::new(i as f64 * 0.7, (i as f64 * 1.3).sin())).collect();
        let seg = bezier(&pts).unwrap();
        assert!(matches!(seg, Seg2::Bezier { n: 9, .. }));
        for (t, at_end) in [(0.0, false), (1.0, true)] {
            for (got, want) in seg.derivatives(t, MAX_DERIVATIVE_ORDER).unwrap().iter().zip(control_derivatives(&pts, at_end, 4)) {
                assert!(got.dist(want) < 1e-9, "t={t}: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn seg_derivatives_follow_the_circular_recurrence() {
        let arc = Seg2::Arc { center: Vec2::new(2.0, -1.0), radius: 5.0, start: 0.7, sweep: -1.9 };
        let Seg2::Arc { center, sweep, .. } = arc else { panic!("an arc") };
        for t in [0.0, 0.4, 1.0] {
            let d = arc.derivatives(t, MAX_DERIVATIVE_ORDER).unwrap();
            let p = arc.point_at(t);
            let sw2 = sweep * sweep;
            // On a circle every derivative follows from the second: P″ = −sw²(P − c), and the rest
            // differentiate that relation, so a line and an arc have zero curvature rates.
            assert!(d[1].dist((p - center) * -sw2) < 1e-9, "t={t}");
            assert!(d[2].dist(d[0] * -sw2) < 1e-9, "t={t}");
            assert!(d[3].dist(d[1] * -sw2) < 1e-9, "t={t}");
            assert!(d[0].dist(arc.derivative(t)) < 1e-9, "t={t}");
        }
    }

    #[test]
    fn seg_derivatives_of_a_conic_satisfy_the_quotient_identity() {
        let conic = Seg2::Conic { a: Vec2::new(-2.0, 0.0), apex: Vec2::new(0.0, 3.0), b: Vec2::new(4.0, 0.5), w: 0.7 };
        let Seg2::Conic { a, apex, b, w } = conic else { panic!("a conic") };
        // The rational quadratic's numerator and weight polynomials, straight from their Bernstein
        // form, and their power coefficients at `t` by differences (exact for a quadratic).
        let n_at = |x: f64| a * ((1.0 - x) * (1.0 - x)) + apex * (2.0 * w * x * (1.0 - x)) + b * (x * x);
        let d_at = |x: f64| (1.0 - x) * (1.0 - x) + 2.0 * w * x * (1.0 - x) + x * x;
        for t in [0.0, 0.25, 1.0] {
            let h = 1e-3;
            // Taylor coefficients at `t` (differences are exact for a quadratic): f, f′/1!, f″/2!.
            let coef_v = |f: &dyn Fn(f64) -> Vec2| [f(t), (f(t + h) - f(t - h)) / (2.0 * h), (f(t + h) - f(t) * 2.0 + f(t - h)) / (2.0 * h * h)];
            let coef_s = |f: &dyn Fn(f64) -> f64| [f(t), (f(t + h) - f(t - h)) / (2.0 * h), (f(t + h) - f(t) * 2.0 + f(t - h)) / (2.0 * h * h)];
            let n = coef_v(&n_at);
            let d = coef_s(&d_at);
            let got = conic.derivatives(t, MAX_DERIVATIVE_ORDER).unwrap();
            // Taylor coefficients of P: P(t) and P⁽ʲ⁾(t)/j!. Leibniz on N = P·D then reads
            // Σ xᵢ·dⱼ₋ᵢ = nⱼ at every order, which the quotient rule must reproduce.
            let x: Vec<Vec2> = std::iter::once(conic.point_at(t)).chain(got.iter().enumerate().map(|(i, v)| *v / fact(i + 1))).collect();
            for j in 0..=4usize {
                let lhs: Vec2 = (0..=j).fold(Vec2::ZERO, |acc, i| acc + x[i] * d.get(j - i).copied().unwrap_or(0.0));
                let rhs = n.get(j).copied().unwrap_or(Vec2::ZERO);
                assert!(lhs.dist(rhs) < 1e-8 * rhs.len().max(1.0), "t={t} j={j}: {lhs:?} vs {rhs:?}");
            }
            assert!(got[0].dist(conic.derivative(t)) < 1e-8, "t={t}");
        }
    }

    #[test]
    fn seg_derivatives_refuse_hostile_input_and_degenerate_pieces() {
        let c = Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::X, p2: Vec2::Y, p3: Vec2::X + Vec2::Y };
        assert!(c.derivatives(0.0, MAX_DERIVATIVE_ORDER + 1).is_none(), "no order above G4");
        assert!(c.derivatives(f64::NAN, 2).is_none());
        let nan = Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::X };
        assert!(nan.derivatives(0.0, 1).is_none());
        // A conic whose weight vanishes is a straight chord drawn at zero speed: its derivatives
        // are finite, but the first is zero, which is what a blend refuses.
        let flat = Seg2::Conic { a: Vec2::ZERO, apex: Vec2::new(1.0, 1.0), b: Vec2::new(2.0, 0.0), w: 0.0 };
        let d = flat.derivatives(0.0, 1).unwrap();
        assert_eq!(d[0], Vec2::ZERO);
    }
}

#[test]
fn a_box_from_points_holds_them_and_grows() {
    let b: Aabb3 = [Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, 0.0, 5.0), Vec3::new(f64::INFINITY, 0.0, 0.0)].into_iter().collect();
    assert_eq!((b.min, b.max), (Vec3::new(-1.0, 0.0, 3.0), Vec3::new(1.0, 2.0, 5.0)), "the infinite point is skipped");
    assert!(b.contains(Vec3::new(0.0, 1.0, 4.0)) && !b.contains(Vec3::new(0.0, 1.0, 6.0)));
    assert!(!b.contains(Vec3::new(f64::NAN, 1.0, 4.0)));
    assert!(b.inflated(1.0).contains(Vec3::new(0.0, 1.0, 6.0)));
    assert!(Aabb3::EMPTY.inflated(1e9).is_empty() && !Aabb3::EMPTY.contains(Vec3::ZERO));
    let far = Aabb3 { min: Vec3::new(2.0, 0.0, 3.0), max: Vec3::new(3.0, 1.0, 4.0) };
    assert!(!b.meets(&far) && b.inflated(1.0).meets(&far) && !b.meets(&Aabb3::EMPTY));
    let both = b.intersection(&b.inflated(1.0).intersection(&Aabb3 { min: Vec3::new(0.0, 1.0, 4.0), max: Vec3::new(9.0, 9.0, 9.0) }));
    assert_eq!((both.min, both.max), (Vec3::new(0.0, 1.0, 4.0), Vec3::new(1.0, 2.0, 5.0)));
    assert!(b.intersection(&far).is_empty());
}
