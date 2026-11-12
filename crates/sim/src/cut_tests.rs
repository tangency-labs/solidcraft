//! Cutting a cell, checked against the material it is supposed to hold.

use solidcore_record::Vec3;

use super::*;
use crate::elastic::CORNERS;
use crate::test_shapes::block_at;

/// A cell's eight corners, a cube of side `h` centred at `at`.
fn cell(at: Vec3, h: f64) -> [Vec3; 8] {
    std::array::from_fn(|i| at + Vec3::new(CORNERS[i][0], CORNERS[i][1], CORNERS[i][2]) * (h / 2.0))
}

/// How much of a cell the model fills, by sampling it finely. This is the answer the cut is
/// measured against, and it is deliberately worked out a different way -- by counting points
/// inside, which is what the occupancy already does, rather than by any geometry the cut shares.
fn sampled_fill(inside: &dyn Fn(Vec3) -> bool, corners: &[Vec3; 8], n: usize) -> f64 {
    let (lo, hi) = (corners[0], corners[6]);
    let d = hi - lo;
    let mut hits = 0usize;
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let f = |v: usize| (v as f64 + 0.5) / n as f64;
                if inside(lo + Vec3::new(d.x * f(i), d.y * f(j), d.z * f(k))) {
                    hits += 1;
                }
            }
        }
    }
    hits as f64 / (n * n * n) as f64
}

/// Steel, more or less -- the same figures the elasticity tests use.
fn steel() -> crate::elastic::ElasticMaterial {
    crate::elastic::ElasticMaterial {
        young_modulus_pa: 200.0e9,
        poisson_ratio: 0.3,
        expansion_per_k: [1.2e-5; 3],
        density_kg_m3: 7850.0,
        yield_strength_pa: Some(250.0e6),
    }
}

fn total(tets: &[[Vec3; 4]]) -> f64 {
    tets.iter().filter_map(crate::elastic::tet_volume).sum()
}

/// A cell wholly inside the model is the six of Kuhn, filling it exactly.
#[test]
fn a_cell_the_model_fills_comes_back_whole() {
    let block = block_at([0., 0., 0.], [40., 40., 40.]);
    let inside = inside_of(&block);
    let h = 2.0;
    let corners = cell(Vec3::new(20.0, 20.0, 20.0), h);
    let tets = cut_cell(&inside, &corners);

    assert_eq!(tets.len(), 6, "a cell inside the model should be the six of Kuhn");
    let v = total(&tets);
    assert!((v - h * h * h).abs() < 1e-12, "they hold {v} of a {} cell", h * h * h);
    for t in &tets {
        assert!(crate::elastic::tet_volume(t).is_some_and(|v| v > 0.0), "a tetrahedron came back wound the wrong way");
    }
}

/// A cell wholly outside it comes back empty.
#[test]
fn a_cell_the_model_misses_comes_back_empty() {
    let block = block_at([0., 0., 0.], [40., 40., 40.]);
    let inside = inside_of(&block);
    let corners = cell(Vec3::new(60.0, 60.0, 60.0), 2.0);
    assert!(cut_cell(&inside, &corners).is_empty());
}

/// A flat wall through a cell is cut exactly, because the answer is a plain fraction.
///
/// The block's face at x = 40 crosses a cell centred on it, so the material is exactly half of
/// it, and a cut that is right has to give that to the last bit the bisection can resolve --
/// there is no curvature here for the tetrahedra to approximate.
#[test]
fn a_flat_wall_through_a_cell_is_cut_exactly() {
    let block = block_at([0., 0., 0.], [40., 40., 40.]);
    let inside = inside_of(&block);
    let h = 2.0;
    for (at, want) in
        [(Vec3::new(40.0, 20.0, 20.0), 0.5), (Vec3::new(40.0 - h / 4.0, 20.0, 20.0), 0.75), (Vec3::new(40.0 + h / 4.0, 20.0, 20.0), 0.25)]
    {
        let corners = cell(at, h);
        let tets = cut_cell(&inside, &corners);
        let v = total(&tets);
        let want = want * h * h * h;
        assert!((v - want).abs() < 1e-5, "at {at:?} the cut holds {v} where the cell holds {want}");
        for t in &tets {
            assert!(crate::elastic::tet_volume(t).is_some_and(|x| x > 0.0), "a tetrahedron is wound the wrong way");
        }
    }
}

/// A curved wall through a cell is cut to the material the cell holds.
///
/// Measured against sampling, which is how the occupancy works the same number out and shares no
/// geometry with the cut. The two cannot agree exactly, and the direction of the disagreement is
/// the interesting part: the tetrahedra's faces are flat chords of a convex barrel, so they lie
/// *inside* it and the cut is always a little short. Both halves of that are asserted -- the
/// size of it, and that it is never over.
#[test]
fn a_curved_wall_through_a_cell_is_cut_to_what_the_cell_holds() {
    let mesh = crate::test_shapes::cylinder(10.0, 0.0, 30.0, 256);
    let inside = inside_of(&mesh);
    let h = 1.0;
    let mut checked = 0usize;
    // A ring of cells round the barrel, at a height well clear of the ends.
    for step in 0..24 {
        let a = step as f64 * std::f64::consts::TAU / 24.0;
        let at = Vec3::new(10.0 * a.cos(), 10.0 * a.sin(), 15.0);
        let corners = cell(at, h);
        let tets = cut_cell(&inside, &corners);
        let cut = total(&tets);
        let want = sampled_fill(&inside, &corners, 40) * h * h * h;
        assert!(cut <= want + 1e-9, "at {at:?} the cut holds {cut} mm3, more than the {want} mm3 the cell holds");
        assert!(want - cut < 0.05 * h * h * h, "at {at:?} the cut holds {cut} mm3 where sampling says {want} mm3");
        for t in &tets {
            assert!(crate::elastic::tet_volume(t).is_some_and(|v| v > 0.0), "a tetrahedron is wound the wrong way");
        }
        checked += 1;
    }
    assert_eq!(checked, 24);
}

/// Over a whole cylinder the cut cells add up to the cylinder, and converge on it.
///
/// A fixed tolerance here would say almost nothing: the cut is always a little short, because
/// flat faces chord a round barrel, so any single number is really a statement about the
/// spacing it was measured at. What matters is that the shortfall goes away as the cells get
/// smaller, and at the rate a chord error does -- halve the spacing and a sagitta goes to a
/// quarter. So two spacings are run and the ratio is the assertion.
#[test]
fn the_cut_cells_of_a_cylinder_converge_on_the_cylinder() {
    let (radius, z0, z1) = (10.0, 0.0, 20.0);
    // Finely tessellated, so what is measured is the cut's chords and not the mesh's.
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 1024);
    let inside = inside_of(&mesh);
    let exact = std::f64::consts::PI * radius * radius * (z1 - z0);

    let short_at = |h: f64| {
        let mut volume = 0.0;
        let n = (2.0 * radius / h).ceil() as i64 + 2;
        let nz = ((z1 - z0) / h).ceil() as i64 + 2;
        for k in -1..nz {
            for j in -n / 2 - 1..n / 2 + 1 {
                for i in -n / 2 - 1..n / 2 + 1 {
                    let at = Vec3::new(i as f64 * h + h / 2.0, j as f64 * h + h / 2.0, z0 + k as f64 * h + h / 2.0);
                    volume += total(&cut_cell(&inside, &cell(at, h)));
                }
            }
        }
        (exact - volume) / exact
    };

    let (coarse, fine) = (short_at(2.0), short_at(1.0));
    assert!(coarse > 0.0 && fine > 0.0, "the cut should be short, not over: {coarse}, {fine}");
    assert!(coarse < 0.02, "even at 2 mm cells it should be within 2%: {:.3}%", coarse * 100.0);
    // Second order: halving the spacing should take the shortfall to about a quarter. Anything
    // from a third upwards would mean the faces are not following the surface as they should.
    let ratio = coarse / fine;
    assert!(ratio > 3.0, "halving the cells should take the shortfall to about a quarter, not {ratio:.2}x");
}

// ---------------------------------------------------------------------------------------------
// The mesh those tetrahedra make between them.

/// A box square to the lattice gives back the lattice: no crossings, and exactly its own volume.
///
/// Every corner of every cell is inside the model or outside it and the surface crosses no edge
/// anywhere, so the cut has nothing to cut and should produce the plain Kuhn decomposition of the
/// cells the box fills. Anything else would mean the cut is inventing points where there is no
/// surface to put them on.
#[test]
fn a_box_on_the_lattice_cuts_into_the_lattice() {
    let block = block_at([0., 0., 0.], [20., 20., 20.]);
    let inside = inside_of(&block);
    let grid = crate::grid::Grid3::over(block.bounds(), 8, 1).expect("a lattice");
    let mesh = cut_mesh(&grid, &inside);

    // No crossings at all. The box's faces land on node planes, so every edge leaving one of
    // those nodes crosses the surface at the node itself -- and a crossing that lands on a corner
    // is taken as that corner rather than made into a second point beside it.
    assert_eq!(mesh.crossings(), 0, "a box on the lattice needs no crossings of its own");
    let v = mesh.volume();
    // And so, exactly its own volume: there is nothing to approximate.
    assert!((v - 8000.0).abs() < 1e-9, "the cut holds {v} mm3 of a 8000 mm3 box");
    // Six tetrahedra for each cell the box fills, and none anywhere else.
    assert_eq!(mesh.tets().len(), 6 * 8 * 8 * 8);
}

/// Every crossing is one point, shared by every tetrahedron that reaches it.
///
/// This is the property the whole keying scheme exists for. If two cells either side of a face
/// each made their own crossing on the diagonal they share, the mesh would have two points at the
/// same place, the elements would move independently, and it would tear along exactly the surface
/// it was built to follow. Two points at the same place is therefore the thing to look for, and
/// there should be none.
#[test]
fn no_two_points_of_the_cut_sit_in_the_same_place() {
    let mesh_in = crate::test_shapes::cylinder(10.0, 0.0, 20.0, 256);
    let inside = inside_of(&mesh_in);
    let grid = crate::grid::Grid3::over(mesh_in.bounds(), 16, 1).expect("a lattice");
    let cut = cut_mesh(&grid, &inside);
    assert!(cut.crossings() > 200, "only {} crossings, so this proved little", cut.crossings());

    // Sorted on a fine key, so that coincident points land next to each other.
    let mut keys: Vec<(i64, i64, i64)> =
        cut.points().iter().map(|p| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64)).collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    assert_eq!(before, keys.len(), "{} of {before} points are duplicates of another", before - keys.len());
}

/// Every tetrahedron is wound the right way and has volume, and the lattice nodes it names are
/// the ones the lattice has.
#[test]
fn the_cut_mesh_is_made_of_sound_tetrahedra() {
    let mesh_in = crate::test_shapes::cylinder(8.0, 0.0, 16.0, 128);
    let inside = inside_of(&mesh_in);
    let grid = crate::grid::Grid3::over(mesh_in.bounds(), 12, 1).expect("a lattice");
    let cut = cut_mesh(&grid, &inside);

    let count = grid.count();
    let nodes = (count[0] + 1) * (count[1] + 1) * (count[2] + 1);
    for (i, t) in cut.tets().iter().enumerate() {
        let at = cut.tet_at(i).expect("a tetrahedron");
        assert!(crate::elastic::tet_volume(&at).is_some_and(|v| v > 0.0), "tetrahedron {i} has no volume or is inside out");
        for p in t {
            assert!((*p as usize) < cut.points().len(), "tetrahedron {i} names a point that is not there");
            if let Some(node) = cut.lattice_node(*p as usize) {
                assert!((node as usize) < nodes, "point {p} claims lattice node {node}, of {nodes}");
            }
        }
    }
    assert!(!cut.tets().is_empty());
}

/// The mesh holds the model, and holds it better as the cells get smaller.
///
/// The same convergence the cell-by-cell cut has, now with the points shared: if the sharing
/// dropped or doubled anything, the volume would stop converging on the cylinder and this is
/// where it would show.
#[test]
fn the_cut_mesh_converges_on_the_model() {
    let (radius, z0, z1) = (10.0, 0.0, 20.0);
    let mesh_in = crate::test_shapes::cylinder(radius, z0, z1, 1024);
    let inside = inside_of(&mesh_in);
    let exact = std::f64::consts::PI * radius * radius * (z1 - z0);

    let short_at = |span: usize| {
        let grid = crate::grid::Grid3::over(mesh_in.bounds(), span, 1).expect("a lattice");
        (exact - cut_mesh(&grid, &inside).volume()) / exact
    };
    let (coarse, fine) = (short_at(10), short_at(20));
    assert!(coarse > 0.0 && fine > 0.0, "the cut should be short of the model, not over: {coarse}, {fine}");
    assert!(coarse < 0.02, "at ten cells across it should be within 2%: {:.3}%", coarse * 100.0);
    let ratio = coarse / fine;
    assert!(ratio > 3.0, "halving the cells should take the shortfall to about a quarter, not {ratio:.2}x");
}

// ---------------------------------------------------------------------------------------------
// Solving on the cut mesh.

/// A bar pulled along its axis carries the stress it is pulled with, everywhere.
///
/// The answer is uniform, so nothing about the shape of the elements should show in it: a
/// tetrahedron reproduces a uniform strain exactly whatever shape it is, and the mesh around a
/// box square to the lattice is the plain Kuhn decomposition anyway. Anything other than the
/// traction here would mean the assembly, the supports or the loading is wrong, before any
/// curved surface is involved at all.
#[test]
fn a_bar_cut_and_pulled_carries_exactly_the_stress_it_is_pulled_with() {
    let bar = block_at([0., 0., 0.], [40., 10., 10.]);
    let inside = inside_of(&bar);
    let grid = crate::grid::Grid3::over(bar.bounds(), 16, 1).expect("a lattice");
    let mesh = cut_mesh(&grid, &inside);
    let m = crate::elastic::ElasticMaterial { yield_strength_pa: None, ..steel() };

    let traction = 10.0e6;
    let worst = worst_pulled(&mesh, &m, 0, traction, false, 1e-10, 20_000).expect("a solve");
    assert!((worst - traction).abs() / traction < 1e-6, "a bar pulled at {traction} Pa reads {worst} Pa");
}

/// The hole, solved on cut cells, against CalculiX on a mesh that follows it.
///
/// This is what the whole of `cut` was built for, and it does not do what it was built to do.
/// It reads **3.741** where the staircase reads 3.591 and a conforming mesh of the same
/// first-order elements reads 3.242 -- worse, not better, for all that the boundary now lies on
/// the hole. See docs/analysis-verification.md: the geometry is right, the conditioning is right
/// and the solver is right, and the element is what is left.
///
/// So what is asserted is only that it solves and that the answer is a concentration rather than
/// nonsense. Pinning 3.741 would pin a disappointment.
#[test]
fn the_hole_on_cut_cells_solves_and_reads_a_concentration() {
    // The geometry exactly, as a predicate: `cut_mesh` asks nothing else of it, so there is no
    // tessellation between the cut and the shape it is cutting.
    let (w, d, t, r) = (100.0f64, 100.0f64, 6.0f64, 10.0f64);
    let inside = |p: Vec3| p.x >= 0.0 && p.x <= w && p.y >= 0.0 && p.y <= d && p.z >= 0.0 && p.z <= t && (p.x - w / 2.0).hypot(p.y - d / 2.0) >= r;
    let bounds = solidcraft_geom::Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(w, d, t) };
    let grid = crate::grid::Grid3::over(bounds, 80, 0).expect("a lattice");
    let mesh = cut_mesh(&grid, &inside);
    let m = crate::elastic::ElasticMaterial { yield_strength_pa: None, ..steel() };

    let remote = 10.0e6;
    let worst = worst_pulled(&mesh, &m, 0, remote, true, 1e-9, 60_000).expect("a solve");
    let k = worst / remote;
    println!("  cut cells: {:.3} MPa, K = {k:.3}  (staircase 3.591, CalculiX 3.242)", worst / 1e6);
    assert!(k > 2.5, "a hole in a pulled plate concentrates the stress, it does not relieve it: {k:.3}");
    assert!(k < 5.0, "and it is a concentration, not a singularity: {k:.3}");
}

/// No tetrahedron of the cut is thin enough to wreck the solve.
///
/// This is a guard on a bug that cost an afternoon. A tetrahedron's stiffness is `V·BᵀDB` and
/// `B` goes as one over its thickness, so the stiffness goes as *one over* the thickness: a
/// sliver does not fade out of the problem, it grows without limit. The cut left seven of them
/// in a quarter of a million, the thinnest 7e-20 of a cell, and one of those put 2e25 on the
/// diagonal against a median of 1e9 -- the conjugate gradient then stalled at a residual of
/// 0.009 after sixty thousand iterations, on a mesh that was geometrically perfectly good.
///
/// The spread of the diagonal is the thing to watch, because that is what the solver divides by.
#[test]
fn the_cut_leaves_nothing_thin_enough_to_wreck_the_solve() {
    let (w, d, t, r) = (100.0f64, 100.0f64, 6.0f64, 10.0f64);
    let inside = |p: Vec3| p.x >= 0.0 && p.x <= w && p.y >= 0.0 && p.y <= d && p.z >= 0.0 && p.z <= t && (p.x - w / 2.0).hypot(p.y - d / 2.0) >= r;
    let bounds = solidcraft_geom::Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(w, d, t) };
    let grid = crate::grid::Grid3::over(bounds, 80, 0).expect("a lattice");
    let h = grid.spacing();
    let mesh = cut_mesh(&grid, &inside);

    let cell = h * h * h;
    let thinnest =
        (0..mesh.tets().len()).filter_map(|i| mesh.tet_at(i).and_then(|at| crate::elastic::tet_volume(&at))).fold(f64::INFINITY, f64::min) / cell;
    assert!(thinnest > 1e-7, "the thinnest tetrahedron is {thinnest:.3e} of a cell");

    let m = crate::elastic::ElasticMaterial { yield_strength_pa: None, ..steel() };
    let op = CutOperator::new(&mesh, &m, vec![false; mesh.points().len() * 3]);
    let diag = crate::linalg::Symmetric::diagonal(&op);
    let (mut low, mut high) = (f64::INFINITY, 0.0f64);
    for v in diag {
        low = low.min(*v);
        high = high.max(*v);
    }
    assert!(high / low < 1e4, "the diagonal spans {:.3e}, which no Jacobi preconditioner survives", high / low);
}

/// Does clustering the crossings onto nearby nodes buy anything?
///
/// The diagnosis after the cut cells read worse than the staircase was that what is left is the
/// shape of the elements, not the shape of the boundary. If that is right, the standard remedy
/// should show: a crossing that lands a hair from a lattice node leaves a splinter between the
/// two, and taking it as the node removes the splinter at the cost of moving the boundary by up
/// to that much. This measures both halves -- what it does to the worst element, and what it
/// does to the answer.
#[test]
#[ignore = "a sweep, minutes per point; run it by name when the question comes up again"]
fn what_clustering_the_crossings_buys() {
    let (w, d, t, r) = (100.0f64, 100.0f64, 6.0f64, 10.0f64);
    let inside = |p: Vec3| p.x >= 0.0 && p.x <= w && p.y >= 0.0 && p.y <= d && p.z >= 0.0 && p.z <= t && (p.x - w / 2.0).hypot(p.y - d / 2.0) >= r;
    let bounds = solidcraft_geom::Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(w, d, t) };
    let m = crate::elastic::ElasticMaterial { yield_strength_pa: None, ..steel() };
    let remote = 10.0e6;

    println!("  cluster   tets    thinnest/cell   worst K   (staircase 3.591, CalculiX 3.242)");
    for cluster in [1e-6, 0.05, 0.15, 0.3] {
        let grid = crate::grid::Grid3::over(bounds, 80, 0).expect("a lattice");
        let h = grid.spacing();
        let mesh = cut_mesh_clustering(&grid, &inside, cluster);
        let thinnest =
            (0..mesh.tets().len()).filter_map(|i| mesh.tet_at(i).and_then(|at| crate::elastic::tet_volume(&at))).fold(f64::INFINITY, f64::min)
                / (h * h * h);
        match worst_pulled(&mesh, &m, 0, remote, true, 1e-9, 60_000) {
            Ok(worst) => println!("  {cluster:<9.3} {:<7} {thinnest:<15.3e} {:.3}", mesh.tets().len(), worst / remote),
            Err(e) => println!("  {cluster:<9.3} {:<7} {thinnest:<15.3e} did not solve: {e}", mesh.tets().len()),
        }
    }
}

/// Does cutting the cells stop the hole diverging?
///
/// This is the question the whole of `cut` is finally for, and it is not the one the single
/// measurement answered. Every lattice setting measured on this hole *diverges* -- the peak
/// climbs as the cells shrink rather than settling, because the solved domain keeps re-entrant
/// corners wherever the half-full rule changes its mind, and a re-entrant corner has no finite
/// stress to converge to. A cut mesh has no such corners anywhere: its boundary is the surface.
///
/// So the absolute figure at one spacing (3.741, worse than the staircase) is the less
/// interesting half. If the cut converges while everything else runs away, then what is left is
/// an element that is merely inaccurate rather than a domain that is wrong, and refinement
/// reaches the answer instead of leaving it behind.
#[test]
#[ignore = "minutes a point; run it by name when the question comes up again"]
fn whether_cutting_the_cells_stops_the_hole_diverging() {
    let (w, d, t, r) = (100.0f64, 100.0f64, 6.0f64, 10.0f64);
    let inside = |p: Vec3| p.x >= 0.0 && p.x <= w && p.y >= 0.0 && p.y <= d && p.z >= 0.0 && p.z <= t && (p.x - w / 2.0).hypot(p.y - d / 2.0) >= r;
    let bounds = solidcraft_geom::Aabb3 { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(w, d, t) };
    let m = crate::elastic::ElasticMaterial { yield_strength_pa: None, ..steel() };
    let remote = 10.0e6;

    println!("  cluster  cells  spacing   worst element   averaged to points   (CalculiX 3.242)");
    for cluster in [CLUSTER, 0.15] {
        for span in [42usize, 58, 80] {
            let grid = crate::grid::Grid3::over(bounds, span, 0).expect("a lattice");
            let h = grid.spacing();
            let mesh = cut_mesh_clustering(&grid, &inside, cluster);
            let thinnest =
                (0..mesh.tets().len()).filter_map(|i| mesh.tet_at(i).and_then(|at| crate::elastic::tet_volume(&at))).fold(f64::INFINITY, f64::min)
                    / (h * h * h);
            let _ = thinnest;
            match worst_pulled_either_way(&mesh, &m, 0, remote, true, 1e-9, 60_000) {
                Ok((worst, smoothed)) => {
                    println!("  {cluster:<8.3} {span:<6} {h:<9.3} {:<15.3} {:.3}", worst / remote, smoothed / remote)
                }
                Err(e) => println!("  {cluster:<8.3} {span:<6} {h:<9.3} did not solve: {e}"),
            }
        }
    }
}
