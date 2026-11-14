//! CADIR, cadmpeg's neutral CAD model, read into B-rep records ([`solidcore_record::brep`]),
//! schema 4. Each region of a body is one record: its shells' faces on their surfaces, bounded
//! by loops of edges on their curves between vertices, added to an [`Assembly`] and finished as
//! the STEP reader finishes its solids (pcurves, seams, tolerances, the canonical order), so a
//! file read through cadmpeg and the same file written as STEP become the same record.
//!
//! Conventions: CADIR's topology is STEP's. A face's sense compares its normal with its
//! surface's, a coedge's sense compares its traversal with its edge's curve, an edge runs from
//! its start vertex to its end along its curve, and a loop's coedges are in traversal order, an
//! outer loop counter-clockwise about the face's normal. The geometry is in millimetres and
//! radians ([`geometry`] maps each carrier).
//!
//! An edge whose curve is degenerate, or that has none, collapses to its vertex (a cone's apex,
//! a sphere's pole): its uses are left out of their loops, and a loop of nothing else is that
//! vertex. What cannot be read refuses its body's region, with the reason, and the others are
//! still read ([`Bodies::failures`]).
//!
//! Input is hostile: every id is looked up (a missing one refuses its region), and the faces of
//! one model and the edges of one loop are capped.

mod geometry;

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, Face, FaceLoops, Loop, LoopBoundary, Sense, Shell,
};
use solidcore_ops::pcurve::Trims;
pub use solidcore_step::assembly::Solid;
use solidcore_step::assembly::{Assembly, Bound, Kind};
use solidcore_step::topology::{
    MAX_FACES, MAX_FILE_FACES, MAX_FIT_TOLERANCE, MAX_LOOP, MIN_TOLERANCE,
};

use geometry::R;

/// One body region read into a record.
#[derive(Clone, Debug)]
pub struct Read {
    /// The body's id in the model.
    pub body: String,
    /// Which of the body's regions this is, in its order.
    pub region: usize,
    /// The record; each of its faces' origin is that face's index in the model's faces.
    pub solid: Solid,
}

/// A body region that could not be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// The body's id in the model.
    pub body: String,
    /// Which of the body's regions this is, in its order (from 0).
    pub region: usize,
    /// Why the region was not read.
    pub message: String,
}

/// Every body region of a model: those read and those refused.
#[derive(Clone, Debug, Default)]
pub struct Bodies {
    /// The regions read into records, in the model's order.
    pub read: Vec<Read>,
    /// The regions that could not be read, with why.
    pub failures: Vec<Failure>,
}

/// The model's tolerance for its records: CADIR's linear tolerance, within the range the STEP
/// reader takes a file's.
pub fn tolerance(ir: &CadIr) -> f64 {
    ir.tolerances
        .linear
        .get()
        .clamp(MIN_TOLERANCE, MAX_FIT_TOLERANCE)
}

/// Every region of every solid or sheet body of `ir`, read into a record. `trims` makes the
/// pcurves that have no closed form; without it a face with such a use refuses its region. A
/// wire body has no faces and is not read.
pub fn read_bodies(ir: &CadIr, trims: Option<&dyn Trims>) -> Bodies {
    let model = Model::of(ir);
    let mut out = Bodies::default();
    let mut budget = MAX_FILE_FACES;
    for body in &ir.model.bodies {
        let id = body.id.as_str().to_string();
        let kind = match body.kind {
            BodyKind::Solid => Some(Kind::Solid),
            BodyKind::Sheet | BodyKind::General => Some(Kind::Model),
            BodyKind::Wire => None,
        };
        let Some(kind) = kind else { continue };
        for (k, region) in body.regions.iter().enumerate() {
            let name = body.name.clone().unwrap_or_default();
            match model.region(
                region.as_str(),
                kind,
                name,
                tolerance(ir),
                trims,
                &mut budget,
            ) {
                Ok(solid) => out.read.push(Read {
                    body: id.clone(),
                    region: k,
                    solid,
                }),
                Err(message) => out.failures.push(Failure {
                    body: id.clone(),
                    region: k,
                    message,
                }),
            }
        }
    }
    out
}

/// The model's arenas by id.
struct Model<'a> {
    regions: HashMap<&'a str, &'a cadmpeg_ir::topology::Region>,
    shells: HashMap<&'a str, &'a Shell>,
    faces: HashMap<&'a str, (usize, &'a Face)>,
    loops: HashMap<&'a str, &'a Loop>,
    coedges: HashMap<&'a str, &'a Coedge>,
    edges: HashMap<&'a str, &'a Edge>,
    vertices: HashMap<&'a str, &'a cadmpeg_ir::topology::Vertex>,
    points: HashMap<&'a str, &'a cadmpeg_ir::topology::Point>,
    surfaces: HashMap<&'a str, &'a cadmpeg_ir::geometry::Surface>,
    curves: HashMap<&'a str, &'a cadmpeg_ir::geometry::Curve>,
}

impl<'a> Model<'a> {
    fn of(ir: &'a CadIr) -> Model<'a> {
        let m = &ir.model;
        Model {
            regions: m.regions.iter().map(|x| (x.id.as_str(), x)).collect(),
            shells: m.shells.iter().map(|x| (x.id.as_str(), x)).collect(),
            faces: m
                .faces
                .iter()
                .enumerate()
                .map(|(i, x)| (x.id.as_str(), (i, x)))
                .collect(),
            loops: m.loops.iter().map(|x| (x.id.as_str(), x)).collect(),
            coedges: m.coedges.iter().map(|x| (x.id.as_str(), x)).collect(),
            edges: m.edges.iter().map(|x| (x.id.as_str(), x)).collect(),
            vertices: m.vertices.iter().map(|x| (x.id.as_str(), x)).collect(),
            points: m.points.iter().map(|x| (x.id.as_str(), x)).collect(),
            surfaces: m.surfaces.iter().map(|x| (x.id.as_str(), x)).collect(),
            curves: m.curves.iter().map(|x| (x.id.as_str(), x)).collect(),
        }
    }

    /// Region `id` read into a record of `kind` (a solid's first shell its outer one, the rest
    /// its voids).
    fn region(
        &self,
        id: &str,
        kind: Kind,
        name: String,
        tol: f64,
        trims: Option<&dyn Trims>,
        budget: &mut usize,
    ) -> R<Solid> {
        let region = self
            .regions
            .get(id)
            .ok_or_else(|| format!("missing region {id}"))?;
        let kind = match kind {
            Kind::Solid if region.shells.len() > 1 => Kind::Voids,
            k => k,
        };
        let mut b = Builder {
            model: self,
            asm: Assembly::new(tol, trims),
            vertices: HashMap::new(),
            edges: HashMap::new(),
        };
        let mut shells = Vec::new();
        let mut left_out = 0usize;
        for s in &region.shells {
            if shells.len() >= solidcore_record::brep::MAX_SHELLS {
                return Err(format!(
                    "more than {} shells",
                    solidcore_record::brep::MAX_SHELLS
                ));
            }
            let shell = self
                .shells
                .get(s.as_str())
                .ok_or_else(|| format!("missing shell {s}"))?;
            // A shell's wire edges and free vertices have no place in a record of faces yet.
            left_out += shell.wire_edges().len() + shell.free_vertices().len();
            let mut faces = Vec::new();
            for f in shell.faces() {
                if faces.len() >= MAX_FACES {
                    return Err(format!("more than {MAX_FACES} faces"));
                }
                *budget = budget
                    .checked_sub(1)
                    .ok_or_else(|| format!("more than {MAX_FILE_FACES} faces in one model"))?;
                faces.push(b.face(f.as_str())?);
            }
            shells.push(faces);
        }
        let mut solid = b.asm.finish(0, name, shells, kind)?;
        if left_out > 0 {
            solid.notes.push(format!(
                "{left_out} wire edges and free vertices of its shells left out"
            ));
        }
        Ok(solid)
    }
}

/// One region being read: the model, and what of it the assembly holds.
struct Builder<'m, 'a, 't> {
    model: &'m Model<'a>,
    asm: Assembly<'t>,
    /// CADIR vertex → the assembly's.
    vertices: HashMap<&'a str, u32>,
    /// CADIR edge → the assembly's edge, or the vertex an edge collapsed to a point is.
    edges: HashMap<&'a str, Result<u32, u32>>,
}

impl<'a> Builder<'_, 'a, '_> {
    fn vertex(&mut self, id: &'a str) -> R<u32> {
        if let Some(v) = self.vertices.get(id) {
            return Ok(*v);
        }
        let vertex = self
            .model
            .vertices
            .get(id)
            .ok_or_else(|| format!("missing vertex {id}"))?;
        let point = self
            .model
            .points
            .get(vertex.point.as_str())
            .ok_or_else(|| format!("missing point {}", vertex.point))?;
        let p = point.position().get();
        let v = self.asm.add_vertex([p.x, p.y, p.z])?;
        self.vertices.insert(id, v);
        Ok(v)
    }

    /// Edge `id` in the assembly, or the vertex it collapses to. The record edge runs along its
    /// curve; its vertices are the CADIR edge's, at the interval's ends their points are nearer.
    fn edge(&mut self, id: &'a str, origin: u64) -> R<Result<u32, u32>> {
        if let Some(e) = self.edges.get(id) {
            return Ok(*e);
        }
        let edge = self
            .model
            .edges
            .get(id)
            .ok_or_else(|| format!("missing edge {id}"))?;
        let (start, end) = (
            self.vertex(edge.start.as_str())?,
            self.vertex(edge.end.as_str())?,
        );
        let curve = match edge.carrier.curve() {
            Some(c) => {
                let carrier = self
                    .model
                    .curves
                    .get(c.as_str())
                    .ok_or_else(|| format!("missing curve {c}"))?;
                geometry::curve(&carrier.geometry).map_err(|m| format!("edge {id}: {m}"))?
            }
            None => None,
        };
        let made = match curve {
            Some(curve) => {
                let interval = edge.param_range().map(|r| r.get()).ok_or_else(|| {
                    format!("edge {id}: not supported yet: an edge without a parameter range")
                })?;
                Ok(self
                    .asm
                    .add_edge(origin, curve, interval, [start, end])
                    .map_err(|m| format!("edge {id}: {m}"))?
                    .0)
            }
            // Collapsed to a point: its vertices are one.
            None if start == end || same_point(&self.asm, start, end) => Err(start),
            None => return Err(format!("edge {id}: no curve between two vertices apart")),
        };
        self.edges.insert(id, made);
        Ok(made)
    }

    /// Face `id` as its shell uses it: its index in the assembly and its side (always the face's
    /// own normal: CADIR's shells hold faces as they are).
    fn face(&mut self, id: &'a str) -> R<(u32, bool)> {
        let &(index, face) = self
            .model
            .faces
            .get(id)
            .ok_or_else(|| format!("missing face {id}"))?;
        let origin = index as u64;
        let surface = self
            .model
            .surfaces
            .get(face.surface.as_str())
            .ok_or_else(|| format!("face {id}: missing surface {}", face.surface))?;
        let (surface, reversed) =
            geometry::surface(&surface.geometry).map_err(|m| format!("face {id}: {m}"))?;
        let ids: Vec<&'a cadmpeg_ir::ids::LoopId> = face.loops.iter().collect();
        if ids.is_empty() {
            return Err(format!(
                "face {id}: not supported yet: a face with no boundary"
            ));
        }
        let outer = match &face.loops {
            FaceLoops::Classified { outer, .. } => ids.iter().position(|l| *l == outer),
            FaceLoops::Unspecified { .. } => None,
        };
        // Bounds relative to the record surface's natural normal.
        let side = (face.sense == Sense::Forward) != reversed;
        let mut bounds = Vec::new();
        for l in ids {
            let mut bound = self
                .bound(l.as_str(), origin)
                .map_err(|m| format!("face {id}: {m}"))?;
            if !side && let Bound::Edges(uses) = &mut bound {
                uses.reverse();
                for u in uses.iter_mut() {
                    u.1 = !u.1;
                }
            }
            bounds.push(bound);
        }
        Ok((
            self.asm.add_face(origin, surface, bounds, outer, side)?,
            side,
        ))
    }

    /// Loop `id`: its coedges as edge uses in traversal order, those of collapsed edges left
    /// out; a loop of nothing else (or a vertex loop) is its vertex.
    fn bound(&mut self, id: &'a str, origin: u64) -> R<Bound> {
        let lp = self
            .model
            .loops
            .get(id)
            .ok_or_else(|| format!("missing loop {id}"))?;
        let ring = match &lp.boundary {
            LoopBoundary::Vertex { vertex, .. } => {
                return Ok(Bound::Vertex(self.vertex(vertex.as_str())?));
            }
            LoopBoundary::Ring(ring) => ring.coedges(),
        };
        if ring.len() > MAX_LOOP {
            return Err(format!("a loop of {} edges", ring.len()));
        }
        let mut uses = Vec::new();
        let mut pole = None;
        for c in ring {
            let coedge = self
                .model
                .coedges
                .get(c.as_str())
                .ok_or_else(|| format!("missing coedge {c}"))?;
            match self.edge(coedge.edge.as_str(), origin)? {
                // A coedge's sense is against its edge's curve, which the record edge runs along
                // whichever of its vertices the producer called its start.
                Ok(e) => uses.push((e, coedge.sense == Sense::Forward)),
                Err(v) => pole = Some(v),
            }
        }
        match (uses.is_empty(), pole) {
            (false, _) => Ok(Bound::Edges(uses)),
            (true, Some(v)) => Ok(Bound::Vertex(v)),
            (true, None) => Err(format!("loop {id} has no edges")),
        }
    }
}

/// Two of the assembly's vertices at one point to within its tolerance.
fn same_point(asm: &Assembly<'_>, a: u32, b: u32) -> bool {
    match (asm.vertex_point(a), asm.vertex_point(b)) {
        (Some(p), Some(q)) => {
            let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
            solidcore::Vec3::new(d[0], d[1], d[2]).length() <= asm.tolerance()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
