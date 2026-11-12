//! Which face of the model each face of the lattice belongs to.
//!
//! A boundary condition is set by picking a face in the model -- this one gets the fan, that
//! one bolts to a cold plate -- and the solver works in cells. Something has to carry a pick
//! across, and this is it: every face of every cell on the surface is matched to the nearest
//! triangle of the body that fills it, and takes that triangle's B-rep face.
//!
//! Nearest, rather than along a ray, because a ray can slip out through a gap where a wall
//! lands between cells, and because a lattice's staircase of a curved wall has its cell faces
//! turned away from the real surface. Distance does not care about either. The search runs
//! triangle by triangle rather than face by face, so its cost follows the model's surface area
//! rather than the number of cells: each triangle only looks at the cells its own box reaches.

use std::collections::HashMap;

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Mesh, mesh_distance::TriangleSet};

use crate::grid::{Occupancy, SurfaceFace, cell_range};

/// How far past its own box a triangle looks for cell faces, in cells. A boundary cell's face
/// is within half a cell of the wall by construction, and a diagonal wall can leave it a little
/// further, so one cell of slack is enough and two is safe.
const SLACK_CELLS: f64 = 2.0;

/// A patch of the model's surface, as the lattice holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    /// Which body, in the order the parts were given.
    pub part: u16,
    /// Which of that body's B-rep faces.
    pub face: u32,
    /// The cell faces that belong to it.
    pub faces: Vec<SurfaceFace>,
}

impl Patch {
    /// The patch's area on the lattice, in square metres. This is a staircase of the real
    /// face, so for a face square to an axis it is exact and for a curved or slanted one it
    /// over-states the area -- a slope at forty-five degrees by about forty per cent.
    pub fn area_m2(&self, occ: &Occupancy) -> f64 {
        self.faces.len() as f64 * occ.grid().face_area_m2()
    }
}

/// Every face of the lattice's surface, matched to a face of the model.
#[derive(Clone, Debug)]
pub struct FaceTags {
    faces: Vec<SurfaceFace>,
    /// Per entry of `faces`: the body and B-rep face it belongs to.
    tag: Vec<Option<(u16, u32)>>,
}

impl FaceTags {
    /// Match each of the lattice's surface faces to the nearest triangle of the body that
    /// fills its cell.
    ///
    /// `parts` is in the same order as the occupancy's, so a tag's part number indexes it.
    pub fn of(occ: &Occupancy, parts: &[&Mesh]) -> FaceTags {
        let grid = occ.grid();
        let faces: Vec<SurfaceFace> = occ.surface().collect();
        // Which surface faces each cell has, so a triangle sweeping a range of cells can find
        // them without walking the whole list.
        let mut by_cell: HashMap<u32, Vec<u32>> = HashMap::new();
        for (i, f) in faces.iter().enumerate() {
            by_cell.entry(f.cell).or_default().push(i as u32);
        }
        let mut best = vec![f64::INFINITY; faces.len()];
        let mut tag: Vec<Option<(u16, u32)>> = vec![None; faces.len()];
        let slack = grid.spacing() * SLACK_CELLS;

        for (part, mesh) in parts.iter().enumerate() {
            let set = TriangleSet::new(mesh);
            for t in 0..set.len() as u32 {
                let Some(corners) = set.triangle(t) else { continue };
                let mut around = Aabb3::EMPTY;
                for v in corners {
                    around.add(v);
                }
                let around = around.inflated(slack);
                let Some((from, to)) = cell_range(grid, &around) else { continue };
                let model_face = set.face(t).unwrap_or(0);
                for k in from[2]..=to[2] {
                    for j in from[1]..=to[1] {
                        for i in from[0]..=to[0] {
                            let Some(cell) = grid.at(i, j, k) else { continue };
                            // Only a cell this body actually fills: a face of one body must
                            // not be claimed by a triangle of the body next to it.
                            if occ.part_of(cell) != Some(part as u16) {
                                continue;
                            }
                            let Some(here) = by_cell.get(&(cell as u32)) else { continue };
                            for index in here {
                                let index = *index as usize;
                                let Some(face) = faces.get(index) else { continue };
                                let Some(at) = grid.face_centre(*face) else { continue };
                                let d = set.distance_to(at, &[t]);
                                let Some(slot) = best.get_mut(index) else { continue };
                                // Equal distances go to the lower face number, so the answer
                                // does not depend on the order the triangles happen to be in.
                                let take = match tag.get(index).copied().flatten() {
                                    None => d <= *slot,
                                    Some((_, had)) => d < *slot || (d == *slot && model_face < had),
                                };
                                if take {
                                    *slot = d;
                                    if let Some(entry) = tag.get_mut(index) {
                                        *entry = Some((part as u16, model_face));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        FaceTags { faces, tag }
    }

    /// Every surface face of the lattice, in the order the occupancy gives them.
    pub fn faces(&self) -> &[SurfaceFace] {
        &self.faces
    }

    /// Which body and face a surface face belongs to, or `None` when nothing claimed it.
    pub fn tag_of(&self, index: usize) -> Option<(u16, u32)> {
        self.tag.get(index).copied().flatten()
    }

    /// The cell faces that belong to one face of one body.
    pub fn patch(&self, part: u16, face: u32) -> Vec<SurfaceFace> {
        self.faces.iter().zip(&self.tag).filter(|(_, t)| **t == Some((part, face))).map(|(f, _)| *f).collect()
    }

    /// The cell faces that belong to any of several faces of bodies -- what a selection of
    /// faces in the model becomes.
    pub fn patch_of(&self, picks: &[(u16, u32)]) -> Vec<SurfaceFace> {
        self.faces.iter().zip(&self.tag).filter(|(_, t)| t.is_some_and(|x| picks.contains(&x))).map(|(f, _)| *f).collect()
    }

    /// Every face of the model the lattice can see, in body and face order, with its cells.
    ///
    /// A face too small for the lattice to reach is absent, which is the honest answer: there
    /// is nowhere to put a boundary condition on it at this spacing.
    pub fn patches(&self) -> Vec<Patch> {
        let mut found: HashMap<(u16, u32), Vec<SurfaceFace>> = HashMap::new();
        for (f, t) in self.faces.iter().zip(&self.tag) {
            if let Some(key) = t {
                found.entry(*key).or_default().push(*f);
            }
        }
        let mut out: Vec<Patch> = found.into_iter().map(|((part, face), faces)| Patch { part, face, faces }).collect();
        out.sort_by_key(|p| (p.part, p.face));
        for p in &mut out {
            p.faces.sort();
        }
        out
    }

    /// How many of the lattice's surface faces nothing claimed.
    ///
    /// Anything but zero means the surface has somewhere a boundary condition set by picking a
    /// face would not reach, so a caller can say so rather than silently leaving it to the
    /// study's default.
    pub fn unclaimed(&self) -> usize {
        self.tag.iter().filter(|t| t.is_none()).count()
    }

    /// How much to scale each surface face's area by, so that every face of the model carries
    /// the area it really has rather than the area of the staircase standing in for it.
    ///
    /// This matters more than it sounds. A lattice holds a face square to an axis exactly, but a
    /// slanted one as a flight of steps whose area is larger than the face's -- by a factor of
    /// `|nx| + |ny| + |nz|`, which is √3 for a face at the corner of the axes and never goes
    /// away however fine the lattice gets. A cylinder's curved side comes out `4/π`, some 27 per
    /// cent, too large. Every heat flux, film, pressure and traction is an area multiplied by
    /// something, so without this they are all out by that much.
    ///
    /// The correction is one number per face of the model: what the mesh says its area is,
    /// over what the lattice's cells say. Spread evenly over that face's cells, it makes the
    /// face's *total* right, which is what a flux integrates to. It does not pretend to put
    /// each individual cell's area right, and it cannot: the lattice has no finer picture of
    /// where that face is.
    ///
    /// In the order [`Occupancy::surface`] gives them. Faces nothing claimed keep their own
    /// area, there being nothing better to say about them.
    pub fn area_scale(&self, occ: &Occupancy, parts: &[&Mesh]) -> Vec<f32> {
        let cell_area = occ.grid().face_area_m2();
        // What the model says each of its faces measures.
        let mut truth: HashMap<(u16, u32), f64> = HashMap::new();
        for (p, mesh) in parts.iter().enumerate() {
            for (t, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
                let Some([a, b, c]) = mesh.tri(t) else { continue };
                let area = (b - a).cross(c - a).len() / 2.0;
                if !area.is_finite() {
                    continue;
                }
                // Square metres, the model being in millimetres.
                *truth.entry((p as u16, *face)).or_default() += area * 1e-6;
            }
        }
        // What the lattice says.
        let mut staircase: HashMap<(u16, u32), f64> = HashMap::new();
        for tag in self.tag.iter().flatten() {
            *staircase.entry(*tag).or_default() += cell_area;
        }
        self.tag
            .iter()
            .map(|tag| {
                let Some(key) = tag else { return 1.0 };
                let (Some(real), Some(steps)) = (truth.get(key), staircase.get(key)) else { return 1.0 };
                if !(*real > 0.0) || !(*steps > 0.0) {
                    return 1.0;
                }
                (real / steps) as f32
            })
            .collect()
    }

    /// The point on the lattice each surface face sits at, for drawing a result on the model.
    pub fn centres(&self, occ: &Occupancy) -> Vec<Vec3> {
        self.faces.iter().filter_map(|f| occ.grid().face_centre(*f)).collect()
    }
}

#[cfg(test)]
#[path = "tag_tests.rs"]
mod tests;
