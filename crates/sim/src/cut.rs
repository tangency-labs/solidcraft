//! Filling the material inside one cell with tetrahedra that stop at the model's surface.
//!
//! A lattice cell the wall crosses holds a piece of the part, and the two things done to such a
//! cell so far are both approximations of the same thing. Scaling its stiffness by how full it is
//! keeps the amount of material and loses its shape. Pulling its corners onto the surface
//! ([`crate::snap`]) keeps eight corners, so the element still stands a sliver past the model
//! wherever the wall is not flat. What is wanted is an element that *ends* at the wall, and a
//! brick cannot; a tetrahedron can.
//!
//! So the cell is cut. Its eight corners are each inside the model or not, the cube is split into
//! six tetrahedra ([`KUHN`]), and each of those is clipped against the surface by the signs at
//! its own four corners -- marching tetrahedra, with the crossing on each cut edge found by
//! bisection rather than by interpolating a field, since what is being cut against is a mesh and
//! not a function.
//!
//! This is geometry alone. It produces tetrahedra and says nothing about what to do with them;
//! [`crate::elastic::tet_stiffness`] is the element they are meant for.

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::elastic::tet_volume;

/// A cube split into six tetrahedra about the 0-6 diagonal, in [`crate::elastic::CORNERS`] order.
///
/// Six rather than five because every one of them shares that diagonal, so neighbouring cells cut
/// the same way agree on the faces between them -- five would need the split alternated cell by
/// cell to match up.
pub const KUHN: [[usize; 4]; 6] = [[0, 1, 2, 6], [0, 2, 3, 6], [0, 3, 7, 6], [0, 7, 4, 6], [0, 4, 5, 6], [0, 5, 1, 6]];

/// How many times an edge is halved to find where it crosses the surface.
///
/// Forty takes a millimetre edge to a thousandth of a nanometre, and the count is worth that
/// much: what is left of it goes straight into the volume. At twenty, a 20 mm box square to the
/// lattice came out 0.003 mm³ over -- small, but the box is a shape the rest of the solver gets
/// exactly right, and the residue was the bisection and nothing else. Halvings are cheap and
/// only happen on an edge the surface actually crosses.
///
/// Bisection rather than anything cleverer because the thing being crossed is a triangle mesh:
/// there is no field to interpolate, only an inside test to ask again.
const BISECT: usize = 40;

/// A tetrahedron below this share of the cell is dropped: a sliver that thin carries no material
/// worth the ill-conditioning it would bring.
const LEAST: f64 = 1e-9;

/// The material inside one cell, as tetrahedra.
///
/// `corners` are the cell's eight, in [`crate::elastic::CORNERS`] order. Every tetrahedron comes
/// back wound so its volume is positive. A cell wholly inside the model gives the six of
/// [`KUHN`]; one wholly outside gives none.
pub fn cut_cell(inside: &dyn Fn(Vec3) -> bool, corners: &[Vec3; 8]) -> Vec<[Vec3; 4]> {
    let mut sign = [false; 8];
    for (s, p) in sign.iter_mut().zip(corners) {
        *s = inside(*p);
    }
    let mut out = Vec::new();
    for which in KUHN {
        let at: [Vec3; 4] = std::array::from_fn(|i| corners[which[i]]);
        let s: [bool; 4] = std::array::from_fn(|i| sign[which[i]]);
        clip(inside, &at, &s, &mut out);
    }
    out
}

/// One tetrahedron clipped to the part of it inside the model.
fn clip(inside: &dyn Fn(Vec3) -> bool, at: &[Vec3; 4], s: &[bool; 4], out: &mut Vec<[Vec3; 4]>) {
    let ins: Vec<usize> = (0..4).filter(|i| s[*i]).collect();
    let outs: Vec<usize> = (0..4).filter(|i| !s[*i]).collect();
    let whole = tet_volume(at).unwrap_or(0.0);
    if whole <= 0.0 {
        return;
    }
    let keep = |out: &mut Vec<[Vec3; 4]>, t: [Vec3; 4]| {
        if let Some(v) = tet_volume(&t)
            && v > whole * LEAST
        {
            out.push(t);
        } else {
            // The other winding describes the same tetrahedron; take it if that is the one with
            // the volume, and drop anything too thin to be worth carrying either way.
            let flipped = [t[0], t[2], t[1], t[3]];
            if let Some(v) = tet_volume(&flipped)
                && v > whole * LEAST
            {
                out.push(flipped);
            }
        }
    };
    // Where the surface crosses the edge between an inside corner and an outside one.
    let cross = |a: usize, b: usize| between(inside, at[a], at[b]);

    match (ins.len(), outs.len()) {
        (0, _) => {}
        (4, _) => keep(out, *at),
        (1, _) => {
            let a = ins[0];
            keep(out, [at[a], cross(a, outs[0]), cross(a, outs[1]), cross(a, outs[2])]);
        }
        (3, _) => {
            // Everything but the corner tetrahedron at the one outside corner: a wedge between
            // the triangle of inside corners and the triangle of crossings.
            let d = outs[0];
            let (a, b, c) = (ins[0], ins[1], ins[2]);
            let (da, db, dc) = (cross(a, d), cross(b, d), cross(c, d));
            keep(out, [at[a], at[b], at[c], da]);
            keep(out, [at[b], at[c], da, db]);
            keep(out, [at[c], da, db, dc]);
        }
        (2, _) => {
            // A wedge with triangular ends (a, a-c, a-d) and (b, b-c, b-d).
            let (a, b) = (ins[0], ins[1]);
            let (c, d) = (outs[0], outs[1]);
            let p = [at[a], cross(a, c), cross(a, d), at[b], cross(b, c), cross(b, d)];
            keep(out, [p[0], p[1], p[2], p[3]]);
            keep(out, [p[1], p[2], p[3], p[4]]);
            keep(out, [p[2], p[3], p[4], p[5]]);
        }
        _ => {}
    }
}

/// Where the surface crosses the segment from an inside point to an outside one, by bisection.
fn between(inside: &dyn Fn(Vec3) -> bool, within: Vec3, without: Vec3) -> Vec3 {
    let (mut lo, mut hi) = (within, without);
    for _ in 0..BISECT {
        let mid = (lo + hi) * 0.5;
        if inside(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// An inside test for a mesh, built once and asked many times, counting a point *on* the surface
/// as inside it.
///
/// The second part is not a nicety. Whether a point is inside a closed mesh is settled by
/// counting how many times a ray from it crosses the surface, and for a point lying exactly on
/// that surface the count is whatever the ray happens to do at the triangle it grazes -- the
/// question has no answer there. It comes up constantly, because a part square to the axes has
/// whole faces landing on node planes: a 20 mm box on 2 mm cells lost 5.2 mm³ of 8000 to it,
/// 0.065% on a shape with no curvature at all, which should be exact to the bit.
///
/// Taking "within a whisker of the surface" as inside settles it one way. The crossing on an edge
/// through such a point is then the point itself, the tetrahedron it would have made has no
/// volume and is dropped, and the cell comes out whole. The whisker is scaled to the model, so it
/// means the same thing on a watch part as on a chassis.
pub fn inside_of(mesh: &Mesh) -> impl Fn(Vec3) -> bool + '_ {
    let index = mesh.inside_index();
    let tree = solidcraft_geom::compare::TriangleTree::new(mesh);
    let skin = mesh.bounds().diagonal().max(1.0) * 1e-9;
    move |p| index.contains(p) || tree.nearest(p).is_some_and(|(d, _)| d <= skin)
}

/// A mesh of tetrahedra filling the model, built on the lattice but stopping at the surface.
///
/// The points are of two kinds and the difference is the whole reason this type exists. Some are
/// the lattice's own nodes, which many cells share as they always did. The rest are where the
/// surface crosses an edge between two lattice nodes -- and such a crossing has to be *one* point
/// for every cell that shares that edge, or the elements either side of it move independently and
/// the mesh tears along the surface it was built to follow.
///
/// Keying a crossing by the pair of lattice nodes whose edge it lies on is what guarantees that.
/// Two cells meeting at a face both reach the crossing on that face's diagonal by the same pair,
/// so they find the same point without having to be told about each other. It works because every
/// cell is split the same way ([`KUHN`]): a decomposition chosen cell by cell would put a
/// diagonal in one cell where its neighbour had none.
#[derive(Clone, Debug, Default)]
pub struct CutMesh {
    points: Vec<Vec3>,
    tets: Vec<[u32; 4]>,
    /// For each point, the lattice node it is, or `None` for a crossing.
    lattice: Vec<Option<u32>>,
}

impl CutMesh {
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    pub fn tets(&self) -> &[[u32; 4]] {
        &self.tets
    }

    /// The lattice node a point is, or `None` where it is a crossing of its own.
    pub fn lattice_node(&self, point: usize) -> Option<u32> {
        self.lattice.get(point).copied().flatten()
    }

    /// How many of the points are crossings rather than lattice nodes.
    pub fn crossings(&self) -> usize {
        self.lattice.iter().filter(|l| l.is_none()).count()
    }

    /// The volume of every tetrahedron, which for a watertight cut is the model's own.
    pub fn volume(&self) -> f64 {
        self.tets
            .iter()
            .filter_map(|t| {
                let at: [Vec3; 4] = std::array::from_fn(|i| self.points.get(t[i] as usize).copied().unwrap_or_default());
                crate::elastic::tet_volume(&at)
            })
            .sum()
    }

    /// Take a tetrahedron, wound whichever way has the volume, and drop one too thin to be
    /// worth carrying.
    ///
    /// `least` is where too thin starts, and it is not a tidiness measure. A tetrahedron's
    /// stiffness goes as `V·BᵀDB`, and `B` goes as one over its thickness, so the stiffness goes
    /// as *one over* the thickness: a sliver does not fade away, it grows without limit. Left in,
    /// one 7e-20 of a cell put an entry of 2e25 on the diagonal against a median of 1e9, and the
    /// conjugate gradient stalled at a residual of 0.009 after sixty thousand iterations. The
    /// material such a sliver holds is nothing; the damage it does is not.
    fn push_tet(&mut self, p: [u32; 4], least: f64) {
        let corners = |p: &[u32; 4]| -> Option<[Vec3; 4]> {
            let mut out = [Vec3::new(0.0, 0.0, 0.0); 4];
            for (slot, i) in out.iter_mut().zip(p) {
                *slot = *self.points.get(*i as usize)?;
            }
            Some(out)
        };
        if corners(&p).and_then(|c| crate::elastic::tet_volume(&c)).is_some_and(|v| v > least) {
            self.tets.push(p);
            return;
        }
        let flipped = [p[0], p[2], p[1], p[3]];
        if corners(&flipped).and_then(|c| crate::elastic::tet_volume(&c)).is_some_and(|v| v > least) {
            self.tets.push(flipped);
        }
    }

    /// The four corners of a tetrahedron.
    pub fn tet_at(&self, i: usize) -> Option<[Vec3; 4]> {
        let t = self.tets.get(i)?;
        let mut out = [Vec3::new(0.0, 0.0, 0.0); 4];
        for (slot, p) in out.iter_mut().zip(t) {
            *slot = *self.points.get(*p as usize)?;
        }
        Some(out)
    }
}

/// Build the cut mesh over every cell the model touches.
///
/// Every cell with any material in it is cut, not only the ones the half-full rule kept: the
/// point of cutting is that a cell's share of the domain is the material it holds, so a cell
/// holding a tenth contributes a tenth and no decision has to be made about it.
pub fn cut_mesh(grid: &crate::grid::Grid3, inside: &dyn Fn(Vec3) -> bool) -> CutMesh {
    cut_mesh_clustering(grid, inside, CLUSTER)
}

/// How near a lattice node a crossing must be, as a share of the spacing, to be taken as that
/// node rather than a point of its own.
///
/// The default is just enough to catch a crossing the bisection put on top of a node and nothing
/// else. Larger values are the standard remedy for the shapes marching tetrahedra make -- a
/// crossing a hair from a corner leaves a splinter between them -- at the cost of moving the
/// boundary by up to that share of a cell. Whether that trade is worth taking is measured in
/// `cut_tests`, not assumed.
pub const CLUSTER: f64 = 1e-6;

/// The cut mesh, saying how near a node a crossing has to be to be taken as that node.
pub fn cut_mesh_clustering(grid: &crate::grid::Grid3, inside: &dyn Fn(Vec3) -> bool, cluster: f64) -> CutMesh {
    use std::collections::HashMap;
    let count = grid.count();
    let n = [count[0] + 1, count[1] + 1, count[2] + 1];
    let h = grid.spacing();
    let node_index = |ijk: [usize; 3]| (ijk[0] + ijk[1] * n[0] + ijk[2] * n[0] * n[1]) as u32;
    let node_at = |ijk: [usize; 3]| grid.origin() + Vec3::new(ijk[0] as f64 * h, ijk[1] as f64 * h, ijk[2] as f64 * h);

    // A crossing nearer than this to a corner is that corner. Small enough that nothing real
    // is moved, large enough to catch a crossing the bisection put on top of a node.
    let on_corner = h * cluster;
    // Thinner than this and a tetrahedron is all conditioning and no material. See `push_tet`.
    let least = h * h * h * LEAST;
    let mut out = CutMesh::default();
    // A lattice node, by its index on the node lattice; a crossing, by the pair of nodes whose
    // edge it lies on. Both go in the same table of points.
    let mut from_node: HashMap<u32, u32> = HashMap::new();
    let mut from_edge: HashMap<(u32, u32), u32> = HashMap::new();

    for k in 0..count[2] {
        for j in 0..count[1] {
            for i in 0..count[0] {
                // The cell's eight corners, as lattice indices and as points.
                let mut corner_node = [0u32; 8];
                let mut corner_at = [Vec3::new(0.0, 0.0, 0.0); 8];
                for (c, off) in crate::elastic::CORNERS.iter().enumerate() {
                    let ijk = [i + usize::from(off[0] > 0.0), j + usize::from(off[1] > 0.0), k + usize::from(off[2] > 0.0)];
                    corner_node[c] = node_index(ijk);
                    corner_at[c] = node_at(ijk);
                }
                let sign: [bool; 8] = std::array::from_fn(|c| inside(corner_at[c]));
                // Nothing of the model anywhere near this cell: skip it without cutting.
                if sign.iter().all(|s| !*s) && !crosses(inside, &corner_at) {
                    continue;
                }

                for which in KUHN {
                    let at: [Vec3; 4] = std::array::from_fn(|t| corner_at[which[t]]);
                    let s: [bool; 4] = std::array::from_fn(|t| sign[which[t]]);
                    let ins: Vec<usize> = (0..4).filter(|t| s[*t]).collect();
                    let outs: Vec<usize> = (0..4).filter(|t| !s[*t]).collect();
                    let node = |t: usize| corner_node[which[t]];

                    // The points of the cut. A corner is keyed by its lattice node, a crossing
                    // by the pair of nodes whose edge it lies on, so that every cell sharing
                    // that edge finds the one point rather than making its own.
                    let at_corner = |out: &mut CutMesh, seen: &mut HashMap<u32, u32>, a: usize| -> u32 {
                        let key = node(a);
                        *seen.entry(key).or_insert_with(|| {
                            out.points.push(at[a]);
                            out.lattice.push(Some(key));
                            (out.points.len() - 1) as u32
                        })
                    };
                    // A crossing that lands on one of the edge's own ends *is* that end. Saying
                    // so matters: a node lying on the surface counts as inside it, so the edge
                    // from there out into the air crosses at the node itself, and making a second
                    // point in the same place would leave the elements either side of it free to
                    // move apart -- a tear along the surface, which is the one thing the sharing
                    // is for. On a box square to the lattice this is most of the crossings.
                    let at_cross =
                        |out: &mut CutMesh, nodes: &mut HashMap<u32, u32>, seen: &mut HashMap<(u32, u32), u32>, a: usize, b: usize| -> u32 {
                            let (x, y) = (node(a), node(b));
                            let key = if x < y { (x, y) } else { (y, x) };
                            if let Some(found) = seen.get(&key) {
                                return *found;
                            }
                            let p = between(inside, at[a], at[b]);
                            for end in [a, b] {
                                if p.dist(at[end]) <= on_corner {
                                    let node_key = node(end);
                                    let at_end = *nodes.entry(node_key).or_insert_with(|| {
                                        out.points.push(at[end]);
                                        out.lattice.push(Some(node_key));
                                        (out.points.len() - 1) as u32
                                    });
                                    seen.insert(key, at_end);
                                    return at_end;
                                }
                            }
                            out.points.push(p);
                            out.lattice.push(None);
                            let made = (out.points.len() - 1) as u32;
                            seen.insert(key, made);
                            made
                        };

                    let mut shape: Vec<[u32; 4]> = Vec::new();
                    match ins.len() {
                        0 => {}
                        4 => {
                            let p: [u32; 4] = std::array::from_fn(|t| at_corner(&mut out, &mut from_node, t));
                            shape.push(p);
                        }
                        1 => {
                            let a = ins[0];
                            let pa = at_corner(&mut out, &mut from_node, a);
                            let c0 = at_cross(&mut out, &mut from_node, &mut from_edge, a, outs[0]);
                            let c1 = at_cross(&mut out, &mut from_node, &mut from_edge, a, outs[1]);
                            let c2 = at_cross(&mut out, &mut from_node, &mut from_edge, a, outs[2]);
                            shape.push([pa, c0, c1, c2]);
                        }
                        3 => {
                            let d = outs[0];
                            let (a, b, c) = (ins[0], ins[1], ins[2]);
                            let pa = at_corner(&mut out, &mut from_node, a);
                            let pb = at_corner(&mut out, &mut from_node, b);
                            let pc = at_corner(&mut out, &mut from_node, c);
                            let da = at_cross(&mut out, &mut from_node, &mut from_edge, a, d);
                            let db = at_cross(&mut out, &mut from_node, &mut from_edge, b, d);
                            let dc = at_cross(&mut out, &mut from_node, &mut from_edge, c, d);
                            shape.push([pa, pb, pc, da]);
                            shape.push([pb, pc, da, db]);
                            shape.push([pc, da, db, dc]);
                        }
                        _ => {
                            let (a, b) = (ins[0], ins[1]);
                            let (c, d) = (outs[0], outs[1]);
                            let p = [
                                at_corner(&mut out, &mut from_node, a),
                                at_cross(&mut out, &mut from_node, &mut from_edge, a, c),
                                at_cross(&mut out, &mut from_node, &mut from_edge, a, d),
                                at_corner(&mut out, &mut from_node, b),
                                at_cross(&mut out, &mut from_node, &mut from_edge, b, c),
                                at_cross(&mut out, &mut from_node, &mut from_edge, b, d),
                            ];
                            shape.push([p[0], p[1], p[2], p[3]]);
                            shape.push([p[1], p[2], p[3], p[4]]);
                            shape.push([p[2], p[3], p[4], p[5]]);
                        }
                    }
                    for p in shape {
                        out.push_tet(p, least);
                    }
                }
            }
        }
    }
    out
}

/// Whether the surface passes through a cell none of whose corners are inside it: a bump small
/// enough to miss every corner still holds material, and skipping such a cell would lose it.
fn crosses(inside: &dyn Fn(Vec3) -> bool, corners: &[Vec3; 8]) -> bool {
    let middle = corners.iter().fold(Vec3::new(0.0, 0.0, 0.0), |a, b| a + *b) * (1.0 / 8.0);
    inside(middle)
}

/// A stiffness assembled over a cut mesh, applied element by element the way the lattice's is.
///
/// Three freedoms a point, over both kinds of point alike: a lattice node and a crossing are the
/// same thing to the solver, which is the whole value of having built the mesh so that a crossing
/// is shared rather than duplicated.
pub struct CutOperator<'a> {
    mesh: &'a CutMesh,
    element: Vec<[[f64; crate::elastic::TET_DOFS]; crate::elastic::TET_DOFS]>,
    held: Vec<bool>,
    diag: Vec<f64>,
}

impl<'a> CutOperator<'a> {
    /// Work out every tetrahedron's stiffness once.
    ///
    /// `held` is by freedom, three a point. A held one gets an identity row, as it does on the
    /// lattice, so the solve leaves it where it is.
    pub fn new(mesh: &'a CutMesh, material: &crate::elastic::ElasticMaterial, held: Vec<bool>) -> CutOperator<'a> {
        let element: Vec<_> = (0..mesh.tets().len())
            .map(|i| {
                mesh.tet_at(i)
                    .map(|at| {
                        let m: [Vec3; 4] = std::array::from_fn(|c| at[c] / crate::grid::MM_PER_M);
                        crate::elastic::tet_stiffness(material, &m).unwrap_or([[0.0; crate::elastic::TET_DOFS]; crate::elastic::TET_DOFS])
                    })
                    .unwrap_or([[0.0; crate::elastic::TET_DOFS]; crate::elastic::TET_DOFS])
            })
            .collect();
        let mut this = CutOperator { mesh, element, held, diag: Vec::new() };
        let mut diag = vec![0.0; this.freedoms()];
        for (t, k) in this.mesh.tets().iter().zip(&this.element) {
            for (local, point) in t.iter().enumerate() {
                for axis in 0..3 {
                    let row = local * 3 + axis;
                    if let (Some(v), Some(slot)) = (k.get(row).and_then(|r| r.get(row)), diag.get_mut(*point as usize * 3 + axis)) {
                        *slot += v;
                    }
                }
            }
        }
        for (dof, slot) in diag.iter_mut().enumerate() {
            if this.held.get(dof).copied().unwrap_or(false) || !(*slot > 0.0) {
                *slot = 1.0;
            }
        }
        this.diag = diag;
        this
    }

    pub fn freedoms(&self) -> usize {
        self.mesh.points().len() * 3
    }

    /// The internal force the tetrahedra carry, without the identity rows a support puts in.
    pub fn internal(&self, x: &[f64], out: &mut [f64]) -> crate::Result<()> {
        if x.len() != self.diag.len() || out.len() != self.diag.len() {
            return crate::setup("the solver was handed a vector of the wrong length");
        }
        out.fill(0.0);
        for (t, k) in self.mesh.tets().iter().zip(&self.element) {
            let mut local = [0.0f64; crate::elastic::TET_DOFS];
            for (c, point) in t.iter().enumerate() {
                for axis in 0..3 {
                    let dof = *point as usize * 3 + axis;
                    if self.held.get(dof).copied().unwrap_or(false) {
                        continue;
                    }
                    if let (Some(slot), Some(v)) = (local.get_mut(c * 3 + axis), x.get(dof)) {
                        *slot = *v;
                    }
                }
            }
            for (c, point) in t.iter().enumerate() {
                for axis in 0..3 {
                    let row = c * 3 + axis;
                    let Some(krow) = k.get(row) else { continue };
                    let v: f64 = krow.iter().zip(&local).map(|(a, b)| a * b).sum();
                    if let Some(slot) = out.get_mut(*point as usize * 3 + axis) {
                        *slot += v;
                    }
                }
            }
        }
        Ok(())
    }
}

impl crate::linalg::Symmetric for CutOperator<'_> {
    fn equations(&self) -> usize {
        self.freedoms()
    }

    fn mul(&self, x: &[f64], out: &mut [f64]) -> crate::Result<()> {
        self.internal(x, out)?;
        // A held freedom keeps what it was given, which is what makes the row an identity.
        for (dof, held) in self.held.iter().enumerate() {
            if *held && let (Some(o), Some(v)) = (out.get_mut(dof), x.get(dof)) {
                *o = *v;
            }
        }
        Ok(())
    }

    fn diagonal(&self) -> &[f64] {
        &self.diag
    }
}

/// The worst von Mises stress over a cut mesh, with the part held on one side and pulled on the
/// other.
///
/// This is a measuring instrument, not the study command: it takes the one load case the
/// comparison needs -- hold the near end, pull the far one -- because that is what the references
/// are run with. A traction is spread over the triangles of the loaded face by their own area, a
/// third to each of their points, which is what a linear tetrahedron's face does with a uniform
/// pressure.
///
/// `clamp` says how the near end is held, and the difference is not a detail. Clamped, it is
/// held in every direction, which is what the CalculiX reference does and what a study with a
/// `fixed` support does -- but it also stops the part contracting as it stretches, so there is a
/// real concentration at the clamp and the stress is no longer uniform anywhere near it. On
/// rollers each face of the near corner is held only across itself, which leaves the part free
/// to change shape: the state of strain stays uniform, and a bar pulled that way must read the
/// traction exactly. Use rollers to check the solver, the clamp to compare with something.
pub fn worst_pulled(
    mesh: &CutMesh,
    material: &crate::elastic::ElasticMaterial,
    axis: usize,
    traction_pa: f64,
    clamp: bool,
    tolerance: f64,
    max_iterations: u32,
) -> crate::Result<f64> {
    worst_pulled_either_way(mesh, material, axis, traction_pa, clamp, tolerance, max_iterations).map(|(raw, _)| raw)
}

/// The same, giving both the worst element's stress and the worst after the field is carried to
/// the points and back.
///
/// Averaging to the nodes is what a finite element post-processor shows, and on *this* mesh the
/// question it answers is different from the one it answered on the lattice. There, averaging
/// damped a real singularity and read a tenth low -- the staircase's re-entrant corners have no
/// finite stress, so smoothing them away is smoothing away the only thing there. A cut mesh has
/// no such corners: its boundary is the surface, the field near it is a field, and averaging is
/// then doing what it is meant to do, which is take the sampling noise of one element off a
/// quantity that varies smoothly.
pub fn worst_pulled_either_way(
    mesh: &CutMesh,
    material: &crate::elastic::ElasticMaterial,
    axis: usize,
    traction_pa: f64,
    clamp: bool,
    tolerance: f64,
    max_iterations: u32,
) -> crate::Result<(f64, f64)> {
    let (mut low, mut high) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in mesh.points() {
        for (a, (lo, hi)) in low.iter_mut().zip(high.iter_mut()).enumerate() {
            let v = [p.x, p.y, p.z][a];
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }
    let span = high[axis] - low[axis];
    if !(span > 0.0) {
        return crate::setup("the model has no extent along the axis it is pulled along");
    }
    let near = span * 1e-6;
    let on_axis = |p: Vec3, a: usize, at: f64| ([p.x, p.y, p.z][a] - at).abs() <= near;
    let on = |p: Vec3, at: f64| on_axis(p, axis, at);

    let mut held = vec![false; mesh.points().len() * 3];
    for (i, p) in mesh.points().iter().enumerate() {
        if clamp {
            // Every direction on the near face.
            if on(*p, low[axis]) {
                for a in 0..3 {
                    if let Some(slot) = held.get_mut(i * 3 + a) {
                        *slot = true;
                    }
                }
            }
        } else {
            // Each near face held only across itself, which holds the part still without
            // stopping it change shape.
            for (a, lo) in low.iter().enumerate() {
                if on_axis(*p, a, *lo)
                    && let Some(slot) = held.get_mut(i * 3 + a)
                {
                    *slot = true;
                }
            }
        }
    }

    // Pulled: the traction over the area of each face lying on the high plane.
    let mut force = vec![0.0f64; mesh.points().len() * 3];
    const FACES: [[usize; 3]; 4] = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];
    for t in mesh.tets() {
        for f in FACES {
            let pts: [u32; 3] = [t[f[0]], t[f[1]], t[f[2]]];
            let at: [Vec3; 3] = std::array::from_fn(|i| mesh.points().get(pts[i] as usize).copied().unwrap_or_default());
            if !at.iter().all(|p| on(*p, high[axis])) {
                continue;
            }
            // Millimetres on the mesh, metres in the physics.
            let area = (at[1] - at[0]).cross(at[2] - at[0]).len() * 0.5 / (crate::grid::MM_PER_M * crate::grid::MM_PER_M);
            let share = traction_pa * area / 3.0;
            for p in pts {
                if let Some(slot) = force.get_mut(p as usize * 3 + axis) {
                    *slot += share;
                }
            }
        }
    }

    let operator = CutOperator::new(mesh, material, held.clone());
    let mut rhs = force.clone();
    for (dof, h) in held.iter().enumerate() {
        if *h && let Some(slot) = rhs.get_mut(dof) {
            *slot = 0.0;
        }
    }
    let mut u = vec![0.0; operator.freedoms()];
    crate::linalg::solve(&operator, &rhs, &mut u, tolerance, max_iterations)?;

    // The stress in each tetrahedron, which is constant across it.
    let d = crate::elastic::elasticity_matrix(material);
    let mut worst = 0.0f64;
    // And the same carried to the points, each tetrahedron weighted by its volume so that a
    // sliver does not count for as much as the element beside it.
    let mut at_point = vec![[0.0f64; 6]; mesh.points().len()];
    let mut weight = vec![0.0f64; mesh.points().len()];
    for (i, t) in mesh.tets().iter().enumerate() {
        let Some(at) = mesh.tet_at(i) else { continue };
        let m: [Vec3; 4] = std::array::from_fn(|c| at[c] / crate::grid::MM_PER_M);
        let Some((b, _)) = crate::elastic::tet_strain_matrix_of(&m) else { continue };
        let mut local = [0.0f64; crate::elastic::TET_DOFS];
        for (c, point) in t.iter().enumerate() {
            for axis in 0..3 {
                if let (Some(slot), Some(v)) = (local.get_mut(c * 3 + axis), u.get(*point as usize * 3 + axis)) {
                    *slot = *v;
                }
            }
        }
        let strain: [f64; 6] = std::array::from_fn(|r| b[r].iter().zip(&local).map(|(a, b)| a * b).sum());
        let stress: [f64; 6] = std::array::from_fn(|r| d[r].iter().zip(&strain).map(|(a, b)| a * b).sum());
        worst = worst.max(crate::elastic::von_mises(stress));
        // Every tetrahedron touching a point counts the same. Weighting by volume was tried
        // and is wrong here: a boundary point is shared by the small tetrahedra the cut made
        // and the whole interior one behind them, so the interior one carries the average away
        // from the surface, and worst where the sizes differ most.
        for point in t {
            let Some(slot) = at_point.get_mut(*point as usize) else { continue };
            for (a, v) in slot.iter_mut().zip(&stress) {
                *a += v;
            }
            if let Some(w) = weight.get_mut(*point as usize) {
                *w += 1.0;
            }
        }
    }
    let mut smoothed = 0.0f64;
    let mut at: Option<Vec3> = None;
    for (i, (sum, w)) in at_point.iter().zip(&weight).enumerate() {
        if *w <= 0.0 {
            continue;
        }
        let stress: [f64; 6] = std::array::from_fn(|r| sum[r] / w);
        let v = crate::elastic::von_mises(stress);
        if v > smoothed {
            smoothed = v;
            at = mesh.points().get(i).copied();
        }
    }
    let _ = at;
    Ok((worst, smoothed))
}

#[cfg(test)]
#[path = "cut_tests.rs"]
mod tests;
