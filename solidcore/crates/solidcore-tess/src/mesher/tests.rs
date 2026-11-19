use super::*;

/// Seeds on a motorcycle trace start at the spacing of the boundary it leaves and grow
/// geometrically to the spacing at its other end (here 0.1 to 1 over 5 mm).
#[test]
fn trace_seeds_grade_from_one_end_to_the_other() {
    let t = graded([0.0, 0.0], [5.0, 0.0], 0.1, 1.0, 10.0, [1.0, 1.0], &|_, _| true);
    let all: Vec<f64> = std::iter::once(0.0).chain(t.iter().copied()).chain(std::iter::once(1.0)).map(|f| f * 5.0).collect();
    let sizes: Vec<f64> = all.windows(2).map(|w| w[1] - w[0]).collect();
    assert!((sizes[0] - 0.1).abs() < 0.03, "first {}", sizes[0]);
    assert!((sizes[sizes.len() - 1] - 1.0).abs() < 0.3, "last {}", sizes[sizes.len() - 1]);
    assert!(sizes.windows(2).all(|w| w[1] >= w[0] - 1e-12), "growing: {sizes:?}");
    // Equal ends: even; a segment no longer than its spacing: no seeds.
    let even = graded([0.0, 0.0], [4.0, 0.0], 1.0, 1.0, 10.0, [1.0, 1.0], &|_, _| true);
    assert_eq!(even.len(), 3);
    assert!(graded([0.0, 0.0], [0.5, 0.0], 1.0, 1.0, 10.0, [1.0, 1.0], &|_, _| true).is_empty());
}

/// Thinning keeps a grid within its cap whatever the rounding, both ends of each direction,
/// and never fewer than two lines a direction (a cap below four points takes four).
#[test]
fn thinned_grid_lines_stay_within_the_cap() {
    let lines = |n: usize| (0..n).map(|i| i as f64 / (n - 1) as f64).collect::<Vec<f64>>();
    for (nu, nv, cap) in [(1237, 122, 150_000), (8192, 3, 1000), (3, 8192, 10), (2, 2, 1), (5000, 5000, 4)] {
        let (mut us, mut vs) = (lines(nu), lines(nv));
        grid::thin(&mut us, &mut vs, cap);
        assert!(us.len() * vs.len() <= cap.max(4), "{nu}x{nv} to {cap}: {}x{}", us.len(), vs.len());
        assert!(us.len() >= 2 && vs.len() >= 2);
        assert!(us.first() == Some(&0.0) && us.last() == Some(&1.0) && vs.first() == Some(&0.0) && vs.last() == Some(&1.0));
    }
}

/// Boundary nodes at the very same (u, v) are one vertex to the triangulation: two holes that
/// touch at a vertex they share (a ring standing on a perforated plate, tangent to the holes
/// beside it) bound a face the triangles follow, though each hole has its own copy of the node.
#[test]
fn loops_touching_at_a_shared_vertex_bound_the_face() {
    let outer = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    let left = [[1.0, 1.0], [1.0, 3.0], [2.0, 2.0]];
    let right = [[2.0, 2.0], [3.0, 3.0], [3.0, 1.0]];
    let uvs: Vec<[f64; 2]> = outer.iter().chain(&left).chain(&right).copied().collect();
    let fixed: Vec<Vec3> = uvs.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    let ranges = vec![0..4, 4..7, 7..10];
    let fill = cdt::Fill { seeds: &[], refine: None, shape: false, lenient: false, flat: None };
    let Some((_, triangles)) = cdt::triangulate(&uvs, &ranges, fill) else { panic!("triangulates") };
    assert!(closed_boundary(&ranges, &fixed, &uvs, &triangles), "the triangles follow both holes through their shared vertex");
}

/// A face meeting itself along a line (a hole's wall tangent inside the wall round it, cut from
/// the boss side's top rim down to the hole's bottom): its loop runs up the slit and back down
/// it, with the face on both sides. The iso-line grid's band beside the slit is the face on
/// either side of it, and the triangles follow the slit both ways and cover the whole square.
#[test]
fn a_slit_in_a_trimmed_face_has_the_face_on_both_sides() {
    // The square [0, 8] × [0, 4], its loop starting at the slit's tip (3, 1) up to the top, round
    // the square and back down the slit.
    let mut uvs: Vec<[f64; 2]> = (1..=4).map(|k| [3.0, k as f64]).collect();
    uvs.extend((0..3).rev().map(|k| [k as f64, 4.0]));
    uvs.extend((0..4).rev().map(|k| [0.0, k as f64]));
    uvs.extend((1..=8).map(|k| [k as f64, 0.0]));
    uvs.extend((1..=4).map(|k| [8.0, k as f64]));
    uvs.extend((3..8).rev().map(|k| [k as f64, 4.0]));
    uvs.extend((2..4).rev().map(|k| [3.0, k as f64]));
    let fixed: Vec<Vec3> = uvs.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    let ranges = [std::ops::Range { start: 0, end: uvs.len() }];
    let traces = motorcycle::traces(&uvs, &ranges, [1.0, 1.0], 0.0);
    let lines = |n: usize, top: f64| (0..=n).map(|k| top * k as f64 / n as f64).collect::<Vec<f64>>();
    let Some((extra, triangles)) = cutgrid::cut_grid(&uvs, &ranges, &lines(16, 8.0), &lines(8, 4.0), &traces, [1.0, 1.0]) else { panic!("meshes") };
    assert!(closed_boundary(&ranges, &fixed, &uvs, &triangles), "the triangles follow the slit both ways");
    let mut all = uvs.clone();
    all.extend(extra);
    let area: f64 = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| all[i as usize]);
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
        })
        .sum();
    assert!((area - 32.0).abs() < 1e-12, "area {area}");
}

/// A doubly curved patch 40 mm across, and a loop on it in (u, v) whose right side folds back
/// past itself near the top: the last chord of the fold crosses the side, as a cusp or a sliver
/// whose sides sag across each other leaves a face's boundary.
fn a_face_crossing_itself() -> (EvalSurface, seam::Loop, size::Rules) {
    let mut ctrl = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let bump = if i == 1 && j == 1 { 12.0 } else { 0.0 };
            ctrl.push([20.0 * f64::from(i), 20.0 * f64::from(j), bump, 1.0]);
        }
    }
    let s = EvalSurface::Nurbs {
        degree: [2, 2],
        knots: [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]],
        count: [3, 3],
        ctrl,
        closed: [false, false],
    };
    let corners = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.7], [0.7, 0.95], [0.86, 0.8], [0.1, 0.9]];
    let mut uv: Vec<[f64; 2]> = Vec::new();
    for k in 0..corners.len() {
        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
        for t in 0..8 {
            let f = f64::from(t) / 8.0;
            uv.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
        }
    }
    let pts: Vec<Vec3> = uv.iter().filter_map(|q| s.derivs(q[0], q[1]).map(|(p, _, _)| p)).collect();
    assert_eq!(pts.len(), uv.len());
    (s, seam::Loop { uv, pts }, size::Rules::new(0.05, 40.0, None))
}

/// A curved face crossing itself in (u, v) is refused by every method; the lenient fill leaves
/// the crossing side out and plans the rest in (u, v), with points inside the face on its surface
/// (the GPU evaluates them there), not flat between boundary nodes.
#[test]
fn a_curved_face_crossing_itself_is_filled_on_its_surface() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let loops = std::slice::from_ref(&l);
    let mut stats = PlanStats::default();
    assert!(plan_face(0, "spline", &poles::Face::new(s.clone(), 0.0), loops, &rules, size, 0.0, &mut stats, Lenient::No, Purpose::Display).is_err());
    let Ok(Planned::Face(fp)) = plan_face(0, "spline", &poles::Face::new(s.clone(), 0.0), loops, &rules, size, 0.0, &mut stats, Lenient::Yes, Purpose::Display) else { panic!("a lenient fill") };
    assert!(!fp.triangles.is_empty());
    assert!(fp.uv.len() > fp.fixed.len(), "{} points, {} on the boundary", fp.uv.len(), fp.fixed.len());
    // Each point inside the face on the surface where its parameters put it.
    let mesh = TessPlan { surfaces: vec![s.clone()], faces: vec![FacePlan { surface: Some(0), ..fp.clone() }], ..TessPlan::default() }.evaluate().0;
    for (q, p) in fp.uv.iter().zip(&mesh.positions).skip(fp.fixed.len()) {
        let Some((on, _, _)) = s.derivs(q[0], q[1]) else { panic!("evaluates at {q:?}") };
        assert!(on.dist(solidcore_convert::vec3(*p)) < 1e-9, "{q:?}");
    }
}

/// The face filled leniently is named in the plan's stats with why the strict plan refused it, and
/// counted under no method: the display has no hole, and the defect is still reported.
#[test]
fn a_face_filled_leniently_is_named_in_the_stats() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let Ok((Planned::Face(fp), stats)) = plan_or_fill(7, "spline", &poles::Face::new(s.clone(), 0.0), std::slice::from_ref(&l), &rules, size, 0.0, None, Purpose::Display) else { panic!("filled leniently") };
    assert!(!fp.triangles.is_empty());
    assert_eq!((stats.planar, stats.structured, stats.delaunay), (0, 0, 0));
    let [lenient] = stats.lenient.as_slice() else { panic!("one lenient face: {:?}", stats.lenient) };
    assert_eq!(lenient.face, 7);
    assert!(lenient.reason.contains("face 7 (spline)"), "{}", lenient.reason);
}

/// A face whose boundary still crosses itself after untangling (two edges leaving a vertex
/// tangentially: their chords cross however often they are halved, as on one of Poppy
/// Humanoid's faces) is filled leniently straight away with that reason, and named: the body's
/// other faces are planned as usual instead of the whole body failing with it.
#[test]
fn a_face_untangling_cannot_resolve_is_filled_leniently_and_named() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let why = "face 7 (spline): boundary crosses itself: edges [1, 2] run closer together than their chords can follow";
    let Ok((Planned::Face(fp), stats)) = plan_or_fill(7, "spline", &poles::Face::new(s.clone(), 0.0), std::slice::from_ref(&l), &rules, size, 0.0, Some(why), Purpose::Display) else { panic!("filled leniently") };
    assert!(!fp.triangles.is_empty());
    let [lenient] = stats.lenient.as_slice() else { panic!("one lenient face: {:?}", stats.lenient) };
    assert_eq!(lenient.face, 7);
    assert!(lenient.reason.contains(why), "{}", lenient.reason);
}

/// A sheet whose section is an S (a cubic 30 long, 1.5 off its chord each way, its middle on the
/// chord), straight across, meshed as fans with one triangle flat on the chord across its middle
/// (A, B at the sides, C a hair above the chord a third along). The chord's middle lies on the
/// surface while the triangle's short sides sag: splitting those left a child with the chord and
/// its vertex nearer to it, without end (a loft side of a bike frame refined to slivers until the
/// point budget ran out, its area counted three times). The flat triangle now splits the chord,
/// and refinement ends within its budget with no sliver.
#[test]
fn sag_refinement_across_an_inflection_ends_without_slivers() {
    let mut ctrl = Vec::new();
    for j in 0..2 {
        for (i, z) in [0.0, 4.0, -4.0, 0.0].into_iter().enumerate() {
            ctrl.push([10.0 * i as f64, 10.0 * f64::from(j), z, 1.0]);
        }
    }
    let s = EvalSurface::Nurbs {
        degree: [3, 1],
        knots: [vec![0., 0., 0., 0., 1., 1., 1., 1.], vec![0., 0., 1., 1.]],
        count: [4, 2],
        ctrl,
        closed: [false, false],
    };
    // Boundary: bottom and top rows of n + 1 nodes (their sides within the tolerance, 0.5), A and
    // B halfway up the ends. Inside: C. Bottom fan from A, the triangle A B C, top fan from C.
    let n = 6u32;
    let mut uv: Vec<[f64; 2]> = Vec::new();
    for i in 0..=n {
        uv.push([f64::from(i) / f64::from(n), 0.0]);
    }
    for i in 0..=n {
        uv.push([f64::from(i) / f64::from(n), 1.0]);
    }
    let (bottom, top) = (0u32, n + 1);
    let (a, b, c) = (2 * n + 2, 2 * n + 3, 2 * n + 4);
    uv.extend([[0.0, 0.5], [1.0, 0.5], [1.0 / 3.0, 0.5 + 1e-3]]);
    let mut tris: Vec<[u32; 3]> = vec![[a, bottom, bottom + 1]];
    tris.extend((1..n).map(|i| [a, bottom + i, bottom + i + 1]));
    tris.push([a, bottom + n, b]);
    tris.push([a, b, c]);
    tris.push([c, b, top + n]);
    tris.extend((0..n).rev().map(|i| [c, top + i + 1, top + i]));
    tris.push([c, top, a]);
    // The sides along the ends are straight.
    let fixed: Vec<Vec3> = Vec::new();
    let tol = 0.5;
    let start = uv.len();
    let face = poles::Face::new(s.clone(), 0.0);
    sag::refine(&face, tol, &sag::Poles::new(&face, 1e-9), &fixed, &mut uv, &mut tris, 20_000);
    assert!(uv.len() - start < 1_000, "{} points added: refinement ran to its budget", uv.len() - start);
    for t in &tris {
        let [p, q, r] = t.map(|i| uv[i as usize]);
        let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
        let longest = [(p, q), (q, r), (r, p)].iter().map(|(x, y)| (x[0] - y[0]).hypot(x[1] - y[1])).fold(0.0, f64::max);
        assert!(area > 1e-6 * longest * longest, "a sliver {t:?}: {p:?} {q:?} {r:?}");
    }
}

/// The two triangles are flat on the support in (u, v), but their held vertices are
/// above it. Subdividing their shared side cannot repair the boundary displacement.
#[test]
fn sag_refinement_keeps_a_displaced_flat_boundary_without_splitting() {
    let offset = 0.05;
    let tol = 0.01;
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let fixed = corners.map(|[u, v]| Vec3::new(u, v, offset));
    let mut uv = corners.to_vec();
    let mut triangles = vec![[0, 1, 2], [0, 2, 3]];
    let face = poles::Face::new(EvalSurface::Plane { origin: Vec3::ZERO, du: Vec3::X, dv: Vec3::Y }, offset);
    let poles = sag::Poles::new(&face, 1e-9);
    let refinement = sag::refine(&face, tol, &poles, &fixed, &mut uv, &mut triangles, 64);
    assert!(!refinement.capped && refinement.boundary_has_residual(&face, tol, &poles, &fixed, &uv, &triangles));
    assert_eq!(uv.len(), corners.len(), "added {} points to a flat support", uv.len() - corners.len());
    assert_eq!(triangles, [[0, 1, 2], [0, 2, 3]]);
    let (measured, unmeasured) = sag::worst(&face, &poles, &fixed, &uv, &triangles);
    assert_eq!(unmeasured, 0);
    assert!((measured - offset).abs() < 1e-12, "measured {measured} mm, wanted {offset} mm");
}

/// A radius-4 cylinder over two radians and six millimetres. Its boundary lies at
/// radius `4 + offset`, within its own tolerance. Four rational quadratic arcs in
/// the same parameter make the face's two rims and its surface's two control rows.
fn cylinder_with_displaced_boundary(offset: f64) -> brep::Brep {
    let radius = 4.0;
    let height = 6.0;
    let rim = radius + offset;
    let tol = offset.max(1e-6);
    let angle = |i: u32| 0.5 + 0.25 * f64::from(i);
    let point = |a: f64, z| [rim * a.cos(), rim * a.sin(), z];
    let points = [point(angle(0), 0.0), point(angle(8), 0.0), point(angle(8), height), point(angle(0), height)];
    let arc_knots = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
    let control = |r, z, i| {
        let a = angle(i);
        let w = if i % 2 == 0 { 1.0 } else { 0.25_f64.cos() };
        [r * a.cos(), r * a.sin(), z * w, w]
    };
    let arc = |z| Curve::RationalSpline { knots: arc_knots.clone(), homogeneous: (0..=8).map(|i| control(rim, z, i)).collect() };
    let line = |a: usize, b: usize| Curve::Line { start: points[a], end: points[b] };
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut edges = Vec::new();
    let mut uses = Vec::new();
    for (i, (curve, vertices, forward)) in [
        (arc(0.0), [0, 1], true),
        (line(1, 2), [1, 2], true),
        (arc(height), [3, 2], false),
        (line(0, 3), [0, 3], false),
    ].into_iter().enumerate() {
        let position = u32::try_from(i).unwrap();
        let pcurve = brep::Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vertices.map(|j| corners[j as usize]).to_vec(),
        };
        edges.push(brep::Edge {
            curve,
            nurbs: None,
            interval: [0.0, 1.0],
            vertices,
            tolerance: tol,
            uses: vec![brep::EdgeUseRef { face: 0, r#loop: 0, position, upper: !forward }],
        });
        uses.push(brep::OrientedEdge { edge: position, forward, pcurve });
    }
    brep::Brep {
        version: brep::VERSION,
        tolerance: tol,
        regions: vec![brep::Region { shells: vec![0], void: false }],
        shells: vec![Shell::Faces { faces: vec![brep::OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
        faces: vec![brep::Face {
            surface: brep::Surface::Cylinder {
                frame: brep::Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] },
                radius,
            },
            nurbs: Some(brep::Surface::RationalSpline {
                u_knots: arc_knots,
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                homogeneous: (0..=8).map(|i| vec![control(radius, 0.0, i), control(radius, height, i)]).collect(),
            }),
            domain: [[0.0, 1.0], [0.0, 1.0]],
            loops: vec![Loop::Edges { edges: uses }],
        }],
        edges,
        vertices: points.into_iter().map(|point| brep::Vertex { point, tolerance: tol }).collect(),
    }
}

#[test]
fn sag_refinement_keeps_displaced_cylinder_rims_and_reports_their_gap() {
    let chord = 0.01;
    let area = 4.0 * 2.0 * 6.0;
    for offset in [0.0, 0.05, 0.1] {
        let record = cylinder_with_displaced_boundary(offset);
        record.validate().unwrap();
        let (plan, stats) = plan(&record, &crate::MeshTolerance::chord(chord), 8.0).unwrap();
        let (mesh, refused) = plan.evaluate();
        assert!(refused.is_empty() && stats.unmeshed.is_empty() && stats.lenient.is_empty(), "offset {offset}: {refused:?} {stats:?}");
        assert!(!mesh.triangles.is_empty() && mesh.triangles.len() < 5_000, "offset {offset}: {} triangles", mesh.triangles.len());

        let [face] = plan.faces.as_slice() else { panic!("expected one face") };
        assert_eq!(plan.edges.len(), 4);
        assert!(plan.edges.iter().all(|edge| edge.len() >= 2));
        for point in plan.edges.iter().flatten() {
            assert!(face.fixed.contains(point), "edge sample {point:?} moved or went missing");
            assert!((point.x.hypot(point.y) - (4.0 + offset)).abs() < 1e-9, "edge sample {point:?} left its radius");
        }
        assert!(face.fixed.iter().all(|point| plan.edges.iter().flatten().any(|p| p == point)));
        assert_eq!(&mesh.positions[..face.fixed.len()], face.fixed.as_slice(), "the evaluated mesh retains the shared edge samples");

        let total: f64 = mesh.triangles.iter().map(|triangle| {
            let [a, b, c] = triangle.map(|i| kernel::vec3(mesh.positions[i as usize]));
            let cross = (b - a).cross(c - a);
            let radial = a + b + c;
            assert!(cross.dot(Vec3::new(radial.x, radial.y, 0.0)) > 0.0, "offset {offset}: reversed or degenerate triangle {triangle:?}");
            0.5 * cross.length()
        }).sum();
        assert!((total - area).abs() <= 0.02 * area, "offset {offset}: area {total} mm², wanted {area} mm²");

        if offset == 0.0 {
            assert!(stats.over_tolerance.is_empty(), "{stats:?}");
        } else {
            let [residual] = stats.over_tolerance.as_slice() else { panic!("offset {offset}: {stats:?}") };
            assert_eq!(residual.face, 0);
            assert_eq!(residual.tol, chord);
            assert_eq!(residual.unmeasured, 0);
            assert!(residual.caps.is_empty(), "boundary displacement is not a budget cap: {residual:?}");
            assert!(!residual.within && residual.sag >= offset - 1e-9, "vertical boundary midpoint retains {offset} mm of sag: {residual:?}");
        }
    }
}

/// A sheet curved across (a parabola 20 wide, 3.2 high at a tenth in from either side) and two
/// triangles standing on a chord across the curve: their corners on the surface, their planes
/// across it, so they count far more than the area under them. The flip lays them along the
/// sheet, on the straight side along it.
#[test]
fn triangles_standing_across_a_curved_sheet_are_flipped_along_it() {
    let mut ctrl = Vec::new();
    for j in 0..2 {
        for (i, z) in [5.0, -5.0, 5.0].into_iter().enumerate() {
            ctrl.push([10.0 * i as f64 - 10.0, 10.0 * f64::from(j), z, 1.0]);
        }
    }
    let s = EvalSurface::Nurbs {
        degree: [2, 1],
        knots: [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 1., 1.]],
        count: [3, 2],
        ctrl,
        closed: [false, false],
    };
    let uv = vec![[0.1, 0.5], [0.9, 0.5], [0.5, 0.52], [0.5, 0.48]];
    let mut tris = vec![[0, 1, 2], [1, 0, 3]];
    let area = |tris: &[[u32; 3]]| -> f64 {
        tris.iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| s.eval(uv[i as usize][0], uv[i as usize][1]).0);
                (b - a).cross(c - a).len() / 2.0
            })
            .sum()
    };
    let before = area(&tris);
    let face = poles::Face::new(s.clone(), 0.0);
    sag::flip(&face, 0.1, &sag::Poles::new(&face, 1e-9), &[], &uv, &mut tris);
    let after = area(&tris);
    let sides: Vec<(u32, u32)> = tris.iter().flat_map(|t| (0..3).map(move |k| (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3])))).collect();
    assert!(sides.contains(&(2, 3)) && !sides.contains(&(0, 1)), "{tris:?}");
    assert!(after < 0.5 * before, "{before} -> {after}");
}

/// A flat face asks for no size: a box's faces are meshed as their boundary alone, two
/// triangles each (#13266), at the display and the export tolerance alike.
#[test]
fn a_box_is_two_triangles_a_face() {
    for name in ["box_40x30x20", "box_2x3x4"] {
        let (b, size) = (crate::fixtures::fixture(name), crate::fixtures::size_of(name));
        for t in [crate::MeshTolerance::display_of(size), crate::MeshTolerance::export_of(size)] {
            let Ok((p, _)) = plan(&b, &t, size) else { panic!("{name}: plans") };
            assert_eq!(p.evaluate().0.triangles.len(), 12, "{name} at {t:?}");
        }
    }
}

/// SolidCraft's coil from SMLib, its lower half (`793-coil-split-by-a-plane-across-its-axis`), at
/// the display and export tolerances: the triangle counts pin the planner, and no face is named
/// over its tolerance. Its longest side is a fifth of its extent over its vertices and its edges' points
/// (57.3 mm); its two vertices alone, the coil's ends, lie 20.4 mm apart.
#[test]
fn the_spline_coil_keeps_its_triangles() {
    let name = "smlib_coil_lower_half";
    let (b, size) = (crate::fixtures::fixture(name), crate::fixtures::size_of(name));
    for (t, want) in [(crate::MeshTolerance::display_of(size), 15_392), (crate::MeshTolerance::export_of(size), 30_678)] {
        let Ok((p, stats)) = plan(&b, &t, size) else { panic!("plans") };
        assert_eq!(p.evaluate().0.triangles.len(), want);
        assert!(stats.over_tolerance.is_empty(), "{:?}", stats.over_tolerance);
    }
}


mod refusals {
    use crate::fixtures::{fixture, size_of};
    use crate::{MeshTolerance, Result, TessPlan, plan};
    use solidcore_record::brep::{self, Loop};

    fn ok<T>(r: Result<T>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    /// An edge bent sharply at a corner no halving reaches (a spline turning back on itself at a
    /// third of its parameter) is halved at the corner until the depth bound: there its chords
    /// follow it to a hair, but no segment across the corner turns less than a quarter turn. The
    /// plan names it with that bound, its sag within the tolerance, and how far it exceeds its
    /// rules. Ported from solidcore-tess's own planner.
    #[test]
    fn an_edge_stopped_at_its_depth_bound_is_named_within_its_tolerance() {
        let mut b = fixture("box_10x6x4");
        b.edges[2].curve = brep::Curve::Spline { knots: vec![0.0, 0.0, 1.0 / 3.0, 1.0, 1.0], points: vec![[10.0, 0.0, 4.0], [10.0, 2.0, 20.0], [10.0, 6.0, 4.0]] };
        b.edges[2].nurbs = None;
        let (_, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
        let [capped] = stats.edges_capped.as_slice() else { panic!("{:?}", stats.edges_capped) };
        assert_eq!((capped.edge, capped.cap), (2, crate::EdgeCap::Depth), "{capped:?}");
        assert!(capped.within && capped.sag <= capped.tol, "{capped:?}");
        assert!(capped.excess > 1.0, "{capped:?}");
    }

    /// A torus (radii 6 and 1.5) at a chord of 2e-4 mm, no facet angle: its sag refinement adds
    /// all the points it may with triangles still off the surface, and the face is still meshed
    /// and named in `PlanStats::over_tolerance` with that bound and the sag its triangles reach,
    /// past the tolerance. Ported from solidcore-tess's own planner.
    #[test]
    fn a_face_whose_refinement_reaches_its_bound_is_named_with_its_sag() {
        let tol = 2e-4;
        let b = fixture("torus_6_1_5");
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(tol), size_of("torus_6_1_5")));
        assert!(stats.unmeshed.is_empty(), "{stats:?}");
        let [o] = stats.over_tolerance.as_slice() else { panic!("{:?}", stats.over_tolerance) };
        assert_eq!((o.face, o.caps.as_slice()), (0, [crate::Cap::Refine].as_slice()), "{o:?}");
        assert!(o.sag > tol && o.sag < 10.0 * tol && o.tol == tol && !o.within, "{o:?}");
        assert!(!p.faces[0].triangles.is_empty());
    }

    fn meshed_faces(p: &TessPlan) -> Vec<u32> {
        p.faces.iter().filter(|f| !f.triangles.is_empty()).map(|f| f.face).collect()
    }

    /// Face `face`'s first pcurve in `b`, moved `du` along u: off its edge.
    fn move_pcurve(b: &mut brep::Brep, face: usize, du: f64) -> u32 {
        let Loop::Edges { edges } = &mut b.faces[face].loops[0] else { panic!("an edge loop") };
        let oe = &mut edges[0];
        match &mut oe.pcurve {
            brep::Curve2::Spline { points, .. } => {
                for p in points.iter_mut() {
                    p[0] += du;
                }
            }
            brep::Curve2::RationalSpline { homogeneous, .. } => {
                for p in homogeneous.iter_mut() {
                    p[0] += du * p[2];
                }
            }
        }
        oe.edge
    }

    /// A cylinder face without its NURBS form (a label alone, an unvalidated record): the mesher
    /// takes the label as the revolution it is, on the label's own parameters, which its edges'
    /// pcurves (made for the form) do not follow, so the face is left out, naming it, through
    /// `plan()`; its two end discs are meshed.
    #[test]
    fn a_face_whose_surface_does_not_evaluate_is_refused_through_plan() {
        let mut b = fixture("cylinder_r10_h5");
        assert!(b.faces[0].nurbs.is_some());
        b.faces[0].nurbs = None;
        let (p, stats) = ok(plan(
            &b,
            &MeshTolerance::chord(0.01),
            size_of("cylinder_r10_h5"),
        ));
        let reasons: Vec<(usize, &str)> = stats
            .unmeshed
            .iter()
            .map(|r| (r.face, r.reason.as_str()))
            .collect();
        assert_eq!(reasons.len(), 1, "{stats:?}");
        assert_eq!(reasons[0].0, 0);
        assert!(reasons[0].1.contains("face 0 (cylinder): edge "), "{}", reasons[0].1);
        assert_eq!(meshed_faces(&p), vec![1, 2]);
    }

    /// A box one of whose faces has a pcurve 1 mm off its edge: that face is refused, naming the
    /// face and the edge, and the other five are meshed. The pcurve's error was the whole body's.
    #[test]
    fn a_face_whose_pcurve_is_off_its_edge_is_refused_and_the_rest_meshed() {
        let mut b = fixture("box_10x6x4");
        let edge = move_pcurve(&mut b, 2, 1.0);
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
        assert_eq!(stats.unmeshed.len(), 1, "{stats:?}");
        let refused = &stats.unmeshed[0];
        assert_eq!(refused.face, 2);
        assert!(
            refused.reason.contains(&format!(
                "face 2 (plane): edge {edge}: node 0: its pcurve is"
            )),
            "{}",
            refused.reason
        );
        assert_eq!(meshed_faces(&p), vec![0, 1, 3, 4, 5]);
        assert_eq!(p.faces.len(), 6);
        let (m, refused) = p.evaluate();
        assert!(refused.is_empty(), "{refused:?}");
        assert!(!m.triangles.is_empty() && !m.tri_face.contains(&2));
    }

    /// An edge that bounds no face (a free edge beside a box) and cannot be sampled is left out of
    /// the plan and named in the stats with why; the box is meshed whole, not refused as a body.
    #[test]
    fn a_free_edge_that_cannot_be_sampled_is_named_and_the_body_meshed() {
        let mut b = fixture("box_10x6x4");
        let mut free = b.edges[3].clone();
        free.uses.clear();
        free.interval = [1.0, 0.0];
        b.edges.push(free);
        let ei = b.edges.len() - 1;
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
        assert!(stats.unmeshed.is_empty(), "{stats:?}");
        assert_eq!(stats.unsampled_edges.len(), 1, "{stats:?}");
        assert_eq!(stats.unsampled_edges[0].edge, ei);
        assert!(
            stats.unsampled_edges[0]
                .reason
                .contains(&format!("tessellation: edge {ei}: interval [1, 0]")),
            "{}",
            stats.unsampled_edges[0].reason
        );
        assert_eq!(meshed_faces(&p), (0..6u32).collect::<Vec<u32>>());
        assert!(p.edges[ei].is_empty());
        assert!(p.edge_faces[ei].is_empty());
        assert!(p.edges[..ei].iter().all(|n| n.len() >= 2));
    }

    /// A box with an edge whose interval is reversed: the edge cannot be sampled, and both faces it
    /// bounds are refused, each naming itself and the edge, with no nodes for the edge. The other
    /// four faces are meshed. The edge's error was the whole body's.
    #[test]
    fn an_edge_that_cannot_be_sampled_refuses_both_its_faces_by_name() {
        let mut b = fixture("box_10x6x4");
        let ei = 3;
        b.edges[ei].interval = [1.0, 0.0];
        let mut faces: Vec<usize> = b.edges[ei].uses.iter().map(|u| u.face as usize).collect();
        faces.sort_unstable();
        faces.dedup();
        assert_eq!(faces.len(), 2);
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
        let refused: Vec<usize> = stats.unmeshed.iter().map(|r| r.face).collect();
        assert_eq!(refused, faces, "{stats:?}");
        for r in &stats.unmeshed {
            assert!(
                r.reason.contains(&format!(
                    "face {} (plane): tessellation: edge {ei}: interval [1, 0]",
                    r.face
                )),
                "{}",
                r.reason
            );
        }
        let meshed: Vec<u32> = (0..6u32)
            .filter(|f| !faces.contains(&(*f as usize)))
            .collect();
        assert_eq!(meshed_faces(&p), meshed);
        assert!(p.edges[ei].is_empty());
        assert!(
            p.edges
                .iter()
                .enumerate()
                .all(|(e, n)| e == ei || n.len() >= 2)
        );
    }

    /// A face with a loop of edges too short to bound anything (here none): the face is refused,
    /// naming the loop, and the rest of the body is meshed. The loop was left out, and the face
    /// meshed without it.
    #[test]
    #[ignore = "SolidCraft's mesher fills a face past a loop with no nodes; refusing it, as solidcore-tess did, is still to come"]
    fn a_face_with_a_loop_of_fewer_than_two_nodes_is_refused_naming_the_loop() {
        let mut b = fixture("box_10x6x4");
        b.faces[4].loops.push(Loop::Edges { edges: Vec::new() });
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
        let reasons: Vec<(usize, &str)> = stats
            .unmeshed
            .iter()
            .map(|r| (r.face, r.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            vec![(
                4,
                "the operation failed: not supported yet: tessellation: face 4 (plane): loop 1 has 0 nodes (0 edge uses)"
            )]
        );
        assert_eq!(meshed_faces(&p), vec![0, 1, 2, 3, 5]);
    }
}

/// A sliver far below the display tolerance, bounded by two edges whose pcurves lie apart in
/// (u, v) (a gap within the edges' tolerance), sampled coarsely: six boundary nodes that are
/// three points in space, each twice. Its sides cross, so it is filled leniently, and every
/// Delaunay triangle has two corners on one point: the fill was left empty and the
/// face not drawn ("1 face(s) could not be meshed", a crankshaft's oil drilling in an engine
/// assembly). It is now fanned from its own boundary nodes, each triangle on three distinct
/// nodes, and still named as filled leniently.
#[test]
fn a_sliver_whose_vertices_come_twice_is_still_drawn() {
    let (s, _, rules) = a_face_crossing_itself();
    let uv = vec![[0.40, 0.40], [0.40, 0.46], [0.42, 0.50], [0.42, 0.40], [0.45, 0.46], [0.45, 0.50]];
    let mut pts: Vec<Vec3> = uv.iter().filter_map(|q| s.derivs(q[0], q[1]).map(|(p, _, _)| p)).collect();
    assert_eq!(pts.len(), uv.len());
    // At this tolerance the sliver is three points in space, each sampled twice along the loop
    // with (u, v) apart: nodes 0 and 1 are one point, 2 and 3 another, 4 and 5 the third.
    pts[1] = pts[0];
    pts[3] = pts[2];
    pts[5] = pts[4];
    let l = seam::Loop { uv, pts: pts.clone() };
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let Ok((Planned::Face(fp), stats)) = plan_or_fill(7, "spline", &poles::Face::new(s.clone(), 0.0), std::slice::from_ref(&l), &rules, size, 0.0, None, Purpose::Display) else { panic!("planned") };
    assert!(!fp.triangles.is_empty(), "the face has triangles");
    assert_eq!(stats.lenient.len(), 1, "{:?}", stats.lenient);
    for t in &fp.triangles {
        let p = t.map(|i| fp.fixed.get(i as usize).copied());
        assert!(p[0] != p[1] && p[1] != p[2] && p[2] != p[0], "a triangle on one node twice: {t:?}");
    }
}


/// A ring 1000 mm across with a 1 mm tube, cut in half at z = 0, with a 0.12 mm cube joined to its
/// flat face (the stress corpus's S0437): every vertex lies within about a millimetre of the cube.
/// The longest side was a fifth of an extent taken from the vertices alone, a fifth of a
/// millimetre for the flat face's two circles 3142 mm long: at 1/10000 of the body they took
/// 19352 and 19840 segments, a few hundred short of [`edges::MAX_EDGE_NODES`], which SolidCraft's
/// join of this body passed. The extent counts the edges' points: at that tolerance and at the
/// display's the body is planned whole, and the tolerance alone splits each circle, into at most
/// twice the segments a chord sagging the tolerance needs (bisection halves a chord at most once
/// past it).
#[test]
fn a_thin_half_torus_is_sized_by_its_edges_not_its_vertices() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::{BooleanOp, make_box, make_torus};
    use crate::vec::VecExt as _;
    let torus = make_torus([0.0; 3], 500.0, 0.5).expect("torus");
    let below = make_box([-501.5, -501.5, -1.5], [1003.0, 1003.0, 1.5]).expect("box");
    let half = boolean(torus, below, BooleanOp::Subtract).expect("cut").expect("a half torus");
    let cube = make_box([499.94, -0.06, -0.12], [0.12, 0.12, 0.12]).expect("cube");
    let joined = boolean(half, cube, BooleanOp::Union).expect("join").expect("a body");
    let b = joined.to_record().expect("the record").0;
    // The torus face, the flat face and the cube's five other faces; the flat face's two circles
    // and the torus's seam, the cube's twelve edges; two seam vertices and the cube's eight.
    assert_eq!((b.faces.len(), b.edges.len(), b.vertices.len()), (7, 15, 10));
    // SolidCraft's size of the body: the box diagonal of its vertices and its edges' points.
    let size = 2f64.sqrt() * 1001.0;
    for t in [crate::MeshTolerance::chord(size * 1e-4), crate::MeshTolerance::display_of(size)] {
        let (p, stats) = plan(&b, &t, size).expect("planned");
        assert!(stats.unmeshed.is_empty(), "{t:?}: {:?}", stats.unmeshed);
        let (tol, _) = t.rules(size);
        // The chord that sags the tolerance on the smaller circle (radius 499.5).
        let chord = (8.0 * 499.5 * tol).sqrt();
        let circles: Vec<(f64, usize)> = p
            .edges
            .iter()
            .map(|n| (n.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>(), n.len() - 1))
            .filter(|(len, _)| *len > 3000.0)
            .collect();
        assert_eq!(circles.len(), 2, "{t:?}: {circles:?}");
        for (len, segments) in circles {
            assert!(segments as f64 <= 2.0 * (len / chord).ceil(), "{t:?}: {segments} segments over {len} mm at a chord of {chord} mm");
        }
    }
}
