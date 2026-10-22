//! Part of a record: the bounded material regions asked for, each with the voids inside it, as a
//! record of its own in canonical order (docs/native-brep.md, "Canonical order"), and where each
//! of its faces, edges and vertices was in the whole. A lump of a body is the part of one of its
//! material regions; material inside one of its voids (an island) is a lump of its own.
use super::{Brep, EdgeUseRef, Invalid, Loop, OrientedFace, Region, Result, Shell, require};

/// A record cut from a larger one, and the source index of each of its items.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub brep: Brep,
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
    pub vertices: Vec<u32>,
}

/// First-appearance numbering of a subset of `n` source items.
struct Renumber {
    new: Vec<Option<u32>>,
    old: Vec<u32>,
}
impl Renumber {
    fn new(n: usize) -> Renumber {
        Renumber { new: vec![None; n], old: Vec::new() }
    }
    /// The new index of source item `i`, numbering it now if it is new.
    fn visit(&mut self, i: u32) -> Result<u32> {
        let slot = self.new.get_mut(i as usize).ok_or(Invalid("part reference"))?;
        if let Some(k) = slot {
            return Ok(*k);
        }
        let k = u32::try_from(self.old.len()).map_err(|_| Invalid("part index"))?;
        *slot = Some(k);
        self.old.push(i);
        Ok(k)
    }
    fn get(&self, i: u32) -> Option<u32> {
        self.new.get(i as usize).copied().flatten()
    }
}

impl Brep {
    /// Indices of the bounded material regions (not voids), in region order.
    pub fn material_regions(&self) -> Vec<usize> {
        self.regions.iter().enumerate().skip(1).filter(|(_, r)| !r.void).map(|(i, _)| i).collect()
    }

    /// The part made of the material regions `regions` (indices into `self.regions`, bounded,
    /// not voids): each with its outer shell, its open and wire shells, and a void for each
    /// closed inner shell. Islands inside those voids are left out (they are parts of their own).
    pub fn part(&self, regions: &[usize]) -> Result<Part> {
        // Which shell encloses which region: the region that lists a closed shell first.
        let mut encloses: Vec<Option<usize>> = vec![None; self.shells.len()];
        for (r, region) in self.regions.iter().enumerate().skip(1) {
            if let Some(&s) = region.shells.first() {
                *encloses.get_mut(s as usize).ok_or(Invalid("region shell reference"))? = Some(r);
            }
        }
        // The part's regions over source shells: the infinite one around the outer shells, each
        // material region as it is, and each void inside one by its outer shell alone.
        let mut outer = Vec::new();
        let mut kept: Vec<(Vec<u32>, bool)> = Vec::new();
        for &r in regions {
            let region = self.regions.get(r).ok_or(Invalid("part region"))?;
            require(r > 0 && !region.void, "a part's region is bounded material")?;
            outer.push(*region.shells.first().ok_or(Invalid("bounded region without an outer shell"))?);
            kept.push((region.shells.clone(), false));
            for &s in region.shells.iter().skip(1) {
                if let Some(v) = encloses.get(s as usize).copied().flatten() {
                    require(self.regions.get(v).is_some_and(|v| v.void), "a closed inner shell of material encloses a void")?;
                    kept.push((vec![s], true));
                }
            }
        }
        let source_regions: Vec<(Vec<u32>, bool)> = std::iter::once((outer, false)).chain(kept).collect();

        // Canonical order: shells and faces walking regions → shells → oriented faces.
        let mut shells = Renumber::new(self.shells.len());
        let mut faces = Renumber::new(self.faces.len());
        let mut direction = vec![true; self.faces.len()];
        let mut new_regions = Vec::with_capacity(source_regions.len());
        for (list, void) in &source_regions {
            let mut mapped = Vec::with_capacity(list.len());
            for &s in list {
                mapped.push(shells.visit(s)?);
                if let Some(Shell::Faces { faces: uses, .. }) = self.shells.get(s as usize) {
                    for f in uses {
                        if faces.get(f.face).is_none()
                            && let Some(d) = direction.get_mut(f.face as usize)
                        {
                            *d = f.forward;
                        }
                        faces.visit(f.face)?;
                    }
                }
            }
            new_regions.push(Region { shells: mapped, void: *void });
        }
        // Edges walking faces → loops → oriented edges (backwards where the face's first side is
        // reversed), then wire edges walking shells.
        // Each edge's first use in that walk is its primary use: (new face, loop, position).
        let mut edges = Renumber::new(self.edges.len());
        let mut primary: Vec<Option<(u32, u32, u32)>> = vec![None; self.edges.len()];
        for (k, &f) in faces.old.iter().enumerate() {
            let face = self.faces.get(f as usize).ok_or(Invalid("face reference"))?;
            let forward = direction.get(f as usize).copied().unwrap_or(true);
            let k = index(k)?;
            for (li, l) in face.loops.iter().enumerate() {
                let Loop::Edges { edges: uses } = l else { continue };
                let n = uses.len();
                for step in 0..n {
                    let p = if forward { step } else { n - 1 - step };
                    let u = uses.get(p).ok_or(Invalid("edge reference"))?;
                    edges.visit(u.edge)?;
                    let first = primary.get_mut(u.edge as usize).ok_or(Invalid("edge reference"))?;
                    first.get_or_insert((k, index(li)?, index(p)?));
                }
            }
        }
        for &s in &shells.old {
            let wires = match self.shells.get(s as usize).ok_or(Invalid("shell reference"))? {
                Shell::Faces { wires, .. } => wires.as_slice(),
                Shell::Wire { edges } => edges.as_slice(),
                Shell::Vertex { .. } => &[],
            };
            for &e in wires {
                edges.visit(e)?;
            }
        }
        // Vertices walking edges (start, end), then vertex loops, then vertex shells.
        let mut vertices = Renumber::new(self.vertices.len());
        for &e in &edges.old {
            for &v in &self.edges.get(e as usize).ok_or(Invalid("edge reference"))?.vertices {
                vertices.visit(v)?;
            }
        }
        for &f in &faces.old {
            for l in &self.faces.get(f as usize).ok_or(Invalid("face reference"))?.loops {
                if let Loop::Vertex { vertex } = l {
                    vertices.visit(*vertex)?;
                }
            }
        }
        for &s in &shells.old {
            if let Some(Shell::Vertex { vertex }) = self.shells.get(s as usize) {
                vertices.visit(*vertex)?;
            }
        }

        let map = |r: &Renumber, i: u32| r.get(i).ok_or(Invalid("part reference outside the part"));
        let new_shells = shells
            .old
            .iter()
            .map(|&s| {
                Ok(match self.shells.get(s as usize).ok_or(Invalid("shell reference"))? {
                    Shell::Faces { faces: uses, wires } => Shell::Faces {
                        faces: uses.iter().map(|f| Ok(OrientedFace { face: map(&faces, f.face)?, forward: f.forward })).collect::<Result<_>>()?,
                        wires: wires.iter().map(|&e| map(&edges, e)).collect::<Result<_>>()?,
                    },
                    Shell::Wire { edges: wire } => Shell::Wire { edges: wire.iter().map(|&e| map(&edges, e)).collect::<Result<_>>()? },
                    Shell::Vertex { vertex } => Shell::Vertex { vertex: map(&vertices, *vertex)? },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let new_faces = faces
            .old
            .iter()
            .map(|&f| {
                let mut face = self.faces.get(f as usize).ok_or(Invalid("face reference"))?.clone();
                for l in &mut face.loops {
                    match l {
                        Loop::Edges { edges: uses } => {
                            for u in uses {
                                u.edge = map(&edges, u.edge)?;
                            }
                        }
                        Loop::Vertex { vertex } => *vertex = map(&vertices, *vertex)?,
                    }
                }
                Ok(face)
            })
            .collect::<Result<Vec<_>>>()?;
        let new_edges = edges
            .old
            .iter()
            .map(|&e| {
                let mut edge = self.edges.get(e as usize).ok_or(Invalid("edge reference"))?.clone();
                edge.vertices = [map(&vertices, edge.vertices[0])?, map(&vertices, edge.vertices[1])?];
                edge.uses = part_uses(&edge.uses, &faces, primary.get(e as usize).copied().flatten());
                Ok(edge)
            })
            .collect::<Result<Vec<_>>>()?;
        let new_vertices = vertices.old.iter().map(|&v| self.vertices.get(v as usize).cloned().ok_or(Invalid("vertex reference"))).collect::<Result<_>>()?;
        let brep = Brep {
            version: self.version,
            tolerance: self.tolerance,
            regions: new_regions,
            shells: new_shells,
            faces: new_faces,
            edges: new_edges,
            vertices: new_vertices,
        };
        brep.validate()?;
        Ok(Part { brep, faces: faces.old, edges: edges.old, vertices: vertices.old })
    }
}

/// An edge's uses on the part's faces: the source's radial cycle with the other faces' uses left
/// out (a sub-cycle keeps its order), turned to start at the part's primary use, the first in
/// the part's canonical walk (`primary`: face, loop, position).
fn part_uses(uses: &[EdgeUseRef], faces: &Renumber, primary: Option<(u32, u32, u32)>) -> Vec<EdgeUseRef> {
    let mut kept: Vec<EdgeUseRef> = uses.iter().filter_map(|u| faces.get(u.face).map(|face| EdgeUseRef { face, ..*u })).collect();
    if let Some(first) = primary.and_then(|p| kept.iter().position(|u| (u.face, u.r#loop, u.position) == p)) {
        kept.rotate_left(first);
    }
    kept
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| Invalid("part index"))
}
