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
//! vertex; two vertices such an edge joins at one point are one. A face with no loops is all of
//! its closed surface, which is read for a sphere. What cannot be read refuses its body's region, with the reason, and the others are
//! still read ([`Bodies::failures`]).
//!
//! Input is hostile: every id is looked up (a missing one refuses its region), and the faces of
//! one model and the edges of one loop are capped. Before a region is read, its shells, each
//! shell's faces and each face's loops are counted against their caps (the STEP reader's), and
//! a face or loop used twice in it is refused, so the work of reading a region grows with the
//! model's size, never with how often it names one face or loop.

mod geometry;

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, Face, FaceLoops, Loop, LoopBoundary, Sense, Shell,
};
use solidcore_ops::pcurve::Trims;
pub use solidcore_step::assembly::Solid;
use solidcore_step::assembly::{
    Assembly, Bound, Kind, hyperbola_curve, parabola_curve, periodic_edge,
};
use solidcore_step::topology::{
    MAX_BOUNDS, MAX_FACES, MAX_FILE_FACES, MAX_FIT_TOLERANCE, MAX_LOOP, MIN_TOLERANCE, named_face,
};

/// Most reads of a refused region without the faces its refusals name, as the STEP reader
/// makes of a refused solid.
const MAX_LEFT_OUT_ROUNDS: usize = 8;

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
    /// Where the body's placement puts the record (column-major 4 × 4, millimetres), when it
    /// moves it. Only a rigid placement is taken: a body placed otherwise is refused.
    pub placement: Option<[[f64; 4]; 4]>,
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
        // A body placed by anything but a rotation and a move is left out with the reason: its
        // geometry would come in at the wrong place or size otherwise.
        let placement = match body
            .transform
            .map(|t| geometry::Similarity::of(geometry::Rows3::rows3(&t)))
        {
            None => None,
            Some(Ok(s)) if s.is_rigid() => s.column_major(),
            refused => {
                let why = match refused {
                    Some(Err(m)) => m,
                    _ => "not supported yet: a placement that scales".to_string(),
                };
                for k in 0..body.regions.len() {
                    out.failures.push(Failure {
                        body: id.clone(),
                        region: k,
                        message: format!("its placement: {why}"),
                    });
                }
                continue;
            }
        };
        for (k, region) in body.regions.iter().enumerate() {
            let name = body.name.clone().unwrap_or_default();
            let none = HashSet::new();
            let read = model
                .region(
                    region.as_str(),
                    kind,
                    name.clone(),
                    model.region_tolerance(region.as_str(), tolerance(ir)),
                    trims,
                    &none,
                    &mut budget,
                )
                .or_else(|message| {
                    model.without_named_faces(
                        region.as_str(),
                        kind,
                        name,
                        model.region_tolerance(region.as_str(), tolerance(ir)),
                        trims,
                        message,
                        &mut budget,
                    )
                });
            match read {
                Ok(solid) => {
                    out.read.push(Read {
                        body: id.clone(),
                        region: k,
                        solid,
                        placement,
                    });
                }
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
    procedural_surfaces: HashMap<&'a str, &'a cadmpeg_ir::geometry::ProceduralSurface>,
    procedural_curves: HashMap<&'a str, &'a cadmpeg_ir::geometry::ProceduralCurve>,
    curves: HashMap<&'a str, &'a cadmpeg_ir::geometry::Curve>,
    /// What a placed carrier is held to: the tolerance, and how far from the origin the model's
    /// points reach.
    fit: geometry::Fit,
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
            procedural_surfaces: m
                .procedural_surfaces
                .iter()
                .map(|x| (x.id.as_str(), x))
                .collect(),
            procedural_curves: m
                .procedural_curves
                .iter()
                .map(|x| (x.id.as_str(), x))
                .collect(),
            curves: m.curves.iter().map(|x| (x.id.as_str(), x)).collect(),
            fit: geometry::Fit {
                tolerance: tolerance(ir),
                extent: m
                    .points
                    .iter()
                    .map(|p| {
                        let p = p.position().get();
                        solidcore::Vec3::new(p.x, p.y, p.z).length()
                    })
                    .fold(0.0, f64::max),
            },
        }
    }

    /// The fit tolerance a procedural surface's construction states for its solved cache (mm), or
    /// 0 for a surface that is not procedural, or whose cache is exact.
    fn surface_fit(&self, g: &cadmpeg_ir::geometry::SurfaceGeometry) -> f64 {
        g.procedural_construction()
            .and_then(|c| self.procedural_surfaces.get(c.as_str()))
            .and_then(|p| p.definition().cache_fit_tolerance())
            .map_or(0.0, |t| t.get())
    }

    /// The fit tolerance a procedural curve's construction states for its solved cache (mm), or
    /// 0 for a curve that is not procedural, or whose cache is exact.
    fn curve_fit(&self, g: &cadmpeg_ir::geometry::CurveGeometry) -> f64 {
        g.procedural_construction()
            .and_then(|c| self.procedural_curves.get(c.as_str()))
            .and_then(|p| p.definition().cache_fit_tolerance())
            .map_or(0.0, |t| t.get())
    }

    /// Whether vertices `a` and `b` are at one point, to within `tol`.
    fn same_point(&self, a: &str, b: &str, tol: f64) -> bool {
        let at = |v: &str| {
            let point = self.points.get(self.vertices.get(v)?.point.as_str())?;
            let p = point.position().get();
            Some(solidcore::Vec3::new(p.x, p.y, p.z))
        };
        matches!((at(a), at(b)), (Some(p), Some(q)) if (p - q).length() <= tol)
    }

    /// A region refused with `refusal`, read again without the face the refusal names, as an
    /// open surface of the faces that remain (the faces that do read, sharing the edges they do):
    /// what the STEP reader makes of a solid it cannot close. A refusal of that naming another face
    /// of the region leaves it out too, up to [`MAX_LEFT_OUT_ROUNDS`] reads; a face is only ever
    /// left out where a refusal names it. The record's notes say why it is open and each face left
    /// out. The refusal stands where it names no face of the region, or no read succeeds.
    #[allow(clippy::too_many_arguments)]
    fn without_named_faces(
        &self,
        id: &str,
        kind: Kind,
        name: String,
        tol: f64,
        trims: Option<&dyn Trims>,
        refusal: String,
        budget: &mut usize,
    ) -> R<Solid> {
        let of_region = self.face_indices(id);
        let open = Kind::Open {
            voids: matches!(kind, Kind::Solid)
                && self.regions.get(id).is_some_and(|r| r.shells.len() > 1),
            outer: true,
        };
        let open = if matches!(kind, Kind::Model) {
            Kind::Model
        } else {
            open
        };
        let mut skip = HashSet::new();
        let mut left_out = Vec::new();
        let mut why = refusal.clone();
        // Each read is charged to the file's face budget, as the first one was: a region read
        // again costs what it reads again, so refused regions cannot read past the budget.
        for _ in 0..MAX_LEFT_OUT_ROUNDS {
            let Some(face) = self
                .named(&why)
                .filter(|f| of_region.contains(f) && !skip.contains(f))
            else {
                break;
            };
            skip.insert(face);
            left_out.push(why.clone());
            if skip.len() >= of_region.len() {
                break;
            }
            match self.region(id, open, name.clone(), tol, trims, &skip, budget) {
                Ok(mut solid) => {
                    solid.notes.insert(
                        0,
                        format!("not closed, so its faces came in as an open surface: {refusal}"),
                    );
                    solid
                        .notes
                        .extend(left_out.iter().map(|m| format!("left out: {m}")));
                    return Ok(solid);
                }
                Err(m) => why = m,
            }
        }
        Err(refusal)
    }

    /// The face a refusal names: by its model index (`face #12`, as the assembly stage names a
    /// face) or by its id in the model (`face step:data:face#81405-…`, as this reader names one).
    fn named(&self, message: &str) -> Option<u64> {
        if let Some(f) = named_face(message) {
            return Some(f);
        }
        message.match_indices("face ").find_map(|(at, m)| {
            let rest = message.get(at + m.len()..)?;
            // A model id has colons in it: the id is the word, less the colon after it.
            let id = rest.split_whitespace().next()?.trim_end_matches(':');
            self.faces.get(id).map(|(i, _)| *i as u64)
        })
    }

    /// The tolerance region `id` is built at: the largest its faces carry (a STEP file states
    /// each part's uncertainty, which CADIR gives every face of the part, overriding the
    /// document's), else `document`, within the range the STEP reader takes a file's. An edge's
    /// or vertex's own tolerance (one off its curve) is that entity's alone, never the region's:
    /// one loose edge does not loosen every face.
    fn region_tolerance(&self, id: &str, document: f64) -> f64 {
        let Some(region) = self.regions.get(id) else {
            return document;
        };
        let most = region
            .shells
            .iter()
            .filter_map(|s| self.shells.get(s.as_str()))
            .flat_map(|shell| shell.faces())
            .filter_map(|f| self.faces.get(f.as_str()))
            .filter_map(|(_, face)| face.tolerance.map(|t| t.get()))
            .filter(|t| t.is_finite())
            .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
        most.map_or(document, |t| t.clamp(MIN_TOLERANCE, MAX_FIT_TOLERANCE))
    }

    /// The model indices of region `id`'s faces (what a refusal names a face by).
    fn face_indices(&self, id: &str) -> HashSet<u64> {
        let Some(region) = self.regions.get(id) else {
            return HashSet::new();
        };
        region
            .shells
            .iter()
            .filter_map(|s| self.shells.get(s.as_str()))
            .flat_map(|shell| shell.faces())
            .filter_map(|f| self.faces.get(f.as_str()).map(|(i, _)| *i as u64))
            .collect()
    }

    /// Region `id` read into a record of `kind` (a solid's first shell its outer one, the rest
    /// its voids), without the faces in `skip` (model indices).
    #[allow(clippy::too_many_arguments)]
    fn region(
        &self,
        id: &str,
        kind: Kind,
        name: String,
        tol: f64,
        trims: Option<&dyn Trims>,
        skip: &HashSet<u64>,
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
            joined: HashMap::new(),
            edges: HashMap::new(),
        };
        self.preflight(&region.shells)?;
        b.join_collapsed(&region.shells);
        let mut shells = Vec::new();
        let mut left_out = 0usize;
        let mut outer_left_out = false;
        for (k, s) in region.shells.iter().enumerate() {
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
                if self
                    .faces
                    .get(f.as_str())
                    .is_some_and(|(i, _)| skip.contains(&(*i as u64)))
                {
                    continue;
                }
                if faces.len() >= MAX_FACES {
                    return Err(format!("more than {MAX_FACES} faces"));
                }
                *budget = budget
                    .checked_sub(1)
                    .ok_or_else(|| format!("more than {MAX_FILE_FACES} faces in one model"))?;
                faces.push(b.face(f.as_str())?);
            }
            // A shell all of whose faces a refusal left out is no shell of the open surface. Where
            // that is the outer shell, the shells left are all cavities, and stay voids.
            if faces.is_empty() && !skip.is_empty() {
                outer_left_out |= k == 0;
            } else {
                shells.push(faces);
            }
        }
        let kind = match kind {
            Kind::Open { voids, .. } if outer_left_out => Kind::Open {
                voids,
                outer: false,
            },
            k => k,
        };
        let mut solid = b.asm.finish(0, name, shells, kind)?;
        if left_out > 0 {
            solid.notes.push(format!(
                "{left_out} wire edges and free vertices of its shells left out"
            ));
        }
        Ok(solid)
    }
}

impl<'a> Model<'a> {
    /// The counts of a region of `shells`, checked before anything of it is read: at most
    /// `MAX_SHELLS` shells, each of at most `MAX_FACES` faces, each of at most `MAX_BOUNDS`
    /// loops, and no face or loop used twice (a face is in one shell once, and a loop bounds one
    /// face once). Every face and loop it walks is then one of the model's, once, so the walk is
    /// no longer than the model's own lists.
    fn preflight(&self, shells: &'a [cadmpeg_ir::ids::ShellId]) -> R<()> {
        if shells.len() > solidcore_record::brep::MAX_SHELLS {
            return Err(format!(
                "more than {} shells",
                solidcore_record::brep::MAX_SHELLS
            ));
        }
        let (mut faces, mut loops) = (
            std::collections::HashSet::new(),
            std::collections::HashSet::new(),
        );
        for s in shells {
            let shell = self
                .shells
                .get(s.as_str())
                .ok_or_else(|| format!("missing shell {s}"))?;
            if shell.faces().len() > MAX_FACES {
                return Err(format!("more than {MAX_FACES} faces"));
            }
            for f in shell.faces() {
                let &(_, face) = self
                    .faces
                    .get(f.as_str())
                    .ok_or_else(|| format!("missing face {f}"))?;
                if !faces.insert(f.as_str()) {
                    return Err(format!("face {f} used more than once"));
                }
                if face.loops.len() > MAX_BOUNDS {
                    return Err(format!("face {f} with {} loops", face.loops.len()));
                }
                for l in face.loops.iter() {
                    if !loops.insert(l.as_str()) {
                        return Err(format!("face {f}: loop {l} used more than once"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// One region being read: the model, and what of it the assembly holds.
struct Builder<'m, 'a, 't> {
    model: &'m Model<'a>,
    asm: Assembly<'t>,
    /// CADIR vertex → the assembly's.
    vertices: HashMap<&'a str, u32>,
    /// CADIR vertex → the vertex it is one with: the other end of a curve-less edge between the
    /// two, at one point ([`Builder::join_collapsed`]).
    joined: HashMap<&'a str, &'a str>,
    /// CADIR edge → the assembly's edge, or the vertex an edge collapsed to a point is.
    edges: HashMap<&'a str, Result<u32, u32>>,
}

impl<'a> Builder<'_, 'a, '_> {
    /// Vertices a curve-less edge of the region's loops (one with no curve, or on a degenerate
    /// one) joins at one point (within the tolerance) are one vertex: the edge collapses to it, and the loop runs on from it, as it
    /// does when the edge's two ends are one vertex already. Vertices apart stay apart, and such
    /// an edge between them is refused when it is read.
    fn join_collapsed(&mut self, shells: &[cadmpeg_ir::ids::ShellId]) {
        let model = self.model;
        let coedges = shells
            .iter()
            .filter_map(|s| model.shells.get(s.as_str()))
            .flat_map(|shell| shell.faces().iter().take(MAX_FACES))
            .filter_map(|f| model.faces.get(f.as_str()))
            .flat_map(|&(_, face)| face.loops.iter())
            .filter_map(|l| match &model.loops.get(l.as_str())?.boundary {
                LoopBoundary::Ring(ring) => Some(ring.coedges().iter().take(MAX_LOOP)),
                LoopBoundary::Vertex { .. } => None,
            })
            .flatten();
        for c in coedges {
            let Some(edge) = model
                .coedges
                .get(c.as_str())
                .and_then(|c| model.edges.get(c.edge.as_str()))
            else {
                continue;
            };
            // An edge collapses with no curve, or on a degenerate one ([`Builder::edge`]).
            let collapses = match edge.carrier.curve() {
                None => true,
                Some(c) => model
                    .curves
                    .get(c.as_str())
                    .is_some_and(|c| geometry::degenerate(&c.geometry)),
            };
            if !collapses {
                continue;
            }
            let (a, b) = (
                self.joined_to(edge.start.as_str()),
                self.joined_to(edge.end.as_str()),
            );
            if a != b && model.same_point(a, b, self.asm.tolerance()) {
                self.joined.insert(b, a);
            }
        }
    }

    /// The vertex `id` is one with ([`Builder::join_collapsed`]): itself, when none. Each vertex
    /// on the way is pointed straight at it, so a later walk from any of them is one step: the
    /// chain the joins build (an edge's end under its start, so many collapsed edges into one
    /// vertex from different ones make a chain as long as their number) is walked once, not
    /// once a join. The vertex found stays the one they all join.
    fn joined_to(&mut self, id: &'a str) -> &'a str {
        let mut root = id;
        for _ in 0..=self.joined.len() {
            match self.joined.get(root) {
                Some(&to) => root = to,
                None => break,
            }
        }
        let mut at = id;
        for _ in 0..=self.joined.len() {
            if at == root {
                break;
            }
            match self.joined.insert(at, root) {
                Some(next) => at = next,
                None => break,
            }
        }
        root
    }

    fn vertex(&mut self, id: &'a str) -> R<u32> {
        let id = self.joined_to(id);
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
        // How far the carrier may lie from the curve it stands for, as its producer states it:
        // a polyline's chordal deflection, or the fit of a procedural curve's cache.
        let mut deviation = 0.0;
        let curve = match edge.carrier.curve() {
            Some(c) => {
                let carrier = self
                    .model
                    .curves
                    .get(c.as_str())
                    .ok_or_else(|| format!("missing curve {c}"))?;
                deviation = geometry::curve_deviation(&carrier.geometry)
                    .max(self.model.curve_fit(&carrier.geometry));
                geometry::curve(&carrier.geometry, self.model.fit)
                    .map_err(|m| format!("edge {id}: {m}"))?
            }
            None => None,
        };
        let made = match curve {
            Some(carrier) => {
                let ends = [start, end];
                let made = match (carrier, edge.param_range().map(|r| r.get())) {
                    (
                        geometry::Carrier::Curve {
                            curve,
                            periodic: false,
                        },
                        Some(interval),
                    ) => self.asm.add_edge(origin, curve, interval, ends),
                    (
                        geometry::Carrier::Curve {
                            curve,
                            periodic: true,
                        },
                        Some(interval),
                    ) => periodic_edge(curve, interval).and_then(|(curve, interval)| {
                        self.asm.add_edge(origin, curve, interval, ends)
                    }),
                    (
                        geometry::Carrier::Parabola {
                            vertex,
                            axis,
                            reference,
                            focal,
                        },
                        Some(interval),
                    ) => self.asm.add_edge(
                        origin,
                        parabola_curve(vertex, axis, reference, focal, interval),
                        interval,
                        ends,
                    ),
                    (
                        geometry::Carrier::Hyperbola {
                            center,
                            axis,
                            reference,
                            radii,
                        },
                        Some(interval),
                    ) => self.asm.add_edge(
                        origin,
                        hyperbola_curve(center, axis, reference, radii, interval),
                        interval,
                        ends,
                    ),
                    // An edge with no interval (a STEP file's, bounded by its vertices alone) is
                    // where its vertices lie on its curve.
                    (geometry::Carrier::Curve { curve, .. }, None) => {
                        self.asm.add_edge_on(origin, curve, ends)
                    }
                    (_, None) => Err(
                        "not supported yet: a parabola or hyperbola edge without a parameter range"
                            .into(),
                    ),
                };
                let e = made.map_err(|m| format!("edge {id}: {m}"))?.0;
                // The edge holds that allowance: its chords or cache are not the curve itself.
                if deviation > 0.0 {
                    self.asm
                        .hold_deviation(&Bound::Edges(vec![(e, true)]), deviation)
                        .map_err(|m| format!("edge {id}: {m}"))?;
                }
                Ok(e)
            }
            // Collapsed to a point: its vertices are one ([`Builder::join_collapsed`]).
            None if start == end => Err(start),
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
        // A procedural surface's solved cache, read as the surface, is a fit within the tolerance
        // its construction states: the face's edges hold it. Its definition, read instead when
        // there is no cache, is exact.
        let mut fit = 0.0;
        let read = match geometry::surface(&surface.geometry, self.model.fit) {
            Ok(Some(read)) => {
                fit = self.model.surface_fit(&surface.geometry);
                Ok(read)
            }
            Ok(None) => self.procedural(&surface.geometry),
            Err(m) => Err(m),
        };
        let (surface, reversed) = read.map_err(|m| format!("face {id}: {m}"))?;
        let ids: Vec<&'a cadmpeg_ir::ids::LoopId> = face.loops.iter().collect();
        // A face with no boundary is all of its closed surface: a sphere is made whole on its
        // poles (`Assembly::add_face`); any other is refused.
        let whole_sphere = matches!(
            surface,
            geometry::FaceSurface::Record(solidcore_record::brep::Surface::Sphere { .. })
        );
        if ids.is_empty() && !whole_sphere {
            return Err(format!(
                "face {id}: not supported yet: a face with no boundary"
            ));
        }
        let outer = match &face.loops {
            FaceLoops::Classified { outer, .. } => ids.iter().position(|l| *l == outer),
            FaceLoops::Unspecified { .. } => None,
        };
        let mut bounds = Vec::new();
        for l in ids {
            bounds.push(
                self.bound(l.as_str(), origin)
                    .map_err(|m| format!("face {id}: {m}"))?,
            );
        }
        if fit > 0.0 {
            for bound in &bounds {
                self.asm
                    .hold_deviation(bound, fit)
                    .map_err(|m| format!("face {id}: its solved cache: {m}"))?;
            }
        }
        // A surface whose record form depends on the face is made from its bounds as they run
        // (about the face's normal), as the STEP reader makes it from its loops.
        let (surface, reversed) = match surface {
            geometry::FaceSurface::Record(s) => (s, reversed),
            geometry::FaceSurface::Swept(definition) => {
                let (s, turned) = self
                    .asm
                    .swept_surface(&definition, &bounds)
                    .map_err(|m| format!("face {id}: {m}"))?;
                (s, reversed != turned)
            }
        };
        // Bounds relative to the record surface's natural normal.
        let side = (face.sense == Sense::Forward) != reversed;
        if !side {
            for bound in &mut bounds {
                if let Bound::Edges(uses) = bound {
                    uses.reverse();
                    for u in uses.iter_mut() {
                        u.1 = !u.1;
                    }
                }
            }
        }
        Ok((
            self.asm.add_face(origin, surface, bounds, outer, side)?,
            side,
        ))
    }

    /// A procedural surface read by its definition: a linear sweep, extrusion or revolution of
    /// its directrix ([`geometry::swept`]). Any other is refused, as one without a solved form.
    fn procedural(
        &mut self,
        g: &cadmpeg_ir::geometry::SurfaceGeometry,
    ) -> R<(geometry::FaceSurface, bool)> {
        let unread = || {
            geometry::unsupported(if g.solved().is_some() {
                "a surface of a kind the producer did not state"
            } else {
                "a procedural surface without its solved form"
            })
        };
        let Some(c) = g.procedural_construction() else {
            return Err(unread());
        };
        let procedural = self
            .model
            .procedural_surfaces
            .get(c.as_str())
            .ok_or_else(|| format!("missing procedural surface {c}"))?;
        let Some(directrix) = geometry::directrix(procedural.definition()) else {
            return Err(unread());
        };
        let curve = self
            .model
            .curves
            .get(directrix.as_str())
            .ok_or_else(|| format!("missing curve {directrix}"))?;
        let profile = geometry::curve(&curve.geometry, self.model.fit)?
            .ok_or_else(|| geometry::unsupported("a degenerate curve swept or revolved"))?;
        let (definition, reversed) = geometry::swept(procedural.definition(), profile)?;
        Ok((geometry::FaceSurface::Swept(definition), reversed))
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
            // A coedge whose own 3D curve stands in for its edge's: a record edge has one curve
            // for both its faces, so the region is refused rather than built on the edge's.
            if coedge.use_curve.is_some() {
                return Err(geometry::unsupported(format!(
                    "coedge {c} on a 3D curve of its own"
                )));
            }
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

#[cfg(test)]
mod tests;
