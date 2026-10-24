//! STEP topology (ISO 10303-42 topology and geometric model entities) into the kernel-neutral
//! B-rep, schema 4 ([`solidcraft_geom::brep`], docs/native-brep.md). Written from the public
//! ISO 10303-42 specification.
//!
//! Each MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS and SHELL_BASED_SURFACE_MODEL becomes one
//! [`Brep`]: a closed shell separates the infinite region from a solid (or, as a void, a solid
//! from its cavity); an open shell lies in the infinite region. A solid that cannot be read is
//! skipped with its reason and the rest of the file is still read.
//!
//! Geometry: LINE, CIRCLE, ELLIPSE, B-spline curves and POLYLINE map to the schema's curve kinds
//! (SURFACE_CURVE and its subtypes by their 3D curve, TRIMMED_CURVE by its basis: the edge's
//! vertices bound it); PLANE, CYLINDRICAL_, CONICAL_, SPHERICAL_ and (ring) TOROIDAL_SURFACE and
//! B-spline surfaces map to the surface kinds (RECTANGULAR_TRIMMED_SURFACE by its basis). Anything
//! else refuses its solid ("not supported yet: …").
//!
//! Orientation: an EDGE_CURVE's interval is the parameters of its vertices on its curve, from
//! min to max; the schema edge runs with the STEP edge or against it (`same_sense` .F. on a
//! closed curve, or vertices in decreasing parameter order on an open one), and each oriented
//! edge's `forward` says how the loop runs along the schema edge. Loops are stored relative to
//! the surface's natural normal: FACE_BOUND orientation and ADVANCED_FACE same_sense both
//! reverse a loop, and same_sense (with ORIENTED_FACE and ORIENTED_CLOSED_SHELL orientation)
//! gives the oriented face's side.
//!
//! Tolerances: the representation context's distance uncertainty (default [`DEFAULT_TOLERANCE`]
//! when the file has none, never below [`MIN_TOLERANCE`]) is the brep's tolerance and every
//! vertex's and edge's to start with. A vertex measurably off its curve or surface gets that
//! distance (with margin) as its tolerance; a face whose edges lie off its surface gets its
//! pcurves fitted at a looser tolerance, which its edges then carry.
//!
//! Pcurves are the closed forms the 3D edges have on our surfaces ([`pcurve::exact_pcurves`]); a
//! use with none gets its pcurve from the kernel ([`read_solids_with`], SMLib's own trims). The
//! file's own PCURVE entities are not used.
//!
//! Input is hostile: every reference is checked, nesting and counts are capped, and the work of
//! a whole file is bounded (shared shells count each time a solid uses them).

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::pcurve::{self, EdgeUse, TrimEdge, TrimFace, TrimUse, Trims};
use solidcraft_geom::brep::{
    self, Brep, Curve as BCurve, Curve2, Edge, EdgeUseRef, Face, Frame, Loop, Natural, OrientedEdge, OrientedFace, Region, Shell,
    Surface as BSurface, Vertex,
};

use crate::geom::{BSplineCurve, BSplineSurface, Curve, P3, Placement, dot, scale, sub};
use crate::p21::{Entity, Exchange, Param};
use crate::read::{R, Reader};
use crate::units;

mod seams;
mod swept;

/// Tolerance (mm) when the file states no uncertainty.
pub const DEFAULT_TOLERANCE: f64 = 1e-3;
/// The least tolerance (mm) a record gets, whatever the file states.
pub const MIN_TOLERANCE: f64 = 1e-7;
/// The loosest tolerance (mm) a face's pcurves are fitted at before the solid is refused.
pub const MAX_FIT_TOLERANCE: f64 = 1.0;
/// Most faces in one solid.
pub const MAX_FACES: usize = 500_000;
/// Most edges in one loop, and in all of one face's loops.
pub const MAX_LOOP: usize = 100_000;
/// Most bounds of one face.
pub const MAX_BOUNDS: usize = 10_000;
/// Most faces read in one file, counting a shell again for each solid that uses it.
pub const MAX_FILE_FACES: usize = 5_000_000;
/// Most solid items read from one file.
pub const MAX_SOLIDS: usize = 100_000;
/// Deepest nesting of ORIENTED_EDGE in ORIENTED_EDGE, ORIENTED_FACE in ORIENTED_FACE.
const MAX_NESTING: usize = 4;
/// Most control points in one direction of a schema spline.
const MAX_SPLINE_POINTS: usize = 4096;
/// Highest schema spline degree.
const MAX_SPLINE_DEGREE: usize = 16;
/// Samples along a curve when projecting a vertex onto it.
const MAX_SAMPLES: usize = 4096;
/// How far (mm) a vertex may lie from its curve before the edge is refused.
const MAX_VERTEX_GAP: f64 = 1.0;

/// A solid (or sheet) read from the file.
#[derive(Clone, Debug)]
pub struct Solid {
    /// The STEP item (MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS or SHELL_BASED_SURFACE_MODEL).
    pub item: u64,
    /// The item's own name, else its product's.
    pub name: String,
    /// The name of the product whose shape holds the item, when the file says.
    pub product: Option<String>,
    /// In the coordinates of the item's own representation (assembly placements not applied).
    pub brep: Brep,
    /// The file's face entity (ADVANCED_FACE, FACE_SURFACE) of each of the record's faces, in
    /// the record's face order.
    pub face_origin: Vec<u64>,
    /// No closed shell: a surface model of open shells only.
    pub sheet: bool,
    /// What was repaired or noted on the way (faces whose same_sense disagreed with their
    /// loops, edges whose same_sense disagreed with their vertices, raised tolerances).
    pub notes: Vec<String>,
    /// The record faces turned over because their loops disagree with their same_sense (their
    /// outer loop runs clockwise about the normal the file gives them), in increasing order.
    pub turned: Vec<u32>,
}

/// A solid item that could not be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub item: u64,
    pub message: String,
}

/// Every solid of a file: those read and those refused.
#[derive(Clone, Debug, Default)]
pub struct Solids {
    pub solids: Vec<Solid>,
    pub failures: Vec<Failure>,
}

fn entity(ex: &Exchange, id: u64) -> R<&Entity> {
    ex.get(id).ok_or_else(|| format!("missing entity #{id}"))
}

fn rid(p: Option<&Param>, what: &str) -> R<u64> {
    p.and_then(Param::as_ref_id).ok_or_else(|| format!("{what}: expected a reference"))
}

fn refs(p: Option<&Param>, what: &str) -> R<Vec<u64>> {
    let list = p.and_then(Param::as_list).ok_or_else(|| format!("{what}: expected a list"))?;
    list.iter().map(|x| x.as_ref_id().ok_or_else(|| format!("{what}: expected references"))).collect()
}

fn flag(p: Option<&Param>, what: &str) -> R<bool> {
    p.and_then(Param::as_bool).ok_or_else(|| format!("{what}: expected a boolean"))
}

fn unsupported(what: impl std::fmt::Display) -> String {
    format!("not supported yet: {what}")
}

fn index(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| "too many items".to_string())
}

fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Items and context of a representation entity (plain or complex).
fn representation(e: &Entity) -> Option<(Vec<u64>, Option<u64>)> {
    let p = if e.is_complex() {
        e.record("REPRESENTATION")
            .or_else(|| e.records.iter().find(|r| r.name.ends_with("REPRESENTATION") && r.params.len() >= 3).map(|r| r.params.as_slice()))?
    } else if e.name().ends_with("REPRESENTATION") {
        e.params()
    } else {
        return None;
    };
    let items = p.get(1)?.as_list()?.iter().filter_map(Param::as_ref_id).collect();
    Some((items, p.get(2).and_then(Param::as_ref_id)))
}

const SOLID_ITEMS: [&str; 4] = ["MANIFOLD_SOLID_BREP", "BREP_WITH_VOIDS", "SHELL_BASED_SURFACE_MODEL", "FACETED_BREP"];

/// Where each solid item sits: its representation's context, and the product that representation
/// (or one related to it) is the shape of.
struct Index {
    context: HashMap<u64, u64>,
    product: HashMap<u64, String>,
}

impl Index {
    fn new(ex: &Exchange) -> Index {
        let mut ids: Vec<u64> = ex.entities.keys().copied().collect();
        ids.sort_unstable();
        let mut context = HashMap::new();
        let mut rep_of: HashMap<u64, u64> = HashMap::new();
        let mut shape_of: HashMap<u64, u64> = HashMap::new(); // representation → PRODUCT_DEFINITION_SHAPE
        let mut links: HashMap<u64, Vec<u64>> = HashMap::new();
        for &id in &ids {
            let Some(e) = ex.get(id) else { continue };
            if let Some((items, ctx)) = representation(e) {
                for item in items {
                    if let Some(c) = ctx {
                        context.entry(item).or_insert(c);
                    }
                    rep_of.entry(item).or_insert(id);
                }
            }
            let p = e.params();
            let rf = |i: usize| p.get(i).and_then(Param::as_ref_id);
            match e.name() {
                "SHAPE_DEFINITION_REPRESENTATION" => {
                    if let (Some(d), Some(r)) = (rf(0), rf(1)) {
                        shape_of.entry(r).or_insert(d);
                    }
                }
                "SHAPE_REPRESENTATION_RELATIONSHIP" | "REPRESENTATION_RELATIONSHIP" if !e.is_complex() => {
                    if let (Some(a), Some(b)) = (rf(2), rf(3)) {
                        links.entry(a).or_default().push(b);
                        links.entry(b).or_default().push(a);
                    }
                }
                _ => {}
            }
        }
        let name_of = |pds: u64| -> Option<String> {
            let pd = ex.get(pds)?.params().get(2)?.as_ref_id()?;
            let pdf = ex.get(pd)?.params().get(2)?.as_ref_id()?;
            let product = ex.get(pdf)?.params().get(2)?.as_ref_id()?;
            let p = ex.get(product)?;
            (p.name() == "PRODUCT").then(|| p.params().get(1).or(p.params().first())).flatten()?.as_str().map(str::to_string)
        };
        let mut product = HashMap::new();
        for (&item, &rep) in &rep_of {
            if !ex.get(item).is_some_and(|e| SOLID_ITEMS.contains(&e.name())) {
                continue;
            }
            let direct = shape_of.get(&rep).copied();
            let linked = || links.get(&rep).into_iter().flatten().find_map(|r| shape_of.get(r).copied());
            if let Some(name) = direct.or_else(linked).and_then(name_of) {
                product.insert(item, name);
            }
        }
        Index { context, product }
    }
}

/// Read every solid of the file. A solid that cannot be read is listed in
/// [`Solids::failures`] with its reason; the others are still read. A face's use with no
/// closed-form pcurve refuses its solid: [`read_solids_with`] has a kernel make those.
pub fn read_solids(ex: &Exchange) -> Solids {
    read_solids_in(ex, None)
}

/// [`read_solids`], with `trims` making the pcurves of uses that have no closed form.
pub fn read_solids_with(ex: &Exchange, trims: &dyn Trims) -> Solids {
    read_solids_in(ex, Some(trims))
}

fn read_solids_in(ex: &Exchange, trims: Option<&dyn Trims>) -> Solids {
    let mut items: Vec<u64> = ex.entities.iter().filter(|(_, e)| SOLID_ITEMS.contains(&e.name())).map(|(id, _)| *id).collect();
    items.sort_unstable();
    let index = Index::new(ex);
    let mut out = Solids::default();
    let mut budget = MAX_FILE_FACES;
    let mut readers: HashMap<u64, (Reader<'_>, units::Context)> = HashMap::new();
    for (n, item) in items.into_iter().enumerate() {
        if n >= MAX_SOLIDS {
            out.failures.push(Failure { item, message: format!("more than {MAX_SOLIDS} solids in one file") });
            continue;
        }
        let ctx = index.context.get(&item).copied();
        let key = ctx.unwrap_or(0);
        let (reader, context) = readers.entry(key).or_insert_with(|| {
            let c = ctx.map(|c| units::context(ex, c)).unwrap_or_default();
            (Reader::new(ex, c.units), c)
        });
        let context = *context;
        match read_with(ex, reader, item, context, &mut budget, trims) {
            Ok(mut s) => {
                s.product = index.product.get(&item).cloned();
                if s.name.is_empty()
                    && let Some(p) = &s.product
                {
                    s.name = p.clone();
                }
                out.solids.push(s);
            }
            Err(message) => out.failures.push(Failure { item, message }),
        }
    }
    out
}

/// Read one solid item in the given context.
pub fn read_solid(ex: &Exchange, item: u64, context: units::Context) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, None)
}

/// [`read_solid`], with `trims` making the pcurves of uses that have no closed form.
pub fn read_solid_with(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims))
}

/// The base tolerance of a context.
pub fn tolerance(context: &units::Context) -> f64 {
    context.uncertainty.unwrap_or(DEFAULT_TOLERANCE).clamp(MIN_TOLERANCE, MAX_FIT_TOLERANCE)
}

fn read_with<'a>(
    ex: &'a Exchange,
    reader: &mut Reader<'a>,
    item: u64,
    context: units::Context,
    budget: &mut usize,
    trims: Option<&dyn Trims>,
) -> R<Solid> {
    let e = entity(ex, item)?;
    let p = e.params();
    let name = p.first().and_then(Param::as_str).unwrap_or("").to_string();
    let mut b = Builder::new(ex, reader, tolerance(&context), budget, trims);
    let (shells, kind) = match e.name() {
        "MANIFOLD_SOLID_BREP" => (vec![b.shell(rid(p.get(1), "outer shell")?)?], Kind::Solid),
        "BREP_WITH_VOIDS" => {
            let mut shells = vec![b.shell(rid(p.get(1), "outer shell")?)?];
            let voids = refs(p.get(2), "voids")?;
            if voids.len() >= brep::MAX_SHELLS {
                return Err(format!("{} voids", voids.len()));
            }
            for v in voids {
                shells.push(b.shell(v)?);
            }
            (shells, Kind::Voids)
        }
        "SHELL_BASED_SURFACE_MODEL" => {
            let ids = refs(p.get(1), "surface model shells")?;
            if ids.is_empty() || ids.len() > brep::MAX_SHELLS {
                return Err(format!("a surface model with {} shells", ids.len()));
            }
            let mut shells = Vec::with_capacity(ids.len());
            for s in ids {
                shells.push(b.shell(s)?);
            }
            (shells, Kind::Model)
        }
        n => return Err(unsupported(n)),
    };
    b.finish(item, name, shells, kind)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Solid,
    Voids,
    Model,
}

/// A curve the edge can be read on, and how its parameter relates to its vertices.
struct EdgeB {
    curve: BCurve,
    interval: [f64; 2],
    vertices: [u32; 2],
    tolerance: f64,
    /// The curve's NURBS form and its interval there (`form::edge`), set before the pcurves.
    form: Option<(BCurve, [f64; 2])>,
}

enum LoopB {
    Edges(Vec<(u32, bool)>),
    Vertex(u32),
}

struct FaceB {
    step: u64,
    surface: BSurface,
    /// Relative to the surface's natural normal.
    loops: Vec<LoopB>,
    /// The index in `loops` of the FACE_OUTER_BOUND, if one is marked.
    outer: Option<usize>,
    /// The face's normal is the surface's natural normal.
    forward: bool,
}

struct Builder<'b, 'a> {
    ex: &'a Exchange,
    reader: &'b mut Reader<'a>,
    tol: f64,
    budget: &'b mut usize,
    /// Makes the pcurves of uses that have no closed form; without it such a face is refused.
    trims: Option<&'b dyn Trims>,
    vertex_ids: HashMap<u64, u32>,
    vertices: Vec<Vertex>,
    /// STEP edge → schema edge, and whether the schema edge runs with the STEP edge.
    edge_ids: HashMap<u64, (u32, bool)>,
    edges: Vec<EdgeB>,
    face_ids: HashSet<u64>,
    faces: Vec<FaceB>,
    sense_mismatches: usize,
}

impl<'b, 'a> Builder<'b, 'a> {
    fn new(ex: &'a Exchange, reader: &'b mut Reader<'a>, tol: f64, budget: &'b mut usize, trims: Option<&'b dyn Trims>) -> Self {
        Builder {
            ex,
            reader,
            tol,
            budget,
            trims,
            vertex_ids: HashMap::new(),
            vertices: Vec::new(),
            edge_ids: HashMap::new(),
            edges: Vec::new(),
            face_ids: HashSet::new(),
            faces: Vec::new(),
            sense_mismatches: 0,
        }
    }

    fn vertex(&mut self, id: u64) -> R<u32> {
        if let Some(v) = self.vertex_ids.get(&id) {
            return Ok(*v);
        }
        let e = entity(self.ex, id)?;
        if e.name() != "VERTEX_POINT" {
            return Err(format!("#{id}: {}", unsupported(format!("vertex {}", e.name()))));
        }
        let point = self.reader.point(rid(e.params().get(1), "vertex point")?)?;
        let i = index(self.vertices.len())?;
        self.vertices.push(Vertex { point, tolerance: self.tol });
        self.vertex_ids.insert(id, i);
        Ok(i)
    }

    fn point(&self, v: u32) -> R<P3> {
        self.vertices.get(v as usize).map(|v| v.point).ok_or_else(|| "vertex reference".to_string())
    }

    /// A vertex `gap` mm from where its edge's curve puts it: refused if far off, else its
    /// tolerance covers the gap.
    fn raise_vertex(&mut self, v: u32, gap: f64, edge: u64) -> R<()> {
        if !(gap <= MAX_VERTEX_GAP) {
            return Err(format!("edge #{edge}: a vertex lies {gap} mm off its curve"));
        }
        if let Some(x) = self.vertices.get_mut(v as usize) {
            x.tolerance = x.tolerance.max(1.5 * gap);
        }
        Ok(())
    }

    /// The schema edge of an EDGE_CURVE, and whether it runs with the STEP edge.
    fn edge(&mut self, id: u64) -> R<(u32, bool)> {
        if let Some(e) = self.edge_ids.get(&id) {
            return Ok(*e);
        }
        let e = entity(self.ex, id)?;
        if e.name() != "EDGE_CURVE" {
            return Err(format!("#{id}: {}", unsupported(format!("edge {}", e.name()))));
        }
        let p = e.params();
        let (vs_id, ve_id) = (rid(p.get(1), "edge start")?, rid(p.get(2), "edge end")?);
        let (vs, ve) = (self.vertex(vs_id)?, self.vertex(ve_id)?);
        let same_sense = flag(p.get(4), "edge same_sense")?;
        let curve = self.edge_geometry(rid(p.get(3), "edge geometry")?, 0)?;
        let (ps, pe) = (self.point(vs)?, self.point(ve)?);
        let closed = vs == ve;
        let near = closed || v3(ps).dist(v3(pe)) <= self.tol;
        let mapped = map_edge(&curve, ps, pe, same_sense, closed, near, self.tol).map_err(|m| format!("edge #{id}: {m}"))?;
        if mapped.sense_mismatch {
            self.sense_mismatches += 1;
        }
        let vertices = if mapped.with_step { [vs, ve] } else { [ve, vs] };
        // The gaps as the record will be read: its curve at its interval's ends.
        let eval = CurveEval::new(&mapped.curve).map_err(|m| format!("edge #{id}: {m}"))?;
        for (t, v) in mapped.interval.into_iter().zip(vertices) {
            let at = eval.point(t).ok_or_else(|| format!("edge #{id}: no point at {t}"))?;
            let gap = at.dist(v3(self.point(v)?));
            self.raise_vertex(v, gap, id)?;
        }
        let i = index(self.edges.len())?;
        self.edges.push(EdgeB { curve: mapped.curve, interval: mapped.interval, vertices, tolerance: self.tol, form: None });
        self.edge_ids.insert(id, (i, mapped.with_step));
        Ok((i, mapped.with_step))
    }

    /// An edge's curve: SURFACE_CURVE and its subtypes by their 3D curve, TRIMMED_CURVE by its
    /// basis (the edge's vertices bound it).
    fn edge_geometry(&mut self, id: u64, depth: usize) -> R<Curve> {
        if depth > MAX_NESTING {
            return Err("edge geometry nested too deeply".into());
        }
        let e = entity(self.ex, id)?;
        if !e.is_complex() {
            match e.name() {
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                    return self.edge_geometry(rid(e.params().get(1), "surface curve 3D curve")?, depth + 1);
                }
                "TRIMMED_CURVE" => return self.edge_geometry(rid(e.params().get(1), "trimmed curve basis")?, depth + 1),
                _ => {}
            }
        }
        let c = self.reader.curve(id)?;
        let mut c: &Curve = &c;
        // Already read as part of something else: unwrap the same way.
        for _ in 0..=MAX_NESTING {
            match c {
                Curve::SurfaceCurve { curve_3d, .. } => c = curve_3d,
                Curve::Trimmed { basis, .. } => c = basis,
                _ => return Ok(c.clone()),
            }
        }
        Err("edge geometry nested too deeply".into())
    }

    /// An ORIENTED_EDGE (or a bare EDGE_CURVE) as a loop uses it: its schema edge and whether
    /// the loop runs along it.
    fn oriented_edge(&mut self, id: u64, depth: usize) -> R<(u32, bool)> {
        if depth > MAX_NESTING {
            return Err("oriented edges nested too deeply".into());
        }
        let e = entity(self.ex, id)?;
        if e.name() == "ORIENTED_EDGE" {
            let p = e.params();
            let (edge, along) = self.oriented_edge(rid(p.get(3), "oriented edge element")?, depth + 1)?;
            let orientation = flag(p.get(4), "oriented edge orientation")?;
            return Ok((edge, along == orientation));
        }
        let (edge, with_step) = self.edge(id)?;
        Ok((edge, with_step))
    }

    fn ends(&self, (edge, forward): (u32, bool)) -> R<[u32; 2]> {
        let e = self.edges.get(edge as usize).ok_or("edge reference")?;
        let [a, z] = e.vertices;
        Ok(if forward { [a, z] } else { [z, a] })
    }

    /// A face bound: its loop as the face uses it (bound orientation applied), and whether it
    /// is the FACE_OUTER_BOUND.
    fn bound(&mut self, id: u64, edges_so_far: &mut usize) -> R<(LoopB, bool)> {
        let b = entity(self.ex, id)?;
        let outer = match b.name() {
            "FACE_OUTER_BOUND" => true,
            "FACE_BOUND" => false,
            n => return Err(format!("#{id}: {}", unsupported(format!("face bound {n}")))),
        };
        let p = b.params();
        let lp_id = rid(p.get(1), "face bound loop")?;
        let orientation = flag(p.get(2), "face bound orientation")?;
        let lp = entity(self.ex, lp_id)?;
        match lp.name() {
            "EDGE_LOOP" => {
                let ids = refs(lp.params().get(1), "edge loop")?;
                *edges_so_far = edges_so_far.saturating_add(ids.len());
                if ids.is_empty() || *edges_so_far > MAX_LOOP {
                    return Err(format!("edge loop #{lp_id} with {} edges", ids.len()));
                }
                let mut uses = Vec::with_capacity(ids.len());
                for oe in ids {
                    uses.push(self.oriented_edge(oe, 0)?);
                }
                // Connected and closed, as the loop is written.
                let mut last = None;
                for u in &uses {
                    let [s, e] = self.ends(*u)?;
                    if last.is_some_and(|l| l != s) {
                        return Err(format!("edge loop #{lp_id} is not connected"));
                    }
                    last = Some(e);
                }
                if let (Some(first), Some(last)) = (uses.first(), last)
                    && self.ends(*first)?[0] != last
                {
                    return Err(format!("edge loop #{lp_id} is not closed"));
                }
                if !orientation {
                    reverse(&mut uses);
                }
                Ok((LoopB::Edges(uses), outer))
            }
            "VERTEX_LOOP" => Ok((LoopB::Vertex(self.vertex(rid(lp.params().get(1), "vertex loop")?)?), outer)),
            n => Err(format!("#{lp_id}: {}", unsupported(format!("loop {n}")))),
        }
    }

    /// A face as a shell uses it: its index and whether its normal there is its surface's
    /// natural normal.
    fn face(&mut self, id: u64, depth: usize) -> R<(u32, bool)> {
        if depth > MAX_NESTING {
            return Err("oriented faces nested too deeply".into());
        }
        let e = entity(self.ex, id)?;
        let p = e.params();
        if e.name() == "ORIENTED_FACE" {
            let (f, forward) = self.face(rid(p.get(2), "oriented face element")?, depth + 1)?;
            return Ok((f, forward == flag(p.get(3), "oriented face orientation")?));
        }
        if !matches!(e.name(), "ADVANCED_FACE" | "FACE_SURFACE") {
            return Err(format!("#{id}: {}", unsupported(format!("face {}", e.name()))));
        }
        if !self.face_ids.insert(id) {
            return Err(format!("face #{id} is used twice"));
        }
        *self.budget = self.budget.checked_sub(1).ok_or_else(|| format!("more than {MAX_FILE_FACES} faces in one file"))?;
        if self.faces.len() >= MAX_FACES {
            return Err(format!("more than {MAX_FACES} faces"));
        }
        let step_surface = self.reader.surface(rid(p.get(2), "face geometry")?).map_err(|m| format!("face #{id}: {m}"))?;
        let same_sense = flag(p.get(3), "face same_sense")?;
        let bounds = refs(p.get(1), "face bounds")?;
        if bounds.len() > MAX_BOUNDS {
            return Err(format!("face #{id} with {} bounds", bounds.len()));
        }
        let mut loops = Vec::with_capacity(bounds.len());
        let mut outer = None;
        let mut edges = 0;
        for b in bounds {
            let (l, is_outer) = self.bound(b, &mut edges).map_err(|m| format!("face #{id}: {m}"))?;
            if is_outer && outer.is_none() {
                outer = Some(loops.len());
            }
            loops.push(l);
        }
        if !loops.iter().any(|l| matches!(l, LoopB::Edges(_))) {
            return Err(format!("face #{id}: {}", unsupported("a face without an edge loop")));
        }
        let extent = self.extent(&loops)?;
        let (surface, reversed) = swept::map_surface(&step_surface, &extent, self.tol).map_err(|m| format!("face #{id}: {m}"))?;
        // Loops relative to the record's natural normal: the face's normal is STEP's surface
        // normal when same_sense, and the record's is STEP's unless `reversed`.
        let sense = same_sense != reversed;
        if !sense {
            for l in &mut loops {
                if let LoopB::Edges(uses) = l {
                    reverse(uses);
                }
            }
        }
        let i = index(self.faces.len())?;
        self.faces.push(FaceB { step: id, surface, loops, outer, forward: sense });
        Ok((i, sense))
    }

    /// Points along a face's loops (each edge at nine points, and its vertex loops), and along
    /// the edges its loops use twice (its seams).
    fn extent(&self, loops: &[LoopB]) -> R<swept::Extent> {
        let mut ext = swept::Extent::default();
        let mut count: HashMap<u32, usize> = HashMap::new();
        for l in loops {
            match l {
                LoopB::Edges(es) => {
                    for (e, _) in es {
                        *count.entry(*e).or_default() += 1;
                    }
                }
                LoopB::Vertex(v) => ext.points.push(v3(self.point(*v)?)),
            }
        }
        let mut edges: Vec<u32> = count.keys().copied().collect();
        edges.sort_unstable();
        for e in edges {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            for k in 0..=8 {
                let p = c.point(lo + (hi - lo) * f64::from(k) / 8.0).ok_or("edge point")?;
                ext.points.push(p);
                if count.get(&e).is_some_and(|n| *n > 1) {
                    ext.seam.push(p);
                }
            }
        }
        Ok(ext)
    }

    fn shell(&mut self, id: u64) -> R<Vec<(u32, bool)>> {
        let e = entity(self.ex, id)?;
        let p = e.params();
        let (faces, flip) = match e.name() {
            "CLOSED_SHELL" | "OPEN_SHELL" => (refs(p.get(1), "shell faces")?, false),
            "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
                let inner = rid(p.get(2), "oriented shell element")?;
                let inner_e = entity(self.ex, inner)?;
                if !matches!(inner_e.name(), "CLOSED_SHELL" | "OPEN_SHELL") {
                    return Err(format!("#{inner}: {}", unsupported(format!("shell {}", inner_e.name()))));
                }
                (refs(inner_e.params().get(1), "shell faces")?, !flag(p.get(3), "oriented shell orientation")?)
            }
            n => return Err(format!("#{id}: {}", unsupported(format!("shell {n}")))),
        };
        if faces.is_empty() {
            return Err(format!("shell #{id} has no faces"));
        }
        if faces.len() > MAX_FACES {
            return Err(format!("shell #{id} with {} faces", faces.len()));
        }
        let mut out = Vec::with_capacity(faces.len());
        for f in faces {
            let (face, forward) = self.face(f, 0)?;
            out.push((face, forward != flip));
        }
        Ok(out)
    }

    /// Pcurves, outer loops, domains and the canonical order: the finished record.
    fn finish(mut self, item: u64, name: String, shells: Vec<Vec<(u32, bool)>>, kind: Kind) -> R<Solid> {
        let mut notes = Vec::new();
        let seams = self.add_seams()?;
        if seams > 0 {
            notes.push(format!("{seams} seam edges added to faces that wind around their surface"));
        }
        if self.sense_mismatches > 0 {
            notes.push(format!("{} edges run against their same_sense (read by their vertices)", self.sense_mismatches));
        }
        // Each edge's label and NURBS form: a circle about a revolved face's axis labelled on that
        // face's frame, so its form lies on the face's quarter grid.
        let mut frames: Vec<Vec<brep::Frame>> = vec![Vec::new(); self.edges.len()];
        for f in &self.faces {
            let Some(frame) = brep::form::frame_of(&f.surface) else { continue };
            for l in &f.loops {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        if let Some(slot) = frames.get_mut(*e as usize) {
                            slot.push(frame.clone());
                        }
                    }
                }
            }
        }
        for (edge, fr) in self.edges.iter_mut().zip(&frames) {
            let (label, interval, form) =
                brep::form::edge(&edge.curve, edge.interval, fr, self.tol).ok_or_else(|| unsupported("an arc of more than one turn"))?;
            (edge.curve, edge.interval, edge.form) = (label, interval, form);
        }
        // Each face's pcurves; a face whose loops disagree with its same_sense is turned over.
        let mut pcs: Vec<Vec<Vec<Curve2>>> = Vec::with_capacity(self.faces.len());
        let mut face_forms: Vec<Option<BSurface>> = Vec::with_capacity(self.faces.len());
        let mut domains = Vec::with_capacity(self.faces.len());
        let mut turned = vec![false; self.faces.len()];
        let (mut failed, mut first_failure) = (0usize, None);
        for fi in 0..self.faces.len() {
            match self.face_pcurves(fi) {
                Ok((pc, domain, flipped, form)) => {
                    face_forms.push(form);
                    if flipped && let Some(t) = turned.get_mut(fi) {
                        *t = true;
                    }
                    pcs.push(pc);
                    domains.push(domain);
                }
                Err(m) => {
                    failed += 1;
                    first_failure.get_or_insert_with(|| format!("face #{}: {m}", self.faces.get(fi).map_or(0, |f| f.step)));
                    pcs.push(Vec::new());
                    domains.push([[0.0; 2]; 2]);
                    face_forms.push(None);
                }
            }
        }
        if let Some(first) = first_failure {
            return Err(format!("pcurves failed on {failed} of {} faces; first: {first}", self.faces.len()));
        }
        let flipped = turned.iter().filter(|t| **t).count();
        if flipped > 0 {
            notes.push(format!("{flipped} faces turned over: their loops disagree with their same_sense"));
        }
        let shells: Vec<Vec<(u32, bool)>> = shells
            .into_iter()
            .map(|s| s.into_iter().map(|(f, fwd)| (f, if turned.get(f as usize).copied().unwrap_or(false) { !fwd } else { fwd })).collect())
            .collect();

        // Regions: which shells are closed (every edge used at least twice among its faces).
        let closed: Vec<bool> = shells.iter().map(|s| self.closed(s)).collect();
        let mut regions = vec![Region { shells: Vec::new(), void: false }];
        match kind {
            Kind::Solid | Kind::Voids => {
                if let Some(k) = closed.iter().position(|c| !c) {
                    return Err(format!("{} shell is not closed", if k == 0 { "the outer" } else { "a void" }));
                }
                regions[0].shells.push(0);
                regions.push(Region { shells: (0..index(shells.len())?).collect(), void: false });
                for v in 1..shells.len() {
                    regions.push(Region { shells: vec![index(v)?], void: true });
                }
            }
            Kind::Model => {
                regions[0].shells = (0..index(shells.len())?).collect();
                for (k, c) in closed.iter().enumerate() {
                    if *c {
                        regions.push(Region { shells: vec![index(k)?], void: false });
                    }
                }
            }
        }
        let sheet = !closed.iter().any(|c| *c);

        // Canonical order: faces walking regions → shells → oriented faces.
        let nf = self.faces.len();
        let mut face_new = vec![u32::MAX; nf];
        let mut face_order = Vec::with_capacity(nf);
        let mut direction = vec![true; nf];
        let mut seen_shell = vec![false; shells.len()];
        for r in &regions {
            for &s in &r.shells {
                let Some(seen) = seen_shell.get_mut(s as usize) else { continue };
                if std::mem::replace(seen, true) {
                    continue;
                }
                for &(f, fwd) in shells.get(s as usize).into_iter().flatten() {
                    let slot = face_new.get_mut(f as usize).ok_or("face reference")?;
                    if *slot == u32::MAX {
                        *slot = index(face_order.len())?;
                        face_order.push(f);
                        if let Some(d) = direction.get_mut(f as usize) {
                            *d = fwd;
                        }
                    }
                }
            }
        }
        // Edges walking faces → loops (outer first) → oriented edges, backwards in a face whose
        // first side is reversed; their uses in that order.
        let outer_first: Vec<Vec<usize>> = (0..nf).map(|f| self.loop_order(f)).collect();
        let mut edge_new = vec![u32::MAX; self.edges.len()];
        let mut edge_order = Vec::with_capacity(self.edges.len());
        let mut uses: Vec<Vec<EdgeUseRef>> = vec![Vec::new(); self.edges.len()];
        for (fnew, &f) in face_order.iter().enumerate() {
            let face = self.faces.get(f as usize).ok_or("face reference")?;
            let forward = direction.get(f as usize).copied().unwrap_or(true);
            for (lnew, &l) in outer_first.get(f as usize).into_iter().flatten().enumerate() {
                let Some(LoopB::Edges(es)) = face.loops.get(l) else { continue };
                let n = es.len();
                for k in 0..n {
                    let p = if forward { k } else { n - 1 - k };
                    let &(e, fwd) = es.get(p).ok_or("edge reference")?;
                    let slot = edge_new.get_mut(e as usize).ok_or("edge reference")?;
                    if *slot == u32::MAX {
                        *slot = index(edge_order.len())?;
                        edge_order.push(e);
                    }
                    let at = EdgeUseRef { face: index(fnew)?, r#loop: index(lnew)?, position: index(p)?, upper: !fwd };
                    uses.get_mut(e as usize).ok_or("edge reference")?.push(at);
                }
            }
        }
        // Vertices walking edges (start, end), then vertex loops.
        let mut vertex_new = vec![u32::MAX; self.vertices.len()];
        let mut vertex_order = Vec::with_capacity(self.vertices.len());
        let mut visit = |v: u32, vertex_new: &mut Vec<u32>| -> R<u32> {
            let slot = vertex_new.get_mut(v as usize).ok_or("vertex reference")?;
            if *slot == u32::MAX {
                *slot = index(vertex_order.len())?;
                vertex_order.push(v);
            }
            Ok(*slot)
        };
        for &e in &edge_order {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            for v in edge.vertices {
                visit(v, &mut vertex_new)?;
            }
        }
        for &f in &face_order {
            for &l in outer_first.get(f as usize).into_iter().flatten() {
                if let Some(LoopB::Vertex(v)) = self.faces.get(f as usize).and_then(|x| x.loops.get(l)) {
                    visit(*v, &mut vertex_new)?;
                }
            }
        }
        let vmap = |v: u32| vertex_new.get(v as usize).copied().filter(|x| *x != u32::MAX).ok_or("vertex reference".to_string());
        let emap = |e: u32| edge_new.get(e as usize).copied().filter(|x| *x != u32::MAX).ok_or("edge reference".to_string());
        let fmap = |f: u32| face_new.get(f as usize).copied().filter(|x| *x != u32::MAX).ok_or("face reference".to_string());
        let mut turned_faces = Vec::with_capacity(flipped);
        for (f, t) in turned.iter().enumerate() {
            if *t {
                turned_faces.push(fmap(index(f)?)?);
            }
        }
        turned_faces.sort_unstable();

        // Vertex loops: their vertex's distance from the surface is its tolerance.
        for &f in &face_order {
            let Some(face) = self.faces.get(f as usize) else { continue };
            let surface = SurfaceEval::new(&face.surface).map_err(|m| m.to_string())?;
            for l in &face.loops {
                if let LoopB::Vertex(v) = l
                    && let Some(x) = self.vertices.get_mut(*v as usize)
                {
                    let p = v3(x.point);
                    let gap = surface.project(p, None, f64::INFINITY).and_then(|q| surface.point(q)).map_or(f64::INFINITY, |s| s.dist(p));
                    if !(gap <= MAX_VERTEX_GAP) {
                        return Err(format!("face #{}: a vertex loop's vertex lies {gap} mm off its surface", face.step));
                    }
                    x.tolerance = x.tolerance.max(1.5 * gap);
                }
            }
        }

        let mut vertices = Vec::with_capacity(vertex_order.len());
        for &v in &vertex_order {
            vertices.push(self.vertices.get(v as usize).cloned().ok_or("vertex reference")?);
        }
        let mut edges = Vec::with_capacity(edge_order.len());
        for &e in &edge_order {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let (nurbs, interval) = match &edge.form {
                Some((c, i)) => (Some(c.clone()), *i),
                None => (None, edge.interval),
            };
            edges.push(Edge {
                nurbs,
                curve: edge.curve.clone(),
                interval,
                vertices: [vmap(edge.vertices[0])?, vmap(edge.vertices[1])?],
                tolerance: edge.tolerance,
                uses: uses.get(e as usize).cloned().unwrap_or_default(),
            });
        }
        let mut faces = Vec::with_capacity(face_order.len());
        let mut face_origin = Vec::with_capacity(face_order.len());
        for &f in &face_order {
            let face = self.faces.get(f as usize).ok_or("face reference")?;
            face_origin.push(face.step);
            let pc = pcs.get(f as usize).ok_or("face reference")?;
            let mut loops = Vec::with_capacity(face.loops.len());
            let mut edge_loop = 0;
            // pcurves come in the face's edge-loop order; loops go out outer first.
            let mut fitted: Vec<Option<&Vec<Curve2>>> = Vec::with_capacity(face.loops.len());
            for l in &face.loops {
                fitted.push(match l {
                    LoopB::Edges(_) => {
                        edge_loop += 1;
                        pc.get(edge_loop - 1)
                    }
                    LoopB::Vertex(_) => None,
                });
            }
            for &l in outer_first.get(f as usize).into_iter().flatten() {
                match face.loops.get(l) {
                    Some(LoopB::Edges(es)) => {
                        let curves = fitted.get(l).copied().flatten().ok_or("pcurves")?;
                        let mut out = Vec::with_capacity(es.len());
                        for (&(e, forward), c) in es.iter().zip(curves) {
                            out.push(OrientedEdge { edge: emap(e)?, forward, pcurve: c.clone() });
                        }
                        loops.push(Loop::Edges { edges: out });
                    }
                    Some(LoopB::Vertex(v)) => loops.push(Loop::Vertex { vertex: vmap(*v)? }),
                    None => return Err("loop reference".into()),
                }
            }
            let domain = *domains.get(f as usize).ok_or("face reference")?;
            let nurbs = face_forms.get(f as usize).cloned().flatten();
            faces.push(Face { nurbs, surface: face.surface.clone(), domain, loops });
        }
        let mut out_shells = Vec::with_capacity(shells.len());
        for s in &shells {
            let mut faces = Vec::with_capacity(s.len());
            for &(f, forward) in s {
                faces.push(OrientedFace { face: fmap(f)?, forward });
            }
            out_shells.push(Shell::Faces { faces, wires: Vec::new() });
        }
        let brep = Brep { version: brep::VERSION, tolerance: self.tol, regions, shells: out_shells, faces, edges, vertices };
        brep.validate().map_err(|e| format!("the record is invalid: {e}"))?;
        brep.check_geometry().map_err(|e| format!("the record's geometry disagrees: {e}"))?;
        Ok(Solid { item, name, product: None, brep, face_origin, sheet, notes, turned: turned_faces })
    }

    /// Every edge of the shell's faces used at least twice among them.
    fn closed(&self, shell: &[(u32, bool)]) -> bool {
        let mut count: HashMap<u32, u32> = HashMap::new();
        for &(f, _) in shell {
            for l in self.faces.get(f as usize).map(|f| f.loops.as_slice()).unwrap_or(&[]) {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        *count.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        count.values().all(|n| *n >= 2)
    }

    /// The face's loops, outer first (the rest in the file's order).
    fn loop_order(&self, f: usize) -> Vec<usize> {
        let Some(face) = self.faces.get(f) else { return Vec::new() };
        let outer = face.outer.unwrap_or(0);
        std::iter::once(outer).chain((0..face.loops.len()).filter(|l| *l != outer)).collect()
    }

    /// The pcurves of a face's edge loops (in its edge-loop order), its domain, whether it was
    /// turned over, and its NURBS form. Picks the outer loop when the file
    /// marks none (or marks one that another loop encloses), and turns the face over when its
    /// outer loop runs clockwise about the natural normal on a surface where that cannot be a
    /// face's outer boundary.
    fn face_pcurves(&mut self, fi: usize) -> R<(Vec<Vec<Curve2>>, [[f64; 2]; 2], bool, Option<BSurface>)> {
        let face = self.faces.get(fi).ok_or("face reference")?;
        let label = face.surface.clone();
        let edge_loops: Vec<Vec<(u32, bool)>> = face
            .loops
            .iter()
            .filter_map(|l| match l {
                LoopB::Edges(es) => Some(es.clone()),
                LoopB::Vertex(_) => None,
            })
            .collect();
        // Which loop index in `face.loops` each edge loop is.
        let positions: Vec<usize> = face.loops.iter().enumerate().filter(|(_, l)| matches!(l, LoopB::Edges(_))).map(|(i, _)| i).collect();
        let tol = self.tol;
        // A cylinder, cone, sphere, torus or revolution on its NURBS form, over the extent of its
        // boundary on its label; then every pcurve on the forms.
        let form = match brep::form::frame_of(&label) {
            Some(_) => {
                let mut boundary = Vec::with_capacity(edge_loops.len());
                for l in &edge_loops {
                    let mut b = Vec::with_capacity(l.len());
                    for &(e, forward) in l {
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        b.push((&edge.curve, edge.interval, forward));
                    }
                    boundary.push(b);
                }
                let extent = brep::form::extent(&label, &boundary).ok_or_else(|| unsupported("a face whose boundary does not lie on its surface"))?;
                Some(brep::form::surface(&label, extent).ok_or_else(|| unsupported("a face of more than one turn"))?)
            }
            None => None,
        };
        let (form, box_on_form) = match form {
            Some((f, d)) => (Some(f), Some(d)),
            None => (None, None),
        };
        let surface = form.clone().unwrap_or_else(|| label.clone());
        // Closed forms where the geometry has them; the kernel makes the rest (`trims`).
        let exact = pcurve::exact_pcurves(&surface, &uses_of(&self.edges, &edge_loops, None)?, tol, box_on_form).map_err(|e| e.to_string())?;
        // Without a kernel, a use with neither is refused below with why.
        let made = match self.trims {
            Some(trims) if exact.iter().flatten().any(Option::is_none) => {
                let domain = match box_on_form {
                    Some(d) => d,
                    None => natural_box(&surface).ok_or_else(|| unsupported("a face on an unbounded surface without closed-form pcurves"))?,
                };
                Some(trims.trims(&self.trim_face(&label, form.as_ref(), domain, &edge_loops, exact)?)?)
            }
            _ => None,
        };
        let pcs = pcurve::pcurves_within(&surface, &uses_of(&self.edges, &edge_loops, made.as_ref())?, tol, box_on_form).map_err(|e| e.to_string())?;
        // Signed areas of the loops that close in (u, v).
        let periods = natural_periods(&surface);
        // Each use starts near where the one before it ends, along a bounded direction that
        // does not wrap: a loop around a closed spline surface without a seam edge jumps across
        // its domain.
        let natural = surface.natural();
        // At a pole (where the surface collapses to a point across a direction) a loop may jump
        // in that direction: the seam's two uses meet there a period apart.
        let poles = SurfaceEval::new(&surface).ok();
        let at_pole = |uv: [f64; 2]| poles.as_ref().is_some_and(|e| e.pole(uv).iter().any(|p| *p));
        for (l, pc) in edge_loops.iter().zip(&pcs) {
            let mut ends = Vec::with_capacity(l.len());
            for (&(_, forward), c) in l.iter().zip(pc) {
                let [a, b] = c.interval().ok_or("pcurve interval")?;
                let (s, e) = (c.eval(a).ok_or("pcurve start")?, c.eval(b).ok_or("pcurve end")?);
                ends.push(if forward { (s, e) } else { (e, s) });
            }
            for k in 0..ends.len() {
                let (Some((start, _)), Some((_, end))) = (ends.get(k), ends.get(if k == 0 { ends.len() - 1 } else { k - 1 })) else { continue };
                if at_pole(*start) || at_pole(*end) {
                    continue;
                }
                for d in 0..2 {
                    // Only a bounded direction has a seam to jump; half its width is no gap.
                    let Some(Natural::Range(a, b)) = natural.get(d) else { continue };
                    if !(b - a).is_finite() {
                        continue;
                    }
                    if !((start[d] - end[d]).abs() <= 0.5 * (b - a)) {
                        return Err(unsupported("a loop across a closed spline surface's seam without a seam edge"));
                    }
                }
            }
        }
        let shapes: Vec<Option<f64>> = edge_loops.iter().zip(&pcs).map(|(l, pc)| loop_area(l, pc, periods)).collect();
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        let marked = face.outer.and_then(|o| positions.iter().position(|p| *p == o));
        let largest = shapes.iter().enumerate().filter_map(|(i, a)| a.map(|a| (i, a.abs()))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
        let outer = match (marked, largest) {
            // A marked outer loop that closes in (u, v) but another encloses more: that one.
            (Some(m), Some(l)) if shapes.get(m).copied().flatten().is_some() => l,
            (Some(m), _) => m,
            (None, Some(l)) if shapes.iter().all(Option::is_some) => l,
            (None, _) => 0,
        };
        face.outer = positions.get(outer).copied();
        let can_turn = matches!(
            surface,
            BSurface::Plane { .. } | BSurface::Cylinder { .. } | BSurface::Cone { .. } | BSurface::Spline { .. } | BSurface::RationalSpline { .. }
        );
        let flip = can_turn && shapes.get(outer).copied().flatten().is_some_and(|a| a < 0.0);
        if flip {
            for l in &mut face.loops {
                if let LoopB::Edges(es) = l {
                    reverse(es);
                }
            }
            face.forward = !face.forward;
        }
        let pcs: Vec<Vec<Curve2>> = if flip { pcs.into_iter().map(|l| l.into_iter().rev().collect()).collect() } else { pcs };
        let domain = face_domain(&surface, &pcs)?;
        Ok((pcs, domain, flip, form))
    }
}

impl Builder<'_, '_> {
    /// The face as a kernel takes it to make its missing pcurves: its edges and vertices
    /// numbered within it, each use with its closed-form pcurve where `exact` has one.
    fn trim_face<'s>(
        &'s self,
        surface: &'s BSurface,
        nurbs: Option<&'s BSurface>,
        domain: [[f64; 2]; 2],
        edge_loops: &[Vec<(u32, bool)>],
        exact: Vec<Vec<Option<Curve2>>>,
    ) -> R<TrimFace<'s>> {
        let (mut edge_at, mut vertex_at) = (HashMap::new(), HashMap::new());
        let (mut edges, mut vertices) = (Vec::new(), Vec::new());
        let mut loops = Vec::with_capacity(edge_loops.len());
        for (l, pcs) in edge_loops.iter().zip(exact) {
            let mut uses = Vec::with_capacity(l.len());
            for (&(e, forward), pcurve) in l.iter().zip(pcs) {
                let local = match edge_at.get(&e) {
                    Some(i) => *i,
                    None => {
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        let mut ends = [0u32; 2];
                        for (end, v) in ends.iter_mut().zip(edge.vertices) {
                            *end = match vertex_at.get(&v) {
                                Some(i) => *i,
                                None => {
                                    let i = index(vertices.len())?;
                                    vertices.push(self.point(v)?);
                                    vertex_at.insert(v, i);
                                    i
                                }
                            };
                        }
                        let (curve, nurbs, interval) = match &edge.form {
                            Some((n, i)) => (&edge.curve, Some(n), *i),
                            None => (&edge.curve, None, edge.interval),
                        };
                        let i = index(edges.len())?;
                        edges.push(TrimEdge { curve, nurbs, interval, vertices: ends, tolerance: edge.tolerance });
                        edge_at.insert(e, i);
                        i
                    }
                };
                uses.push(TrimUse { edge: local, forward, pcurve });
            }
            loops.push(uses);
        }
        Ok(TrimFace { surface, nurbs, domain, edges, vertices, loops, tolerance: self.tol })
    }
}

/// A face's uses on their edges' NURBS forms, with `given` pcurves where a kernel made them.
fn uses_of<'e>(edges: &'e [EdgeB], edge_loops: &[Vec<(u32, bool)>], given: Option<&'e Vec<Vec<Curve2>>>) -> R<Vec<Vec<EdgeUse<'e>>>> {
    let mut uses = Vec::with_capacity(edge_loops.len());
    for (li, l) in edge_loops.iter().enumerate() {
        let mut u = Vec::with_capacity(l.len());
        for (p, &(e, forward)) in l.iter().enumerate() {
            let edge = edges.get(e as usize).ok_or("edge reference")?;
            let (curve, interval) = match &edge.form {
                Some((c, i)) => (c, *i),
                _ => (&edge.curve, edge.interval),
            };
            let given = match given {
                Some(g) => Some(g.get(li).and_then(|l| l.get(p)).ok_or("a pcurve the kernel did not make")?),
                None => None,
            };
            u.push(EdgeUse { edge: e, curve, interval, forward, tolerance: 0.0, given });
        }
        uses.push(u);
    }
    Ok(uses)
}

/// A spline surface's parameter box: its knot ranges.
fn natural_box(s: &BSurface) -> Option<[[f64; 2]; 2]> {
    match s.natural() {
        [Natural::Range(a, b), Natural::Range(c, d)] if [a, b, c, d].iter().all(|x| x.is_finite()) => Some([[a, b], [c, d]]),
        _ => None,
    }
}

/// The loop run the other way.
fn reverse(uses: &mut [(u32, bool)]) {
    uses.reverse();
    for u in uses {
        u.1 = !u.1;
    }
}

fn natural_periods(s: &BSurface) -> [bool; 2] {
    s.natural().map(|n| matches!(n, Natural::Periodic))
}

/// The signed area a loop encloses in (u, v) (positive counter-clockwise), or `None` when it
/// does not close there (it winds around a periodic direction) or is degenerate.
fn loop_area(l: &[(u32, bool)], pcs: &[Curve2], periodic: [bool; 2]) -> Option<f64> {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for (&(_, forward), c) in l.iter().zip(pcs) {
        let [a, b] = c.interval()?;
        for k in 0..=16 {
            let s = f64::from(k) / 16.0;
            let t = if forward { a + (b - a) * s } else { b - (b - a) * s };
            pts.push(c.eval(t)?);
        }
    }
    let (first, last) = (*pts.first()?, *pts.last()?);
    for d in 0..2 {
        if periodic.get(d).copied().unwrap_or(false) && (first.get(d)? - last.get(d)?).abs() > 1.0 {
            return None;
        }
    }
    let mut area = 0.0;
    let mut magnitude = 0.0;
    for w in pts.windows(2) {
        let (p, q) = (w.first()?, w.get(1)?);
        let c = (p[0] - first[0]) * (q[1] - first[1]) - (q[0] - first[0]) * (p[1] - first[1]);
        area += c;
        magnitude += c.abs();
    }
    (area.abs() > 1e-9 * magnitude && magnitude > 0.0).then_some(0.5 * area)
}

/// The parameter box around every pcurve's control points, padded a little, within the
/// surface's natural domain (at most one turn where periodic).
fn face_domain(surface: &BSurface, pcs: &[Vec<Curve2>]) -> R<[[f64; 2]; 2]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for c in pcs.iter().flatten() {
        let pts: Vec<[f64; 2]> = match c {
            Curve2::Spline { points, .. } => points.clone(),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().filter(|h| h[2] > 0.0).map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
        };
        for p in pts {
            for d in 0..2 {
                lo[d] = lo[d].min(p[d]);
                hi[d] = hi[d].max(p[d]);
            }
        }
    }
    let natural = surface.natural();
    let mut out = [[0.0; 2]; 2];
    for d in 0..2 {
        let (a, b) = (lo[d], hi[d]);
        if !(a.is_finite() && b.is_finite() && a <= b) {
            return Err("a face without pcurves".into());
        }
        let pad = 1e-6 * (1.0 + (b - a)) + 1e-9 * a.abs().max(b.abs());
        let (mut x, mut y) = (a - pad, b + pad);
        match natural.get(d) {
            Some(Natural::Periodic) => {
                if y - x > TAU {
                    x = a;
                    y = a + TAU;
                }
            }
            Some(Natural::Range(n0, n1)) => {
                x = x.max(*n0);
                y = y.min(*n1);
            }
            None => {}
        }
        if !(x < y) {
            return Err("an empty face domain".into());
        }
        if let Some(slot) = out.get_mut(d) {
            *slot = [x, y];
        }
    }
    Ok(out)
}

fn frame(p: &Placement) -> Frame {
    Frame { origin: p.origin, x_axis: p.ref_direction, y_axis: p.y() }
}

/// Knots repeated by their multiplicities, for a spline of `count` points and `degree`.
fn full_knots(knots: &[f64], mults: &[usize], count: usize, degree: usize) -> R<Vec<f64>> {
    if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
        return Err(unsupported(format!("a B-spline of degree {degree}")));
    }
    if !(2..=MAX_SPLINE_POINTS).contains(&count) {
        return Err(unsupported(format!("a B-spline with {count} control points")));
    }
    if knots.len() != mults.len() {
        return Err("B-spline knots and multiplicities differ in number".into());
    }
    let total = mults.iter().try_fold(0usize, |a, m| a.checked_add(*m)).ok_or("B-spline knot count")?;
    if total != count + degree + 1 {
        return Err(format!("B-spline with {total} knots for {count} points of degree {degree}"));
    }
    let mut v = Vec::with_capacity(total);
    for (k, m) in knots.iter().zip(mults) {
        v.extend(std::iter::repeat_n(*k, *m));
    }
    Ok(v)
}

fn map_bspline_curve(b: &BSplineCurve) -> R<BCurve> {
    let knots = full_knots(&b.knots, &b.multiplicities, b.points.len(), b.degree)?;
    Ok(match &b.weights {
        None => BCurve::Spline { knots, points: b.points.clone() },
        Some(w) => {
            if w.len() != b.points.len() {
                return Err("B-spline weights and points differ in number".into());
            }
            BCurve::RationalSpline { knots, homogeneous: b.points.iter().zip(w).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect() }
        }
    })
}

fn map_bspline_surface(b: &BSplineSurface) -> R<BSurface> {
    let nu = b.points.len();
    let nv = b.points.first().map_or(0, Vec::len);
    let u_knots = full_knots(&b.u_knots, &b.u_multiplicities, nu, b.u_degree)?;
    let v_knots = full_knots(&b.v_knots, &b.v_multiplicities, nv, b.v_degree)?;
    if b.points.iter().any(|r| r.len() != nv) {
        return Err("ragged B-spline surface".into());
    }
    Ok(match &b.weights {
        None => BSurface::Spline { u_knots, v_knots, points: b.points.clone() },
        Some(w) => {
            if w.len() != nu || w.iter().any(|r| r.len() != nv) {
                return Err("B-spline surface weights and points differ in number".into());
            }
            let homogeneous =
                b.points.iter().zip(w).map(|(row, wr)| row.iter().zip(wr).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect()).collect();
            BSurface::RationalSpline { u_knots, v_knots, homogeneous }
        }
    })
}

/// An edge on its schema curve.
#[derive(Debug)]
struct Mapped {
    curve: BCurve,
    interval: [f64; 2],
    /// The schema edge (interval min → max) runs from the STEP edge's start to its end.
    with_step: bool,
    /// The file's same_sense disagrees with the vertices' order on an open curve.
    sense_mismatch: bool,
}

fn angle_of(a: f64, b: f64) -> f64 {
    let t = b.atan2(a);
    if t < 0.0 { t + TAU } else { t }
}

/// `x` reduced to [0, 2π).
fn turn(x: f64) -> f64 {
    let r = x.rem_euclid(TAU);
    if r >= TAU { 0.0 } else { r }
}

/// The edge from `ps` to `pe` on `curve`: its schema curve and interval (min → max) and which
/// way it runs. `closed`: the same vertex at both ends; `near`: the ends coincide within the
/// tolerance (a full turn on a periodic curve).
fn map_edge(curve: &Curve, ps: P3, pe: P3, same_sense: bool, closed: bool, near: bool, tol: f64) -> R<Mapped> {
    match curve {
        Curve::Line { origin, direction, magnitude } => {
            if closed {
                return Err("a closed edge on a line".into());
            }
            let m = *magnitude;
            let param = |p: P3| dot(sub(p, *origin), *direction) / m;
            let (ts, te) = (param(ps), param(pe));
            if !((te - ts).abs() * m > 1e-12 * (1.0 + ts.abs().max(te.abs()) * m)) {
                return Err("a degenerate edge on a line".into());
            }
            let with_step = ts < te;
            let (lo, hi) = if with_step { (ts, te) } else { (te, ts) };
            let start = crate::geom::add(*origin, scale(*direction, lo * m));
            let end = crate::geom::add(start, scale(*direction, m));
            Ok(Mapped { curve: BCurve::Line { start, end }, interval: [0.0, hi - lo], with_step, sense_mismatch: with_step != same_sense })
        }
        Curve::Circle { position, radius } => {
            conic(BCurve::Circle { frame: frame(position), radius: *radius }, position, [*radius; 2], ps, pe, same_sense, near)
        }
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => {
            let radii = [*semi_axis_1, *semi_axis_2];
            conic(BCurve::Ellipse { frame: frame(position), radii }, position, radii, ps, pe, same_sense, near)
        }
        Curve::BSpline(b) => open_or_closed(map_bspline_curve(b)?, ps, pe, same_sense, closed, tol),
        Curve::Polyline(points) => {
            if points.windows(2).any(|w| w.first() == w.get(1)) {
                return Err("a polyline with a repeated point".into());
            }
            if !(2..=MAX_SPLINE_POINTS * 16).contains(&points.len()) {
                return Err(unsupported(format!("a polyline of {} points", points.len())));
            }
            open_or_closed(BCurve::Polyline { points: points.clone() }, ps, pe, same_sense, closed, tol)
        }
        Curve::Hyperbola { position, semi_axis, semi_imag_axis } => {
            open_conic(curve, ps, pe, same_sense, closed, |lo, hi| swept::hyperbola_spline(position, *semi_axis, *semi_imag_axis, lo, hi))
        }
        Curve::Parabola { position, focal_dist } => {
            open_conic(curve, ps, pe, same_sense, closed, |lo, hi| swept::parabola_spline(position, *focal_dist, lo, hi))
        }
        Curve::OffsetCurve3d { .. } => Err(unsupported("OFFSET_CURVE_3D")),
        Curve::Trimmed { .. } | Curve::SurfaceCurve { .. } => Err(unsupported("nested trimmed or surface curves")),
    }
}

/// A hyperbola or parabola: the vertices' parameters in closed form, the edge running the way
/// they go, as an exact spline over just that interval.
fn open_conic(curve: &Curve, ps: P3, pe: P3, same_sense: bool, closed: bool, spline: impl Fn(f64, f64) -> BCurve) -> R<Mapped> {
    if closed {
        return Err("a closed edge on an open conic".into());
    }
    let (ts, te) = swept::open_conic_params(curve, ps, pe)?;
    if !((te - ts).abs() > 1e-12 * (1.0 + ts.abs().max(te.abs()))) {
        return Err("a degenerate edge on an open conic".into());
    }
    if (ts - te).abs() > 64.0 {
        return Err(unsupported("a hyperbola or parabola edge that long"));
    }
    let with_step = ts < te;
    let (lo, hi) = if with_step { (ts, te) } else { (te, ts) };
    Ok(Mapped { curve: spline(lo, hi), interval: [lo, hi], with_step, sense_mismatch: with_step != same_sense })
}

/// A circle or ellipse: the vertices' angles, the arc between them in the edge's sense (a full
/// turn when they coincide).
fn conic(curve: BCurve, position: &Placement, radii: [f64; 2], ps: P3, pe: P3, same_sense: bool, near: bool) -> R<Mapped> {
    let y = position.y();
    let param = |p: P3| {
        let q = sub(p, position.origin);
        angle_of(dot(q, position.ref_direction) / radii[0], dot(q, y) / radii[1])
    };
    let (ts, te) = (param(ps), param(pe));
    // Along the curve from `from` to `to`, increasing.
    let span = |from: f64, to: f64| if near { TAU } else { turn(to - from) };
    let (lo, d) = if same_sense { (ts, span(ts, te)) } else { (te, span(te, ts)) };
    if !(d > 0.0) {
        return Err("a degenerate edge on a conic".into());
    }
    Ok(Mapped { curve, interval: [lo, lo + d], with_step: same_sense, sense_mismatch: false })
}

/// A spline or polyline: the vertices' parameters by projection. On an open curve the edge runs
/// the way its vertices go; on a closed one (its ends coincide) the edge's sense decides, and a
/// vertex at the seam takes whichever end the sense needs.
fn open_or_closed(curve: BCurve, ps: P3, pe: P3, same_sense: bool, closed: bool, tol: f64) -> R<Mapped> {
    let eval = CurveEval::new(&curve).map_err(|e| e.to_string())?;
    let Natural::Range(a, b) = curve.natural() else { return Err("a periodic spline".into()) };
    if !(a < b) {
        return Err("an empty spline domain".into());
    }
    let (pa, pb) = (eval.point(a).ok_or("spline start")?, eval.point(b).ok_or("spline end")?);
    let curve_closed = pa.dist(pb) <= tol.max(1e-9 * (1.0 + pa.len()));
    if closed {
        if !curve_closed {
            return Err("a closed edge on an open curve".into());
        }
        return Ok(Mapped { curve, interval: [a, b], with_step: true, sense_mismatch: false });
    }
    let knots = match &curve {
        BCurve::Spline { knots, .. } | BCurve::RationalSpline { knots, .. } => knots.clone(),
        BCurve::Polyline { points } => (0..points.len()).map(|i| i as f64).collect(),
        _ => Vec::new(),
    };
    let ts = project(&eval, a, b, &knots, v3(ps)).ok_or("no parameter for the edge's start")?;
    let te = project(&eval, a, b, &knots, v3(pe)).ok_or("no parameter for the edge's end")?;
    let eps = 1e-9 * (b - a);
    if !curve_closed {
        if !((te - ts).abs() > eps) {
            return Err("a degenerate edge on a spline".into());
        }
        let with_step = ts < te;
        let interval = if with_step { [ts, te] } else { [te, ts] };
        return Ok(Mapped { curve, interval, with_step, sense_mismatch: with_step != same_sense });
    }
    let (mut from, mut to) = if same_sense { (ts, te) } else { (te, ts) };
    let at_seam = |t: f64| t - a <= eps || b - t <= eps;
    if !(from + eps < to) {
        if at_seam(from) && b - from <= eps {
            from = a;
        }
        if at_seam(to) && to - a <= eps {
            to = b;
        }
    }
    if !(from + eps < to) {
        return Err(unsupported("an edge across a closed spline's seam"));
    }
    Ok(Mapped { curve, interval: [from, to], with_step: same_sense, sense_mismatch: false })
}

/// The parameter in [a, b] of the curve's point nearest `p`: the nearest of samples along each
/// knot span, refined by golden-section search between its neighbours.
fn project(c: &CurveEval, a: f64, b: f64, knots: &[f64], p: Vec3) -> Option<f64> {
    let mut breaks: Vec<f64> = knots.iter().copied().filter(|k| *k > a && *k < b).collect();
    breaks.dedup();
    let spans = breaks.len() + 1;
    let per = (MAX_SAMPLES / spans).clamp(1, 8);
    let mut ts = Vec::with_capacity(spans * per + 1);
    let mut lo = a;
    for hi in breaks.into_iter().chain(std::iter::once(b)) {
        for k in 0..per {
            ts.push(lo + (hi - lo) * k as f64 / per as f64);
        }
        lo = hi;
    }
    ts.push(b);
    if ts.len() > MAX_SAMPLES {
        let stride = ts.len().div_ceil(MAX_SAMPLES);
        let last = *ts.last()?;
        ts = ts.into_iter().step_by(stride).collect();
        ts.push(last);
    }
    let dist = |t: f64| c.point(t).map_or(f64::INFINITY, |q| q.dist(p));
    let (best, _) = ts.iter().enumerate().map(|(i, t)| (i, dist(*t))).min_by(|x, y| x.1.total_cmp(&y.1))?;
    let mut x0 = *ts.get(best.saturating_sub(1))?;
    let mut x1 = *ts.get((best + 1).min(ts.len() - 1))?;
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let (mut c1, mut c2) = (x1 - g * (x1 - x0), x0 + g * (x1 - x0));
    let (mut f1, mut f2) = (dist(c1), dist(c2));
    for _ in 0..200 {
        if x1 - x0 <= 1e-15 * (1.0 + x0.abs().max(x1.abs())) {
            break;
        }
        if f1 < f2 {
            x1 = c2;
            c2 = c1;
            f2 = f1;
            c1 = x1 - g * (x1 - x0);
            f1 = dist(c1);
        } else {
            x0 = c1;
            c1 = c2;
            f1 = f2;
            c2 = x0 + g * (x1 - x0);
            f2 = dist(c2);
        }
    }
    let t = 0.5 * (x0 + x1);
    // The ends themselves when they are as near (a vertex at the end of the curve).
    let t = [a, b, t].into_iter().min_by(|x, y| dist(*x).total_cmp(&dist(*y)))?;
    let near_end = |e: f64| (dist(e) - dist(t)).abs() <= 1e-12 * (1.0 + p.len());
    Some(
        if near_end(a) {
            a
        } else if near_end(b) {
            b
        } else {
            t
        }
        .clamp(a, b),
    )
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod tests;
