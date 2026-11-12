//! Pulling boundary nodes onto the surface, checked on a shape the lattice cannot hold and one
//! it holds exactly.

use solidcore_record::Vec3;

use super::*;
use crate::grid::Grid3;
use crate::test_shapes::block_at;

/// A box whose walls land on cell boundaries has nothing to pull, and comes back untouched.
///
/// This is the one that keeps the rest of the solver safe. Every answer the elasticity suite
/// holds to *equality* is on a part square to the axes, and a snap that nudged even one node of
/// one of them would move all of those answers. So the pass has to be able to tell that there is
/// nothing to do, and say so.
#[test]
fn a_box_square_to_the_lattice_is_left_exactly_where_it_is() {
    let block = block_at([0., 0., 0.], [100., 20., 20.]);
    let grid = Grid3::over(block.bounds(), 20, 1).expect("a lattice");
    let occ = Occupancy::of(grid, &[&block], 4).expect("the block on the lattice");
    let nodes = Nodes::of(&occ);
    let snapped = snap(&occ, &nodes, &[&block]);

    assert!(snapped.is_lattice(), "{} nodes moved on a box that lands on the lattice", snapped.moved());
    assert_eq!(snapped.gave_up(), 0);
    // And every node is still exactly where the lattice put it, to the bit.
    for eq in 0..nodes.count() {
        let at = snapped.node(eq).expect("a node");
        let p = nodes.position(eq).expect("a lattice position");
        let h = occ.grid().spacing();
        let want = occ.grid().origin() + Vec3::new(p[0] as f64 * h, p[1] as f64 * h, p[2] as f64 * h);
        assert_eq!(at, want, "node {eq} moved");
    }
}

/// A cylinder's wall is where the staircase is, and the nodes on it end up on the cylinder.
///
/// The measurement is how far the boundary nodes are from the surface they are meant to be on.
/// On the lattice they are scattered through the cell the wall crosses. Pulled, they are a good
/// deal nearer it -- but not on it, and deliberately not: a move that would leave an element
/// lopsided is backed off part of the way (see `MAX_UNEVEN`), because an element stretched at
/// one corner and pinched at the opposite reads a stress it does not carry. Geometry is given up
/// for element quality, and the stress measured either way says that is the right way round.
#[test]
fn a_cylinders_boundary_nodes_end_up_on_the_cylinder() {
    let radius = 10.0;
    let (z0, z1) = (0.0, 30.0);
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 256);
    let grid = Grid3::over(mesh.bounds(), 32, 0).expect("a lattice over the cylinder");
    let occ = Occupancy::of(grid, &[&mesh], 4).expect("the cylinder on the lattice");
    let nodes = Nodes::of(&occ);
    let h = occ.grid().spacing();
    let axis = {
        let b = mesh.bounds();
        Vec3::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0, 0.0)
    };

    let snapped = snap(&occ, &nodes, &[&mesh]);
    assert!(snapped.moved() > 100, "only {} nodes moved, so this proved little", snapped.moved());

    // How far off the barrel a node is, for the nodes on the barrel rather than near an end.
    let off = |p: Vec3| ((p.x - axis.x).hypot(p.y - axis.y) - radius).abs();
    let (mut was, mut now, mut count, mut worse) = (0.0f64, 0.0f64, 0usize, 0usize);
    for eq in 0..nodes.count() {
        let Some(at) = snapped.node(eq).filter(|p| p.is_finite()) else { continue };
        let Some(ijk) = nodes.position(eq) else { continue };
        let home = occ.grid().origin() + Vec3::new(ijk[0] as f64 * h, ijk[1] as f64 * h, ijk[2] as f64 * h);
        // Away from the flat ends, where a node belongs on a cap and not on the barrel.
        if home.z < z0 + 2.0 * h || home.z > z1 - 2.0 * h {
            continue;
        }
        if at.dist(home) <= 1e-12 {
            continue;
        }
        let (before, after) = (off(home), off(at));
        if after > before + 1e-9 {
            worse += 1;
        }
        was += before;
        now += after;
        count += 1;
    }
    assert!(count > 50, "only {count} barrel nodes moved");
    let (was, now) = (was / count as f64, now / count as f64);
    assert!(now < was / 2.0, "off the barrel: {was:.4} mm before, {now:.4} mm after -- it should be a good deal nearer");
    // And none is left further off than the lattice had it: a move that is backed off is
    // shortened along the line from where it was to the surface, never pushed past it.
    assert_eq!(worse, 0, "{worse} nodes ended further from the barrel than they started");
}

/// Nothing is pulled further than it is allowed, and nothing folds.
///
/// A node that moved more than [`MAX_SNAP`] of a cell would be one the limit failed to hold; an
/// element whose map turns inside out anywhere in it is one no integral over it means anything
/// for. Both are checked over every solid cell, not sampled.
#[test]
fn no_node_moves_too_far_and_no_element_folds() {
    let mesh = crate::test_shapes::cylinder(8.0, 0.0, 20.0, 128);
    let grid = Grid3::over(mesh.bounds(), 24, 0).expect("a lattice");
    let occ = Occupancy::of(grid, &[&mesh], 4).expect("the cylinder on the lattice");
    let nodes = Nodes::of(&occ);
    let h = occ.grid().spacing();
    let snapped = snap(&occ, &nodes, &[&mesh]);

    for eq in 0..nodes.count() {
        let Some(at) = snapped.node(eq).filter(|p| p.is_finite()) else { continue };
        let Some(ijk) = nodes.position(eq) else { continue };
        let home = occ.grid().origin() + Vec3::new(ijk[0] as f64 * h, ijk[1] as f64 * h, ijk[2] as f64 * h);
        let d = at.dist(home);
        assert!(d <= MAX_SNAP * h + 1e-9, "node {eq} moved {d} mm, past the {} mm it is allowed", MAX_SNAP * h);
    }

    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let Some(corners) = snapped.cell(&occ, &nodes, cell) else { continue };
        for at in std::iter::once([0.0; 3]).chain(CORNERS) {
            let det = crate::elastic::jacobian_det(at, &corners);
            assert!(det.is_some_and(|d| d > 0.0), "cell {cell} has folded at {at:?}: {det:?}");
        }
    }
}
