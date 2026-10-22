//! Part of a record: the bounded material regions asked for, each with the voids inside it, as a
//! record of its own in canonical order (docs/native-brep.md, "Canonical order"), and where each
//! of its faces, edges and vertices was in the whole. A lump of a body is the part of one of its
//! material regions; material inside one of its voids (an island) is a lump of its own. Records
//! of bodies apart from each other join into one by [`Brep::compound`].
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
    /// closed inner shell. An island inside one of those voids is in the part, inside it, when
    /// its own region is among `regions`, and left out otherwise.
    pub fn part(&self, regions: &[usize]) -> Result<Part> {
        // Which region each closed shell encloses (the region listing it first), and which
        // region lies around it (the one listing it after its own outer shell; 0 for the open).
        let mut encloses: Vec<Option<usize>> = vec![None; self.shells.len()];
        let mut around: Vec<usize> = vec![0; self.shells.len()];
        for (r, region) in self.regions.iter().enumerate() {
            for (k, &s) in region.shells.iter().enumerate() {
                let i = s as usize;
                if r > 0 && k == 0 {
                    *encloses.get_mut(i).ok_or(Invalid("region shell reference"))? = Some(r);
                } else {
                    *around.get_mut(i).ok_or(Invalid("region shell reference"))? = r;
                }
            }
        }
        // The part's regions over source shells: each material region as it is, each void
        // inside one by its outer shell, and the infinite region around every outer shell whose
        // surrounding void is not in the part. A material region inside a void that is in the
        // part (an island in a hollow both are taken from) stays inside it.
        let mut voids: Vec<(usize, Vec<u32>)> = Vec::new();
        let mut kept: Vec<(Vec<u32>, bool)> = Vec::new();
        for &r in regions {
            let region = self.regions.get(r).ok_or(Invalid("part region"))?;
            require(r > 0 && !region.void, "a part's region is bounded material")?;
            require(!region.shells.is_empty(), "bounded region without an outer shell")?;
            kept.push((region.shells.clone(), false));
            for &s in region.shells.iter().skip(1) {
                if let Some(v) = encloses.get(s as usize).copied().flatten() {
                    require(self.regions.get(v).is_some_and(|v| v.void), "a closed inner shell of material encloses a void")?;
                    voids.push((v, vec![s]));
                }
            }
        }
        let mut outer = Vec::new();
        for &r in regions {
            let Some(&s) = self.regions.get(r).and_then(|region| region.shells.first()) else { continue };
            let container = around.get(s as usize).copied().unwrap_or(0);
            match voids.iter_mut().find(|(v, _)| *v == container) {
                Some((_, list)) => list.push(s),
                None => outer.push(s),
            }
        }
        let source_regions: Vec<(Vec<u32>, bool)> =
            std::iter::once((outer, false)).chain(kept).chain(voids.into_iter().map(|(_, list)| (list, true))).collect();
        self.renumbered(&source_regions)
    }

    /// The record in canonical order (docs/native-brep.md, "Canonical order"), every region and
    /// shell kept, open, wire and vertex shells included, and where each item was in it. For a
    /// record whose parts a builder put together in its own order: each edge's uses are worked
    /// out from the walk (a list that already names exactly those uses keeps its radial order).
    /// A face no shell uses is left out.
    pub fn canonical(&self) -> Result<Part> {
        let regions: Vec<(Vec<u32>, bool)> = self.regions.iter().map(|r| (r.shells.clone(), r.void)).collect();
        self.renumbered(&regions)
    }

    /// The record of `source_regions` (each the source shells it lists and whether it is a void,
    /// the infinite region first), renumbered in canonical order.
    fn renumbered(&self, source_regions: &[(Vec<u32>, bool)]) -> Result<Part> {
        // Canonical order: shells and faces walking regions → shells → oriented faces.
        let mut shells = Renumber::new(self.shells.len());
        let mut faces = Renumber::new(self.faces.len());
        let mut direction = vec![true; self.faces.len()];
        let mut new_regions = Vec::with_capacity(source_regions.len());
        for (list, void) in source_regions {
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
        let mut walked: Vec<Vec<EdgeUseRef>> = vec![Vec::new(); self.edges.len()];
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
                    let at = EdgeUseRef { face: k, r#loop: index(li)?, position: index(p)?, upper: !u.forward };
                    walked.get_mut(u.edge as usize).ok_or(Invalid("edge reference"))?.push(at);
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
                edge.uses = part_uses(&edge.uses, &faces, walked.get(e as usize).map_or(&[][..], Vec::as_slice));
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

/// An edge's uses on the part's faces (`walked`: in the canonical walk's order, the first its
/// primary use): the source's radial cycle with the other faces' uses left out (a sub-cycle keeps
/// its order), turned to start at the primary use, where the source names exactly the walked
/// uses; the walk's own order otherwise (a record whose builder left them to be worked out).
fn part_uses(uses: &[EdgeUseRef], faces: &Renumber, walked: &[EdgeUseRef]) -> Vec<EdgeUseRef> {
    let mut kept: Vec<EdgeUseRef> = uses.iter().filter_map(|u| faces.get(u.face).map(|face| EdgeUseRef { face, ..*u })).collect();
    if kept.len() != walked.len() || !walked.iter().all(|w| kept.contains(w)) {
        return walked.to_vec();
    }
    if let Some(first) = walked.first().and_then(|p| kept.iter().position(|u| u == p)) {
        kept.rotate_left(first);
    }
    kept
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| Invalid("part index"))
}

/// Bodies' records joined into one record of all their material, side by side, in canonical
/// order: what bodies apart from each other are together. The part's `faces`, `edges` and
/// `vertices` number the inputs one after another (input 0's items first).
pub fn compound(records: &[&Brep]) -> Result<Part> {
    let mut all = Brep {
        version: super::VERSION,
        tolerance: records.iter().map(|r| r.tolerance).fold(0.0, f64::max),
        regions: vec![Region { shells: Vec::new(), void: false }],
        shells: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
    };
    for r in records {
        let offset = |n: usize| u32::try_from(n).map_err(|_| Invalid("compound size"));
        let (s0, f0, e0, v0) = (offset(all.shells.len())?, offset(all.faces.len())?, offset(all.edges.len())?, offset(all.vertices.len())?);
        let (infinite, bounded) = r.regions.split_first().ok_or(Invalid("region count"))?;
        if let Some(open) = all.regions.first_mut() {
            open.shells.extend(infinite.shells.iter().map(|s| s + s0));
        }
        all.regions.extend(bounded.iter().map(|region| Region { shells: region.shells.iter().map(|s| s + s0).collect(), void: region.void }));
        all.shells.extend(r.shells.iter().map(|shell| match shell {
            Shell::Faces { faces, wires } => Shell::Faces {
                faces: faces.iter().map(|f| OrientedFace { face: f.face + f0, forward: f.forward }).collect(),
                wires: wires.iter().map(|e| e + e0).collect(),
            },
            Shell::Wire { edges } => Shell::Wire { edges: edges.iter().map(|e| e + e0).collect() },
            Shell::Vertex { vertex } => Shell::Vertex { vertex: vertex + v0 },
        }));
        all.faces.extend(r.faces.iter().map(|f| {
            let mut f = f.clone();
            for l in &mut f.loops {
                match l {
                    Loop::Edges { edges } => edges.iter_mut().for_each(|u| u.edge += e0),
                    Loop::Vertex { vertex } => *vertex += v0,
                }
            }
            f
        }));
        all.edges.extend(r.edges.iter().map(|e| {
            let mut e = e.clone();
            e.vertices = e.vertices.map(|v| v + v0);
            e.uses.iter_mut().for_each(|u| u.face += f0);
            e
        }));
        all.vertices.extend(r.vertices.iter().cloned());
    }
    all.part(&all.material_regions())
}
