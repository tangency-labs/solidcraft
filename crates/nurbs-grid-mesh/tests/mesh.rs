//! Checks against answers known in closed form. The dense-surface check is the `coral` example.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::HashMap;
use std::f64::consts::PI;

use nurbs_grid_mesh::{
    deviation, grid_lines, mesh_trimmed, Dir, LoopPoint, MeshError, MeshOptions, NurbsSurface,
    TrimmedMesh,
};

/// Uniform clamped knots for `n` poles of degree `p` over [0, 1].
fn knots(n: usize, p: usize) -> Vec<f64> {
    let inner = n - p;
    let mut k = vec![0.0; p];
    k.extend((0..=inner).map(|i| i as f64 / inner as f64));
    k.extend(vec![1.0; p]);
    k
}

/// A bicubic patch over a 10 x 10 square in the plane z = 0, one span each way: S(u, v) = (10u, 10v, 0).
fn flat_patch() -> NurbsSurface {
    let poles = (0..4)
        .flat_map(|i| {
            (0..4).map(move |j| [10.0 * f64::from(i) / 3.0, 10.0 * f64::from(j) / 3.0, 0.0])
        })
        .collect();
    NurbsSurface::new(3, 3, knots(4, 3), knots(4, 3), 4, 4, poles, None).unwrap()
}

/// A bumpy bicubic surface, 12 x 12 poles over 10 x 10 with heights up to 2.
fn bumpy_patch() -> NurbsSurface {
    let n = 12;
    let poles = (0..n)
        .flat_map(|i| {
            (0..n).map(move |j| {
                let (x, y) = (i as f64, j as f64);
                [
                    10.0 * x / (n - 1) as f64,
                    10.0 * y / (n - 1) as f64,
                    2.0 * (x * 1.3).sin() * (y * 0.9).cos(),
                ]
            })
        })
        .collect();
    NurbsSurface::new(3, 3, knots(n, 3), knots(n, 3), n, n, poles, None).unwrap()
}

/// A quarter of a cylinder of radius 5 and height 10, exact: rational quadratic in u, linear in v.
fn quarter_cylinder() -> NurbsSurface {
    let w = 0.5_f64.sqrt();
    let ring = [[5.0, 0.0], [5.0, 5.0], [0.0, 5.0]];
    let poles = ring
        .iter()
        .flat_map(|&[x, y]| [[x, y, 0.0], [x, y, 10.0]])
        .collect();
    let weights = [1.0, 1.0, w, w, 1.0, 1.0];
    NurbsSurface::new(2, 1, knots(3, 2), knots(2, 1), 3, 2, poles, Some(&weights)).unwrap()
}

fn on(s: &NurbsSurface, u: f64, v: f64) -> LoopPoint {
    LoopPoint {
        u,
        v,
        p: s.evaluate(u, v).unwrap().point,
    }
}

fn circle(s: &NurbsSurface, c: [f64; 2], r: f64, n: usize, clockwise: bool) -> Vec<LoopPoint> {
    (0..n)
        .map(|k| {
            let a = if clockwise { -1.0 } else { 1.0 } * 2.0 * PI * k as f64 / n as f64;
            on(s, c[0] + r * a.cos(), c[1] + r * a.sin())
        })
        .collect()
}

fn square(s: &NurbsSurface, a: f64, b: f64) -> Vec<LoopPoint> {
    vec![on(s, a, a), on(s, b, a), on(s, b, b), on(s, a, b)]
}

/// Area in the xy plane (signed: up is positive), triangles facing down, and edges used more than twice.
fn facts(m: &TrimmedMesh) -> (f64, usize, usize) {
    let (mut area, mut down) = (0.0, 0);
    let mut edges: HashMap<(u32, u32), usize> = HashMap::new();
    for &[a, b, c] in &m.triangles {
        let (p, q, r) = (
            m.positions[a as usize],
            m.positions[b as usize],
            m.positions[c as usize],
        );
        let z = ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])) / 2.0;
        area += z;
        if z < -1e-12 {
            down += 1;
        }
        for (x, y) in [(a, b), (b, c), (c, a)] {
            *edges.entry((x.min(y), x.max(y))).or_default() += 1;
        }
    }
    (area, down, edges.values().filter(|&&n| n > 2).count())
}

#[test]
fn covers_exactly_the_region_inside_a_loop_facing_the_normal() {
    let s = flat_patch();
    let m = mesh_trimmed(
        &s,
        &[circle(&s, [0.5, 0.5], 0.3, 200, false)],
        false,
        &MeshOptions::new(0.01),
    )
    .unwrap();
    let (area, down, over_shared) = facts(&m);
    // The area of the 200-gon the loop is, in model units: the boundary is the edge's own points.
    let polygon = 100.0 * 9.0 * (2.0 * PI / 200.0).sin();
    assert!(
        (area - polygon).abs() < 1e-9,
        "area {area} against {polygon}"
    );
    assert_eq!(down, 0, "every triangle faces up");
    assert_eq!(over_shared, 0, "no edge belongs to more than two triangles");
    assert!(
        m.positions.iter().all(|p| p.iter().all(|x| x.is_finite())),
        "every vertex evaluated"
    );
}

#[test]
fn cuts_a_hole_and_turns_every_triangle_over_when_reversed() {
    let s = flat_patch();
    let loops = [
        square(&s, 0.1, 0.9),
        circle(&s, [0.5, 0.5], 0.2, 100, false),
    ];
    let expect = 64.0 - 50.0 * 4.0 * (2.0 * PI / 100.0).sin();
    let up = mesh_trimmed(&s, &loops, false, &MeshOptions::new(0.01)).unwrap();
    assert!(
        (facts(&up).0 - expect).abs() < 1e-9,
        "area {} against {expect}",
        facts(&up).0
    );
    let down = mesh_trimmed(&s, &loops, true, &MeshOptions::new(0.01)).unwrap();
    assert!(
        (facts(&down).0 + expect).abs() < 1e-9,
        "reversed: the same area facing down"
    );
}

#[test]
fn takes_loops_either_way_round() {
    let s = flat_patch();
    let a = facts(
        &mesh_trimmed(
            &s,
            &[circle(&s, [0.5, 0.5], 0.3, 200, false)],
            false,
            &MeshOptions::new(0.01),
        )
        .unwrap(),
    )
    .0;
    let b = facts(
        &mesh_trimmed(
            &s,
            &[circle(&s, [0.5, 0.5], 0.3, 200, true)],
            false,
            &MeshOptions::new(0.01),
        )
        .unwrap(),
    )
    .0;
    assert!((a - b).abs() < 1e-12);
}

#[test]
fn meshes_a_hole_smaller_than_one_cell() {
    // The hole crosses no grid line, so one is put through it.
    let s = flat_patch();
    let hole = circle(&s, [0.53, 0.47], 0.001, 12, false);
    let m = mesh_trimmed(
        &s,
        &[square(&s, 0.1, 0.9), hole],
        false,
        &MeshOptions::new(0.01),
    )
    .unwrap();
    let expect = 64.0 - 6.0 * 1e-4 * (2.0 * PI / 12.0).sin();
    let (area, down, over_shared) = facts(&m);
    assert!((area - expect).abs() < 1e-9, "area {area} against {expect}");
    assert_eq!((down, over_shared), (0, 0));
}

#[test]
fn spaces_grid_lines_so_a_cell_strays_no_more_than_the_tolerance() {
    // At each cell center, the bilinear blend of its corners against the surface itself. The bound
    // per direction is the chord sag h^2 |C''| / 8, so the two directions together are within
    // twice the tolerance. A grid sixteen times coarser must fail the same test, or the test
    // proves nothing.
    let s = bumpy_patch();
    let worst = |tol: f64| {
        let u = grid_lines(&s, Dir::U, tol, f64::INFINITY, 64)
            .unwrap()
            .lines;
        let v = grid_lines(&s, Dir::V, tol, f64::INFINITY, 64)
            .unwrap()
            .lines;
        let mut w = 0.0_f64;
        for i in 0..u.len() - 1 {
            for j in 0..v.len() - 1 {
                let c = [
                    (u[i], v[j]),
                    (u[i + 1], v[j]),
                    (u[i + 1], v[j + 1]),
                    (u[i], v[j + 1]),
                ]
                .map(|(a, b)| s.evaluate(a, b).unwrap().point);
                let exact = s
                    .evaluate((u[i] + u[i + 1]) / 2.0, (v[j] + v[j + 1]) / 2.0)
                    .unwrap()
                    .point;
                let d: f64 = (0..3)
                    .map(|k| (exact[k] - (c[0][k] + c[1][k] + c[2][k] + c[3][k]) / 4.0).powi(2))
                    .sum();
                w = w.max(d.sqrt());
            }
        }
        w
    };
    let tol = 0.01;
    assert!(
        worst(tol) <= 2.0 * tol,
        "worst {} against {}",
        worst(tol),
        2.0 * tol
    );
    assert!(
        worst(16.0 * tol) > 2.0 * tol,
        "a grid sixteen times coarser strays further"
    );
}

#[test]
fn a_trimmed_bumpy_face_stays_near_its_surface() {
    // Measured, not assumed: every triangle's seven samples against the surface.
    let s = bumpy_patch();
    let tol = 0.01;
    let loops = [
        circle(&s, [0.5, 0.5], 0.45, 400, false),
        circle(&s, [0.4, 0.55], 0.1, 120, true),
    ];
    let m = mesh_trimmed(&s, &loops, false, &MeshOptions::new(tol)).unwrap();
    let d = deviation(&s, &m, tol).unwrap();
    assert!(d.max <= tol, "max {} against {tol}", d.max);
    assert_eq!(m.capped_spans, 0);
    // A sixteen times looser mesh must be caught by the same measurement.
    let loose = mesh_trimmed(&s, &loops, false, &MeshOptions::new(16.0 * tol)).unwrap();
    assert!(deviation(&s, &loose, tol).unwrap().over > 0);
}

/// A dome of radius 10 whose u = 0 edge collapses to the apex: a pole.
fn pole_dome() -> NurbsSurface {
    let (nu, nv, p) = (6, 7, 3);
    let (ku, kv) = (knots(nu, p), knots(nv, p));
    let greville = |k: &[f64], i: usize| k[i + 1..=i + p].iter().sum::<f64>() / p as f64;
    let poles = (0..nu)
        .flat_map(|i| {
            let g = greville(&ku, i);
            let kv = &kv;
            (0..nv).map(move |j| {
                let a = 1.5 * PI * greville(kv, j);
                [10.0 * g * a.cos(), 10.0 * g * a.sin(), 6.0 * (1.0 - g * g)]
            })
        })
        .collect();
    NurbsSurface::new(p, p, ku.clone(), kv.clone(), nu, nv, poles, None).unwrap()
}

#[test]
fn a_pole_leaves_no_zero_area_triangles_and_stays_within_tolerance() {
    let s = pole_dome();
    let tol = 0.01;
    // The whole domain, each side as an edge polyline of 200 chords (the pole side all one point).
    let side = |a: [f64; 2], b: [f64; 2]| {
        (0..200)
            .map(move |k| f64::from(k) / 200.0)
            .map(move |t| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])])
    };
    let rim: Vec<LoopPoint> = side([0.0, 0.0], [1.0, 0.0])
        .chain(side([1.0, 0.0], [1.0, 1.0]))
        .chain(side([1.0, 1.0], [0.0, 1.0]))
        .chain(side([0.0, 1.0], [0.0, 0.0]))
        .map(|[u, v]| on(&s, u, v))
        .collect();
    let m = mesh_trimmed(&s, &[rim], false, &MeshOptions::new(tol)).unwrap();
    assert!(
        m.dropped_degenerate > 0,
        "the apex row's zero-area triangles are left out"
    );
    for &[a, b, c] in &m.triangles {
        let [p, q, r] = [a, b, c].map(|k| m.positions[k as usize]);
        let (e1, e2) = (
            [q[0] - p[0], q[1] - p[1], q[2] - p[2]],
            [r[0] - p[0], r[1] - p[1], r[2] - p[2]],
        );
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        assert!(
            n.iter().map(|x| x * x).sum::<f64>() > 0.0,
            "a zero-area triangle is left"
        );
    }
    let d = deviation(&s, &m, tol).unwrap();
    assert!(d.max <= tol, "max {} against {tol}", d.max);
}

#[test]
fn a_rational_cylinder_lands_on_its_radius() {
    let s = quarter_cylinder();
    let tol = 0.005;
    // Both arcs as edge polylines of 64 chords (sag 4e-4), bottom out and top back.
    let arc = |v: f64| {
        (0..=64)
            .map(move |k| f64::from(k) / 64.0)
            .map(move |u| (u, v))
    };
    let loops = [arc(0.0)
        .chain(arc(1.0).collect::<Vec<_>>().into_iter().rev())
        .map(|(u, v)| on(&s, u, v))
        .collect::<Vec<_>>()];
    let m = mesh_trimmed(&s, &loops, false, &MeshOptions::new(tol)).unwrap();
    // Grid vertices are the surface's own points; boundary ones lie on the edge chords instead.
    let inside = |[u, v]: [f64; 2]| u > 1e-6 && u < 1.0 - 1e-6 && v > 1e-6 && v < 1.0 - 1e-6;
    for (p, &uv) in m.positions.iter().zip(&m.uvs) {
        if inside(uv) {
            assert!(
                ((p[0] * p[0] + p[1] * p[1]).sqrt() - 5.0).abs() < 1e-12,
                "vertex off the cylinder: {p:?}"
            );
        }
    }
    let d = deviation(&s, &m, tol).unwrap();
    assert!(d.max <= tol, "max {} against {tol}", d.max);
}

#[test]
fn two_faces_sharing_an_edge_polyline_meet_without_a_crack() {
    // The square cut along a wavy polyline into two faces. Welded by position, every edge is used
    // twice except those on the square's outside. With min_cells 1 curvature alone places the
    // lines, so both faces share one grid and meet vertex for vertex; with differing grids they
    // meet along the same 3D polyline but with T-junctions.
    let s = bumpy_patch();
    let cut: Vec<LoopPoint> = (0..=60)
        .map(|k| {
            let t = f64::from(k) / 60.0;
            on(&s, 0.1 + 0.8 * t, 0.5 + 0.2 * (6.0 * t).sin())
        })
        .collect();
    let mut low = vec![on(&s, 0.9, 0.1), on(&s, 0.9, 0.5 + 0.2 * 6.0_f64.sin())];
    low.extend(cut.iter().rev().skip(1).take(cut.len() - 2).copied());
    low.extend([on(&s, 0.1, 0.5), on(&s, 0.1, 0.1)]);
    let mut high = vec![on(&s, 0.1, 0.9), on(&s, 0.1, 0.5)];
    high.extend(cut.iter().skip(1).take(cut.len() - 2).copied());
    high.extend([on(&s, 0.9, 0.5 + 0.2 * 6.0_f64.sin()), on(&s, 0.9, 0.9)]);
    let opts = MeshOptions {
        min_cells: 1,
        ..MeshOptions::new(0.01)
    };
    let a = mesh_trimmed(&s, &[low], false, &opts).unwrap();
    let b = mesh_trimmed(&s, &[high], false, &opts).unwrap();
    let key = |p: [f64; 3]| p.map(|x| (x * 1e8).round() as i64);
    let mut ids: HashMap<[i64; 3], usize> = HashMap::new();
    let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
    let mut uv_of: HashMap<usize, [f64; 2]> = HashMap::new();
    for m in [&a, &b] {
        for &t in &m.triangles {
            let w = t.map(|i| {
                let n = ids.len();
                let id = *ids.entry(key(m.positions[i as usize])).or_insert(n);
                uv_of.insert(id, m.uvs[i as usize]);
                id
            });
            for (x, y) in [(w[0], w[1]), (w[1], w[2]), (w[2], w[0])] {
                *edges.entry((x.min(y), x.max(y))).or_default() += 1;
            }
        }
    }
    let on_outside = |uv: [f64; 2]| {
        [uv[0] - 0.1, 0.9 - uv[0], uv[1] - 0.1, 0.9 - uv[1]]
            .iter()
            .any(|d| d.abs() < 1e-6)
    };
    for (&(x, y), &n) in &edges {
        if n == 1 {
            assert!(
                on_outside(uv_of[&x]) && on_outside(uv_of[&y]),
                "open edge inside the square at {:?}",
                uv_of[&x]
            );
        } else {
            assert_eq!(n, 2, "edge used {n} times");
        }
    }
}

#[test]
fn finds_a_point_on_the_surface_again_from_a_nearby_seed() {
    let s = bumpy_patch();
    for (u, v) in [(0.2, 0.7), (0.55, 0.31), (0.9, 0.12)] {
        let p = s.evaluate(u, v).unwrap().point;
        let back = s.project(p, [u + 0.03, v - 0.03], 1e-9).unwrap().unwrap();
        assert!(
            (back[0] - u).abs() < 1e-9 && (back[1] - v).abs() < 1e-9,
            "({u}, {v}) came back as {back:?}"
        );
    }
}

#[test]
fn refuses_bad_input_by_name() {
    let poles = vec![[0.0; 3]; 15];
    assert!(matches!(
        NurbsSurface::new(3, 3, knots(4, 3), knots(4, 3), 4, 4, poles, None),
        Err(MeshError::BadSurface(_))
    ));
    let s = flat_patch();
    let l = [square(&s, 0.1, 0.9)];
    assert!(matches!(
        mesh_trimmed(&s, &l, false, &MeshOptions::new(f64::NAN)),
        Err(MeshError::BadOptions(_))
    ));
    assert_eq!(
        mesh_trimmed(&s, &[], false, &MeshOptions::new(0.01)),
        Err(MeshError::NoLoops)
    );
}
