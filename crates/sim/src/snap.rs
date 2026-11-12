//! Pulling the lattice's boundary nodes onto the model's own surface.
//!
//! A lattice holds a curved wall as a staircase, and a staircase's inside corners are re-entrant:
//! they have no finite stress, so the peak a study reports there climbs without limit as the
//! lattice is refined. Six ways of reading that answer more carefully are written up in
//! `docs/analysis-verification.md` as not working, and none of them could -- the surface stress
//! is not in the answer to be recovered.
//!
//! What is left is to stop having the corners. The cells on the wall keep their topology and give
//! up being cubes: each node that sits on the boundary is moved to the nearest point of the
//! model, so the elements' faces follow the surface instead of stepping across it.
//!
//! Two things keep that safe. A node moves at most [`MAX_SNAP`] of a cell, so an element can be
//! distorted but not turned inside out by a surface that happens to pass close to a node. And
//! every element a moved node touches is checked afterwards, at its corners and its middle; where
//! one has folded, the moves that folded it are backed off and checked again, and a node that
//! cannot be placed without folding something goes back to where the lattice put it.
//!
//! This is geometry only. It works out where the nodes should be and says how far it got; nothing
//! here solves anything, and [`solidcraft_sim::elastic`](crate::elastic) is where the result is
//! put to use.

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::compare::TriangleTree;
use solidcraft_geom::prelude::*;

use crate::elastic::{CORNERS, Nodes, jacobian_det};
use crate::grid::Occupancy;

/// How far a node may be pulled, as a fraction of the cell spacing.
///
/// A staircase corner can be as much as `h·√2/2 ≈ 0.707` of a cell from the surface it stands
/// for, so a cap much under that refuses exactly the nodes most worth moving -- the ones at the
/// corners of the steps. Six tenths was measured against four and five tenths and against three
/// quarters, on a round bar pulled along its axis and on a plate with a hole in it, and is the
/// best of them for both.
///
/// It is only usable at all because of [`MAX_UNEVEN`]. Raised to three quarters on its own, with
/// nothing but the fold guard, this made the bar *worse* than not fitting at all: the elements it
/// let through were not folded, merely lopsided, and a lopsided element reads a stress it does
/// not carry.
pub const MAX_SNAP: f64 = 0.6;

/// How uneven an element's map may be across itself: the largest determinant over the smallest,
/// at its corners and its middle.
///
/// Folding is not the only way a snap can spoil an element. One that is merely *lopsided* --
/// stretched at one corner and pinched at the opposite one -- still maps the right way out
/// everywhere, so the fold guard passes it, and it still reads a stress far above the one it
/// carries. A cube is 1.0; this allows a good deal of distortion and refuses the rest.
const MAX_UNEVEN: f64 = 2.5;

/// How many times a move is halved before the node is given up on.
const BACK_OFF: usize = 4;

/// Where the lattice's nodes actually are, once the ones on the wall have been pulled onto the
/// model.
#[derive(Clone, Debug)]
pub struct Snapped {
    at: Vec<Vec3>,
    moved: usize,
    gave_up: usize,
}

impl Snapped {
    /// Where a node equation's node sits, in the document's millimetres.
    pub fn node(&self, eq: usize) -> Option<Vec3> {
        self.at.get(eq).copied()
    }

    /// The eight corners of a cell, in [`CORNERS`] order, ready for an element.
    pub fn cell(&self, occ: &Occupancy, nodes: &Nodes, cell: usize) -> Option<[Vec3; 8]> {
        let eqs = nodes.of_cell(occ, cell)?;
        let mut out = [Vec3::new(0.0, 0.0, 0.0); 8];
        for (slot, eq) in out.iter_mut().zip(eqs) {
            *slot = self.node(eq)?;
        }
        Some(out)
    }

    /// How many nodes were pulled onto the surface.
    pub fn moved(&self) -> usize {
        self.moved
    }

    /// How many were left where the lattice put them because no amount of backing off placed
    /// them without folding an element.
    pub fn gave_up(&self) -> usize {
        self.gave_up
    }

    /// Whether anything moved at all. A part whose every wall lands on a cell boundary -- a box
    /// square to the axes -- has nothing to pull and comes back untouched, which is what keeps
    /// every case that is exact today exact.
    pub fn is_lattice(&self) -> bool {
        self.moved == 0
    }
}

/// Where each node of the lattice belongs: on the model's surface for the ones on the wall, and
/// where the lattice put it for the rest.
/// Where each node of the lattice belongs, leaving `pinned` ones where they are.
///
/// A node is pinned when something else has already been worked out from where it is -- the area
/// a load acts over, most of all, which a study still takes from the lattice's own faces. Moving
/// such a node would leave the load and the element disagreeing about the same piece of
/// surface. An empty slice pins nothing.
pub fn snap_keeping(occ: &Occupancy, nodes: &Nodes, meshes: &[&Mesh], pinned: &[bool]) -> Snapped {
    snap_inner(occ, nodes, meshes, pinned)
}

/// Where each node of the lattice belongs, with nothing held back.
pub fn snap(occ: &Occupancy, nodes: &Nodes, meshes: &[&Mesh]) -> Snapped {
    snap_inner(occ, nodes, meshes, &[])
}

fn snap_inner(occ: &Occupancy, nodes: &Nodes, meshes: &[&Mesh], pinned: &[bool]) -> Snapped {
    let grid = occ.grid();
    let h = grid.spacing();
    let mut at: Vec<Vec3> = Vec::with_capacity(nodes.count());
    for eq in 0..nodes.count() {
        match nodes.position(eq).map(|p| lattice_point(occ, p)) {
            Some(Some(p)) => at.push(p),
            _ => at.push(Vec3::new(f64::NAN, f64::NAN, f64::NAN)),
        }
    }
    let home = at.clone();

    let trees: Vec<TriangleTree> = meshes.iter().map(|m| TriangleTree::new(m)).collect();
    if trees.is_empty() || h <= 0.0 {
        return Snapped { at, moved: 0, gave_up: 0 };
    }

    // Where each boundary node wants to go, and how far that is.
    let limit = MAX_SNAP * h;
    let mut wants: Vec<(usize, Vec3)> = Vec::new();
    for eq in 0..nodes.count() {
        let Some(p) = home.get(eq).copied().filter(|p| p.is_finite()) else { continue };
        if pinned.get(eq).copied().unwrap_or(false) || !on_the_wall(occ, nodes, eq) {
            continue;
        }
        let Some(to) = nearest(&trees, meshes, p, limit) else { continue };
        if to.dist(p) > 1e-12 {
            wants.push((eq, to));
        }
    }
    if wants.is_empty() {
        return Snapped { at, moved: 0, gave_up: 0 };
    }

    // Move them all, then back off whatever folded something.
    let mut fraction: Vec<f64> = vec![1.0; wants.len()];
    for (eq, to) in &wants {
        if let Some(slot) = at.get_mut(*eq) {
            *slot = *to;
        }
    }
    for _ in 0..BACK_OFF {
        let bad = folded(occ, nodes, &at);
        if bad.is_empty() {
            break;
        }
        let mut backed = false;
        for (i, (eq, to)) in wants.iter().enumerate() {
            if !bad.contains(eq) {
                continue;
            }
            let Some(f) = fraction.get_mut(i) else { continue };
            *f *= 0.5;
            let f = *f;
            if let (Some(slot), Some(from)) = (at.get_mut(*eq), home.get(*eq)) {
                *slot = *from + (*to - *from) * f;
            }
            backed = true;
        }
        if !backed {
            break;
        }
    }

    // Anything still folded goes home, however much it was wanted.
    let mut gave_up = 0usize;
    for _ in 0..BACK_OFF {
        let bad = folded(occ, nodes, &at);
        if bad.is_empty() {
            break;
        }
        for (i, (eq, _)) in wants.iter().enumerate() {
            if !bad.contains(eq) {
                continue;
            }
            if let (Some(slot), Some(from)) = (at.get_mut(*eq), home.get(*eq)) {
                if slot.dist(*from) > 1e-12 {
                    gave_up += 1;
                }
                *slot = *from;
            }
            if let Some(f) = fraction.get_mut(i) {
                *f = 0.0;
            }
        }
    }

    let moved = at.iter().zip(&home).filter(|(a, b)| a.is_finite() && b.is_finite() && a.dist(**b) > 1e-12).count();
    Snapped { at, moved, gave_up }
}

/// Where the lattice puts a node.
fn lattice_point(occ: &Occupancy, node: [usize; 3]) -> Option<Vec3> {
    let grid = occ.grid();
    let h = grid.spacing();
    Some(grid.origin() + Vec3::new(node[0] as f64 * h, node[1] as f64 * h, node[2] as f64 * h))
}

/// Whether a node has solid on one side of it and not on the other, which is what makes it a
/// node on the wall rather than one inside the material or out in the air.
fn on_the_wall(occ: &Occupancy, nodes: &Nodes, eq: usize) -> bool {
    let Some(node) = nodes.position(eq) else { return false };
    let grid = occ.grid();
    let (mut solid, mut empty) = (false, false);
    for corner in CORNERS {
        // The eight cells around a node are the ones whose index is the node's, less one along
        // each axis the corner points back along.
        let mut cell = [0usize; 3];
        let mut outside = false;
        for (a, slot) in cell.iter_mut().enumerate() {
            let back = corner.get(a).copied().unwrap_or(0.0) < 0.0;
            match (node.get(a).copied(), back) {
                // A node on the lattice's own edge has air on that side by definition.
                (Some(0), true) => outside = true,
                (Some(v), true) => *slot = v - 1,
                (Some(v), false) => *slot = v,
                (None, _) => return false,
            }
        }
        if outside {
            empty = true;
            continue;
        }
        match grid.at(cell[0], cell[1], cell[2]) {
            Some(c) if occ.is_solid(c) => solid = true,
            _ => empty = true,
        }
    }
    solid && empty
}

/// The nearest point of any of the meshes, within `limit`.
fn nearest(trees: &[TriangleTree], meshes: &[&Mesh], p: Vec3, limit: f64) -> Option<Vec3> {
    let mut best: Option<(f64, Vec3)> = None;
    for (tree, mesh) in trees.iter().zip(meshes) {
        let Some((d, tri)) = tree.nearest(p) else { continue };
        if d > limit {
            continue;
        }
        let Some(abc) = mesh.triangles.get(tri as usize) else { continue };
        let (Some(a), Some(b), Some(c)) =
            (mesh.positions.get(abc[0] as usize), mesh.positions.get(abc[1] as usize), mesh.positions.get(abc[2] as usize))
        else {
            continue;
        };
        let on = solidcraft_geom::compare::closest_on_triangle(p, *a, *b, *c);
        let d = on.dist(p);
        if d <= limit && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, on));
        }
    }
    best.map(|(_, p)| p)
}

/// The nodes of every solid cell a snap has spoilt: one whose map turns inside out somewhere in
/// it, or one so lopsided across itself that it would read a stress it does not carry. Checked
/// at the corners and the middle, which is where both show first.
fn folded(occ: &Occupancy, nodes: &Nodes, at: &[Vec3]) -> std::collections::HashSet<usize> {
    let mut bad = std::collections::HashSet::new();
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let Some(eqs) = nodes.of_cell(occ, cell) else { continue };
        let mut corners = [Vec3::new(0.0, 0.0, 0.0); 8];
        let mut ok = true;
        for (slot, eq) in corners.iter_mut().zip(eqs) {
            match at.get(eq) {
                Some(p) if p.is_finite() => *slot = *p,
                _ => ok = false,
            }
        }
        if !ok {
            continue;
        }
        let (mut low, mut high) = (f64::INFINITY, 0.0f64);
        let mut sound = true;
        for c in std::iter::once([0.0; 3]).chain(CORNERS) {
            match jacobian_det(c, &corners) {
                Some(d) if d > 0.0 => {
                    low = low.min(d);
                    high = high.max(d);
                }
                _ => {
                    sound = false;
                    break;
                }
            }
        }
        if sound && !(low > 0.0 && high / low <= MAX_UNEVEN) {
            sound = false;
        }
        if !sound {
            bad.extend(eqs);
        }
    }
    bad
}

#[cfg(test)]
#[path = "snap_tests.rs"]
mod tests;
