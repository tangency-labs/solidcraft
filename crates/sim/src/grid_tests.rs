use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;

use crate::SimError;
use crate::grid::{DEFAULT_SPAN, Grid3, MAX_CELLS, MAX_SPAN, Occupancy, flat_ends, misfit};
use crate::test_shapes::block_at;

fn bounds(min: [f64; 3], max: [f64; 3]) -> Aabb3 {
    Aabb3 { min: Vec3::new(min[0], min[1], min[2]), max: Vec3::new(max[0], max[1], max[2]) }
}

/// The lattice of every analytic test: a 100 by 20 by 20 block on 5 mm cells, so the block's
/// walls land exactly on cell boundaries and the solid is 20 by 4 by 4 cells with one cell of
/// room all round.
fn slab_lattice() -> Grid3 {
    let g = Grid3::over(bounds([0., 0., 0.], [100., 20., 20.]), 20, 1).expect("lattice");
    assert_eq!(g.count(), [22, 6, 6]);
    assert!((g.spacing() - 5.0).abs() < 1e-12);
    g
}

#[test]
fn a_lattice_is_cubic_and_holds_the_model_in_the_middle() {
    let g = slab_lattice();
    // Equal spacing along every axis, so a result does not depend on how the part is turned.
    assert!((g.spacing() - 5.0).abs() < 1e-12);
    // The model is centred: the padding is shared, not added to one end.
    assert!((g.origin().x + 5.0).abs() < 1e-12, "{:?}", g.origin());
    assert!((g.origin().y + 5.0).abs() < 1e-12, "{:?}", g.origin());
    let b = g.bounds();
    assert!((b.max.x - 105.0).abs() < 1e-12, "{b:?}");
    assert!((b.max.y - 25.0).abs() < 1e-12, "{b:?}");
}

#[test]
fn the_metric_quantities_follow_the_spacing() {
    let g = slab_lattice();
    // 5 mm cells: 5e-3 m on a side. Compared in proportion, since these are small numbers
    // and an absolute bound on them would be below what a double can tell apart.
    let close = |got: f64, want: f64| (got - want).abs() <= want.abs() * 1e-15;
    assert!(close(g.pitch_m(), 5e-3), "{}", g.pitch_m());
    assert!(close(g.face_area_m2(), 25e-6), "{}", g.face_area_m2());
    assert!(close(g.cell_volume_m3(), 125e-9), "{}", g.cell_volume_m3());
}

#[test]
fn an_index_and_a_cell_are_the_same_thing_read_two_ways() {
    let g = slab_lattice();
    for (i, j, k) in [(0, 0, 0), (1, 2, 3), (21, 5, 5), (7, 0, 4)] {
        let c = g.at(i, j, k).expect("cell");
        assert_eq!(g.ijk(c), Some([i, j, k]));
        let centre = g.centre(c).expect("centre");
        assert_eq!(g.cell_at(centre), Some(c), "a centre falls in its own cell");
    }
    assert_eq!(g.at(22, 0, 0), None, "off the lattice");
    assert_eq!(g.ijk(g.cells()), None);
    assert_eq!(g.cells(), 22 * 6 * 6);
}

#[test]
fn stepping_runs_out_at_the_edges() {
    let g = slab_lattice();
    let c = g.at(0, 0, 0).expect("cell");
    assert_eq!(g.prev(c, 0), None);
    assert_eq!(g.next(c, 0), g.at(1, 0, 0));
    let far = g.at(21, 5, 5).expect("cell");
    assert_eq!(g.next(far, 2), None);
    assert_eq!(g.prev(far, 1), g.at(21, 4, 5));
    assert_eq!(g.next(c, 3), None, "there is no fourth axis");
}

#[test]
fn a_lattice_nobody_could_hold_is_refused_rather_than_attempted() {
    let b = bounds([0., 0., 0.], [100., 100., 100.]);
    match Grid3::over(b, 4000, 0) {
        Err(SimError::Setup(m)) => assert!(m.contains("cells across"), "{m}"),
        other => panic!("{other:?}"),
    }
    // Inside the span limit but still more cells than an analysis may hold.
    match Grid3::over(b, MAX_SPAN, 0) {
        Err(SimError::TooLarge { cells, limit }) => {
            assert!(cells > MAX_CELLS as u64, "{cells}");
            assert_eq!(limit, MAX_CELLS as u64);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_model_with_no_extent_is_refused() {
    let flat = Aabb3 { min: Vec3::new(1., 2., 3.), max: Vec3::new(1., 2., 3.) };
    assert!(matches!(Grid3::over(flat, DEFAULT_SPAN, 1), Err(SimError::Domain(_))));
    let empty = Aabb3::EMPTY;
    assert!(matches!(Grid3::over(empty, DEFAULT_SPAN, 1), Err(SimError::Domain(_))));
    let wild = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(f64::NAN, 1., 1.) };
    assert!(Grid3::over(wild, DEFAULT_SPAN, 1).is_err());
}

#[test]
fn a_block_fills_exactly_the_cells_its_walls_enclose() {
    let mesh = block_at([0., 0., 0.], [100., 20., 20.]);
    let occ = Occupancy::of(slab_lattice(), &[&mesh], 1).expect("occupancy");
    // 20 by 4 by 4 cells of 5 mm, and nothing in the padding.
    assert_eq!(occ.equations(), 20 * 4 * 4);
    // Read off the fractions rather than counted whole, the volume is the block's own.
    assert!((occ.volume_mm3() - 100.0 * 20.0 * 20.0).abs() < 1e-6, "{}", occ.volume_mm3());
    let g = occ.grid().clone();
    assert!(occ.is_solid(g.at(1, 1, 1).expect("cell")));
    assert!(!occ.is_solid(g.at(0, 1, 1).expect("cell")), "the padding is outside");
    assert!(!occ.is_solid(g.at(21, 1, 1).expect("cell")));
    assert_eq!(occ.part_of(g.at(1, 1, 1).expect("cell")), Some(0));
    assert_eq!(occ.part_of(g.at(0, 1, 1).expect("cell")), None);
}

#[test]
fn the_boundary_is_the_blocks_own_surface() {
    let mesh = block_at([0., 0., 0.], [100., 20., 20.]);
    let occ = Occupancy::of(slab_lattice(), &[&mesh], 1).expect("occupancy");
    let faces: Vec<_> = occ.boundary().collect();
    // Two ends of 4 by 4, and four sides of 20 by 4.
    assert_eq!(faces.len(), 2 * (4 * 4) + 4 * (20 * 4));
    let area = faces.len() as f64 * occ.grid().face_area_m2();
    let truth = 2.0 * (0.02 * 0.02) + 4.0 * (0.1 * 0.02);
    assert!((area - truth).abs() < 1e-15, "{area} vs {truth}");
    // Every boundary face belongs to a solid cell and looks at something that is not solid.
    for f in &faces {
        assert!(occ.is_solid(f.cell));
        let across = if f.positive { occ.grid().next(f.cell, f.axis) } else { occ.grid().prev(f.cell, f.axis) };
        assert!(across.is_none_or(|n| !occ.is_solid(n)));
    }
}

#[test]
fn two_blocks_keep_their_own_cells() {
    let left = block_at([0., 0., 0.], [50., 20., 20.]);
    let right = block_at([50., 0., 0.], [50., 20., 20.]);
    let occ = Occupancy::of(slab_lattice(), &[&left, &right], 1).expect("occupancy");
    assert_eq!(occ.equations(), 20 * 4 * 4, "together they are the same slab");
    let g = occ.grid().clone();
    // The seam is at x = 50 mm, which is the boundary between cells 10 and 11.
    assert_eq!(occ.part_of(g.at(10, 1, 1).expect("cell")), Some(0));
    assert_eq!(occ.part_of(g.at(11, 1, 1).expect("cell")), Some(1));
    // and nothing is on the surface between them.
    let inner = occ.boundary().filter(|f| f.axis == 0 && !f.positive && g.ijk(f.cell).is_some_and(|c| c[0] == 11)).count();
    assert_eq!(inner, 0, "the two halves conduct into each other");
}

#[test]
fn sampling_more_finely_fills_a_wall_cell_by_the_fraction_it_covers() {
    // A block half a cell thick in x, so the cells on its wall are exactly half full.
    let mesh = block_at([0., 0., 0.], [2.5, 20., 20.]);
    let g = Grid3::over(bounds([0., 0., 0.], [100., 20., 20.]), 20, 1).expect("lattice");
    let occ = Occupancy::of(g, &[&mesh], 4).expect("occupancy");
    let grid = occ.grid().clone();
    let cell = grid.at(1, 2, 2).expect("cell");
    assert!((occ.fill_of(cell) - 0.5).abs() < 1e-6, "{}", occ.fill_of(cell));
    // Half full counts as solid, and the volume read off the fractions is the block's.
    assert!(occ.is_solid(cell));
    assert!((occ.volume_mm3() - 2.5 * 20.0 * 20.0).abs() < 1e-6, "{}", occ.volume_mm3());
}

#[test]
fn a_model_the_lattice_cannot_see_is_refused() {
    // One cell across a hundred-millimetre box: nothing's centre lands inside the thin part.
    let mesh = block_at([0., 0., 0.], [100., 0.01, 0.01]);
    let g = Grid3::over(bounds([0., 0., 0.], [100., 100., 100.]), 2, 0).expect("lattice");
    match Occupancy::of(g, &[&mesh], 1) {
        Err(SimError::Domain(m)) => assert!(m.contains("too coarse"), "{m}"),
        other => panic!("{other:?}"),
    }
    let g = Grid3::over(bounds([0., 0., 0.], [10., 10., 10.]), 4, 0).expect("lattice");
    assert!(matches!(Occupancy::of(g, &[], 1), Err(SimError::Domain(_))));
}

/// What the lattice makes of a shape nothing about lands on it.
///
/// A cylinder's volume, its curved side's area and the share each cell carries are the three
/// numbers every later correction is built on, so they are measured here against the exact
/// answers rather than against each other.
#[test]
fn a_cylinder_comes_out_the_volume_a_cylinder_has() {
    let radius = 10.0;
    let (z0, z1) = (0.0, 30.0);
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 256);
    let exact = std::f64::consts::PI * radius * radius * (z1 - z0);
    for span in [16usize, 32, 48] {
        let grid = Grid3::over(mesh.bounds(), span, 0).expect("a lattice over the cylinder");
        let occ = Occupancy::of(grid, &[&mesh], 4).expect("the cylinder on the lattice");
        let sampled = occ.volume_mm3();
        let carried = occ.solid_volume_m3() * 1e9;
        // Sampling the cells is the measurement; four samples a side over a 24-cell span is
        // already within a part in four hundred of the real cylinder.
        let error = (sampled - exact).abs() / exact;
        assert!(error < 0.005, "span {span}: the lattice says {sampled} mm3 against {exact}");
        // And what the solver carries is that same volume, not the staircase's.
        let carried_error = (carried - sampled).abs() / sampled;
        assert!(carried_error < 1e-6, "span {span}: the cells carry {carried} mm3 of {sampled}");
    }
}

/// The spacing is fitted to the flat faces the part really has, and to no others.
///
/// A lattice that does not divide the part overhangs it and the half-full rule turns the
/// overhang into material, which is worth up to 42 per cent of the answer on a bar whose answer
/// is exact (`fitted_span`, docs/analysis-verification.md). So a study fits the spacing. What it
/// must not do is fit an axis the part has nothing flat on: the nearest count that divides a
/// cylinder's diameter can be far from the one asked for, and it would buy a coarser lattice in
/// exchange for landing on a face that is not there.
#[test]
fn the_spacing_is_fitted_to_the_faces_the_part_really_has() {
    let bar = block_at([0.0, 0.0, 0.0], [100.0, 20.0, 20.0]);
    let rod = crate::test_shapes::cylinder(10.0, 0.0, 30.0, 256);
    assert_eq!(flat_ends(&[&bar], &bar.bounds()), [true; 3], "a box is flat on every axis");
    assert_eq!(flat_ends(&[&rod], &rod.bounds()), [false, false, true], "a rod is flat on its ends alone");

    // 48 cells over the bar's length would be 2.0833 mm, which divides 20 mm 9.6 times. The
    // fitted lattice moves to 2 mm, where it divides it ten times.
    let grid = Grid3::over_parts(&[&bar], 48, 0).expect("a lattice over the bar");
    let h = grid.spacing();
    assert_eq!(grid.count(), [50, 10, 10], "the bar is a whole number of cells on every axis");
    for extent in [100.0f64, 20.0, 20.0] {
        let whole = (extent / h).round();
        assert!((extent - whole * h).abs() < 1e-9, "{extent} mm is {} cells of {h} mm", extent / h);
    }

    // The rod's length is the longest side, so it divides whatever the count; across the
    // diameter there is nothing to land on, and the spacing asked for is the spacing given.
    let asked = Grid3::over(rod.bounds(), 16, 0).expect("a lattice over the rod");
    let fitted = Grid3::over_parts(&[&rod], 16, 0).expect("a fitted lattice over the rod");
    assert!(
        (fitted.spacing() - asked.spacing()).abs() < 1e-12,
        "the rod's spacing moved from {} to {} for a face it has not got",
        asked.spacing(),
        fitted.spacing()
    );

    // And a part with nothing flat anywhere keeps what it was asked for rather than wandering.
    let ball = crate::test_shapes::cylinder(10.0, 0.0, 20.0, 8);
    assert!(Grid3::over_parts(&[&ball], 7, 0).is_ok(), "a lattice over an eight-sided rod");
}

/// Where the fit gives up, the study is told how much too big the part is and what to ask for.
///
/// The fit moves the count a few per cent and no further, so a part whose extents have no common
/// measure near the count asked for keeps the lattice it was given and is solved too big. That
/// is the case this has to be loud about: the overhang is material, raising `cells` does not
/// reduce it, and a caller who is told nothing has no way to find out.
///
/// The case is the verification plate's block, 100 x 100 x 6, at 75 cells. A spacing that
/// divides 6 as well as 100 needs a count that is a multiple of 50, and the fit will only move
/// 75 by a quarter of itself, which reaches 57 to 93 and so reaches neither 50 nor 100. The
/// lattice stays at 1.333 mm and the 6 mm plate is solved five cells, 6.667 mm, a ninth over.
#[test]
fn a_lattice_that_cannot_be_landed_on_the_part_says_so_and_says_what_to_ask_for() {
    let plate = block_at([0.0, 0.0, 0.0], [100.0, 100.0, 6.0]);
    let asked = Grid3::over_parts(&[&plate], 75, 0).expect("a lattice over the plate");
    let h = asked.spacing();
    assert!((h - 100.0 / 75.0).abs() < 1e-12, "the fit should have given up and left 75 cells, and left {h} mm");

    let told = misfit(&[&plate], &asked, 75).expect("the study should be told the lattice misses");
    let [x, y, z] = told.oversize;
    assert_eq!((x, y), (None, None), "100 mm is 75 whole cells, so neither of those axes is out");
    let z = z.expect("the 6 mm thickness is not a whole number of 1.333 mm cells");
    assert!((z - 1.0 / 9.0).abs() < 1e-9, "the plate is solved 6.667 mm thick, a ninth over, and was told {z}");
    assert_eq!(told.would_fit, Some(100), "100 cells is 1 mm, which divides 6 and 100 alike");

    // And where the lattice does land, there is nothing to say.
    let lands = Grid3::over_parts(&[&plate], 48, 0).expect("a lattice over the plate");
    assert_eq!(misfit(&[&plate], &lands, 48), None, "48 cells is fitted, so nothing is out");
}

/// Where the material sits inside a cell, which is where a reading for that cell belongs.
///
/// Two things have to hold. A cell the model fills whole has its material centred, exactly, or
/// every case that is exact today would move. A cell a wall crosses has its material off to the
/// inboard side, and the centre has to say so -- the middle of such a cell can be outside the
/// model altogether, and a stress read there is extrapolation into the void rather than anything
/// the part does.
#[test]
fn the_material_in_a_cell_a_wall_crosses_is_off_to_the_side_the_model_is_on() {
    // A block whose walls land on cell boundaries: every cell it fills, it fills whole.
    let block = block_at([0., 0., 0.], [100., 20., 20.]);
    let occ = Occupancy::of(slab_lattice(), &[&block], 4).expect("the block on the lattice");
    for e in 0..occ.equations() {
        let cell = occ.cell_of(e).expect("a cell");
        let centre = occ.material_centre_local(cell);
        assert_eq!(centre, [0.0, 0.0, 0.0], "a cell the block fills whole is not centred: {centre:?}");
    }

    // And a cylinder, whose curved side crosses cells at every angle. Its axis is along z, so a
    // cut cell on the barrel has its material towards the axis: the centre, pushed out into
    // space, lands nearer the axis than the middle of the cell does.
    let radius = 10.0;
    let (z0, z1) = (0.0, 30.0);
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 256);
    let grid = Grid3::over(mesh.bounds(), 40, 0).expect("a lattice over the cylinder");
    let occ = Occupancy::of(grid, &[&mesh], 4).expect("the cylinder on the lattice");
    let h = occ.grid().spacing();
    let axis = Vec3::new((mesh.bounds().min.x + mesh.bounds().max.x) / 2.0, (mesh.bounds().min.y + mesh.bounds().max.y) / 2.0, 0.0);
    let mut cut = 0usize;
    for e in 0..occ.equations() {
        let cell = occ.cell_of(e).expect("a cell");
        let at = occ.grid().centre(cell).expect("a cell centre");
        // Away from the flat ends, where the cut is in z and says nothing about the barrel.
        if at.z < z0 + 2.0 * h || at.z > z1 - 2.0 * h {
            continue;
        }
        let local = occ.material_centre_local(cell);
        if occ.fill_of(cell) >= 1.0 {
            // Filled whole, so centred -- and that is the other half of the rule.
            assert_eq!(local, [0.0, 0.0, 0.0], "cell {cell} is filled whole and not centred: {local:?}");
            continue;
        }
        cut += 1;
        // The centre stays inside the cell, whatever the cut.
        for (a, v) in local.iter().enumerate() {
            assert!(v.abs() <= 1.0, "cell {cell}: the material centre leaves the cell on axis {a}: {v}");
        }
        // Moving to it takes the reading towards the axis, which is the side the material is on.
        let out = Vec3::new(at.x - axis.x, at.y - axis.y, 0.0);
        let along = out.x * local[0] + out.y * local[1];
        assert!(along <= 1e-9, "cell {cell} at {at:?}: the material centre {local:?} points away from the material");
    }
    assert!(cut > 200, "only {cut} cut cells on the barrel, so this proved little");
}

/// The cells a wall crosses carry less than a whole cell's worth, and the ones it misses carry
/// more than their own fraction -- because the material of the cells left out has to go
/// somewhere, and the nearest cell is where.
#[test]
fn the_share_a_cell_carries_adds_up_to_the_whole_part() {
    let mesh = crate::test_shapes::cylinder(8.0, 0.0, 20.0, 128);
    let grid = Grid3::over(mesh.bounds(), 32, 0).expect("a lattice");
    let occ = Occupancy::of(grid, &[&mesh], 4).expect("the cylinder on the lattice");
    let mut cut = 0usize;
    let mut whole = 0usize;
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        if occ.fill_of(cell) > 0.999 {
            whole += 1;
            // A cell the model fills has nothing taken off it, only the small share handed
            // back from the cells that were dropped.
            assert!(occ.share_of(cell) >= 1.0, "a full cell carries {}", occ.share_of(cell));
            assert!(occ.share_of(cell) < 1.1, "a full cell carries {}", occ.share_of(cell));
        } else {
            cut += 1;
            assert!(occ.share_of(cell) < 1.05, "a cut cell carries {}", occ.share_of(cell));
        }
    }
    assert!(cut > 100 && whole > 100, "{cut} cut cells and {whole} whole ones");
    assert_eq!(occ.equations(), cut + whole);
}
