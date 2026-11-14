//! STEP topology (ISO 10303-42 topology and geometric model entities) into the kernel-neutral
//! B-rep, schema 4 ([`solidcore_record::brep`]). Written from the public
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
//! vertex's and edge's to start with. A vertex measurably off its curve or surface, or an edge
//! whose curve lies measurably off a face's surface, gets that distance (with margin) as its
//! tolerance, and the solid's notes say how many and how far.
//!
//! Pcurves are the closed forms the 3D edges have on our surfaces ([`pcurve::exact_pcurves`]); a
//! use with none gets its pcurve from the kernel ([`read_solids_with`], SMLib's own trims). The
//! file's own PCURVE entities are not used.
//!
//! Input is hostile: every reference is checked, nesting and counts are capped, and the work of
//! a whole file is bounded (shared shells count each time a solid uses them).

use crate::vec::Vec3Geom as _;
use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use crate::kernel::{CurveEval, SurfaceEval, face_flux};
use solidcore_ops::pcurve::{self, EdgeUse, TrimEdge, TrimFace, TrimUse, Trims};
use solidcore_record::Vec3;
use solidcore_record::brep::{
    self, Brep, Curve as BCurve, Curve2, Edge, EdgeUseRef, Face, Frame, Loop, Natural, OrientedEdge, OrientedFace, Region, Shell,
    Surface as BSurface, Vertex,
};

use crate::geom::{BSplineCurve, BSplineSurface, Curve, P3, Placement, Transform, dot, scale, sub};
use crate::p21::{Entity, Exchange, Param};
use crate::read::{R, Reader};
use crate::units;

mod frames;
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
/// The largest tolerance (mm) an edge or vertex is given: SMLib takes none larger (smlib.h's
/// EDGE_TOLERANCE and VERTEX_TOLERANCE). Geometry that
/// would need more is refused, with how far off it lies.
const MAX_TOLERANCE: f64 = 1.0;
/// A vertex's tolerance over its largest gap from its curves and surfaces.
const VERTEX_GAP_MARGIN: f64 = 1.5;
/// Intervals an edge's curve is measured over against each face's surface it bounds.
const EDGE_GAP_SAMPLES: usize = 16;
/// The least span of a face's parameter box along a direction of its surface that is not
/// closed on itself, where its natural range has room ([`face_domain`]): 50 times the 2e-5 mm
/// within which SMLib takes two of a surface's sides for one (#12757: a planar sliver face
/// 1e-5 mm wide).
const OPEN_DOMAIN_SPAN: f64 = 1e-3;
/// An edge's tolerance over the largest gap measured between its pcurves and its curve, as the
/// record check measures it: with margin, as a vertex's.
const EDGE_GAP_MARGIN: f64 = 1.5;
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
/// The shells a file may hold outside any solid or surface model.
const SHELL_ITEMS: [&str; 4] = ["CLOSED_SHELL", "OPEN_SHELL", "ORIENTED_CLOSED_SHELL", "ORIENTED_OPEN_SHELL"];
/// The faces a file may hold outside any shell.
const FACE_ITEMS: [&str; 4] = ["ADVANCED_FACE", "FACE_SURFACE", "FACE", "ORIENTED_FACE"];

/// Whether an item is a solid or a surface model, read as a body wherever a representation lists
/// it.
pub fn is_solid_item(name: &str) -> bool {
    SOLID_ITEMS.contains(&name)
}

/// Whether an item of the file's unlisted topology ([`roots`]) is read as a body: a solid or
/// surface model, or a shell or face on its own. A representation's own shell or face items (a
/// presentation's, a tolerance's) are not bodies.
pub fn is_body_item(name: &str) -> bool {
    SOLID_ITEMS.contains(&name) || SHELL_ITEMS.contains(&name) || FACE_ITEMS.contains(&name)
}

/// The topology a file holds outside any representation, as bodies to read (a file of bare
/// geometry, or one whose topology no shape representation lists): every solid or surface model
/// no other entity refers to, then every shell no solid, model or other shell lists, then every
/// face no shell lists (each a one-face sheet), in id order within each. A presentation's reference
/// (a style, a layer, an invisibility) does not count: a styled loose shell is still unlisted. As
/// solidcore's
/// step-import read such files (#2583, topology first): what the file's topology holds is read
/// whether or not a product structure names it.
pub fn roots(ex: &Exchange) -> Vec<u64> {
    fn refer(p: &Param, out: &mut HashSet<u64>, depth: usize) {
        match p {
            Param::Ref(id) => {
                out.insert(*id);
            }
            Param::List(items) | Param::Typed(_, items) if depth < 64 => {
                for q in items {
                    refer(q, out, depth + 1);
                }
            }
            _ => {}
        }
    }
    let presentation = |name: &str| name.ends_with("STYLED_ITEM") || matches!(name, "PRESENTATION_LAYER_ASSIGNMENT" | "INVISIBILITY");
    let mut referenced = HashSet::new();
    for e in ex.entities.values() {
        for r in e.records.iter().filter(|r| !presentation(&r.name)) {
            for p in &r.params {
                refer(p, &mut referenced, 0);
            }
        }
    }
    let mut out = Vec::new();
    for kinds in [&SOLID_ITEMS[..], &SHELL_ITEMS[..], &FACE_ITEMS[..]] {
        let mut ids: Vec<u64> =
            ex.entities.iter().filter(|(id, e)| kinds.contains(&e.name()) && !referenced.contains(id)).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        out.extend(ids);
    }
    out
}

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
    let mut solids: Vec<u64> = ex.entities.iter().filter(|(_, e)| SOLID_ITEMS.contains(&e.name())).map(|(id, _)| *id).collect();
    solids.sort_unstable();
    let mut index = Index::new(ex);
    // A surface model's closed shells read on their own sit where the model does.
    let mut items = Vec::with_capacity(solids.len());
    for solid in solids {
        for piece in self::items(ex, solid) {
            if piece != solid {
                if let Some(c) = index.context.get(&solid).copied() {
                    index.context.insert(piece, c);
                }
                if let Some(p) = index.product.get(&solid).cloned() {
                    index.product.insert(piece, p);
                }
            }
            items.push(piece);
        }
    }
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
        match read_with(ex, reader, item, context, &mut budget, trims, &HashSet::new()) {
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
    read_with(ex, &mut reader, item, context, &mut budget, None, &HashSet::new())
}

/// [`read_solid`], with `trims` making the pcurves of uses that have no closed form.
pub fn read_solid_with(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims), &HashSet::new())
}

/// [`read_solid_with`] with the faces `left_out` (file face entities: the ADVANCED_FACE or
/// FACE_SURFACE, or the ORIENTED_FACE a shell lists) left out of their shells: what is left of a
/// solid that cannot be read or made whole, its faces still sharing their edges. With any face
/// left out it is a surface model of open shells (`sheet`), none of them bounding a region.
pub fn read_solid_without(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims, left_out: &HashSet<u64>) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims), left_out)
}

/// The file face a refusal of the reader names first (its "face #id"), if it names one.
pub fn named_face(message: &str) -> Option<u64> {
    message.match_indices("face #").find_map(|(at, m)| {
        let rest = message.get(at + m.len()..)?;
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        rest.get(..digits)?.parse().ok()
    })
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
    left_out: &HashSet<u64>,
) -> R<Solid> {
    let e = entity(ex, item)?;
    let name = e.params().first().and_then(Param::as_str).unwrap_or("").to_string();
    let mut b = Builder::new(ex, reader, tolerance(&context), budget, trims);
    // The file's faces, edges, vertices and their geometry read into the builder.
    let (ids, kind) = item_shells(ex, item)?;
    let mut shells = Vec::with_capacity(ids.len());
    let mut skipped = false;
    for s in ids {
        let (faces, any) = b.shell(s, left_out)?;
        skipped |= any;
        shells.push(faces);
    }
    b.finish(item, name, shells, if skipped { Kind::Open { voids: kind == Kind::Voids } } else { kind })
}

/// A face of a solid read on its own ([`read_faces_with`]).
#[derive(Clone, Debug)]
pub struct FaceRead {
    /// The face's entity in the file (the ADVANCED_FACE or FACE_SURFACE, or the ORIENTED_FACE
    /// the shell lists when it could not be read).
    pub face: u64,
    /// The face as a one-face surface body, facing as its shell uses it; else why it could not
    /// be read (naming the face).
    pub record: R<Brep>,
}

/// Each face of a solid item read on its own, as a one-face surface body: what can still be
/// shown of a solid that cannot be read whole, or whose body the kernel refuses. A face is read
/// as [`read_solid_with`] reads it, with its own edges and vertices (so its tolerances cover its
/// own gaps only); a face that cannot be read on its own says why and the others are still read.
pub fn read_faces_with(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims) -> R<Vec<FaceRead>> {
    let (shells, _) = item_shells(ex, item)?;
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    let mut out = Vec::new();
    for s in shells {
        let (faces, flip) = shell_faces(ex, s)?;
        for f in faces {
            if out.len() >= MAX_FACES {
                return Err(format!("more than {MAX_FACES} faces"));
            }
            let mut b = Builder::new(ex, &mut reader, tolerance(&context), &mut budget, Some(trims));
            let read = b.face(f, 0).and_then(|(face, forward)| b.finish(item, String::new(), vec![vec![(face, forward != flip)]], Kind::Model));
            let named = |m: String| if m.starts_with(&format!("face #{f}")) { m } else { format!("face #{f}: {m}") };
            out.push(match read {
                Ok(s) => FaceRead { face: s.face_origin.first().copied().unwrap_or(f), record: Ok(s.brep) },
                Err(m) => FaceRead { face: f, record: Err(named(m)) },
            });
        }
    }
    Ok(out)
}

/// A surface model's shells the file calls closed and the others, in its order.
fn model_shells(ex: &Exchange, shells: &[u64]) -> (Vec<u64>, Vec<u64>) {
    shells.iter().partition(|s| ex.get(**s).is_some_and(|e| matches!(e.name(), "CLOSED_SHELL" | "ORIENTED_CLOSED_SHELL")))
}

/// The items a solid item is read as, each one body: the item itself, but for a surface model
/// with both closed and open shells (a part with a sheet beside a solid) each closed shell is a
/// solid body of its own and each open shell a surface body of its own, so each sheet keeps its
/// own body count. A model of open shells only keeps the model as its one body, and so does a model of
/// closed shells only.
pub fn items(ex: &Exchange, item: u64) -> Vec<u64> {
    let shells = match ex.get(item) {
        Some(e) if e.name() == "SHELL_BASED_SURFACE_MODEL" => {
            e.params().get(1).and_then(Param::as_list).unwrap_or(&[]).iter().filter_map(Param::as_ref_id).collect()
        }
        _ => Vec::new(),
    };
    let (closed, open) = model_shells(ex, &shells);
    if closed.is_empty() || open.is_empty() {
        // A model of open shells only (even one whose shell is geometrically closed) is the
        // model's one body, as before; so is a model of closed shells only.
        return vec![item];
    }
    // A closed shell beside open ones: each closed shell is a solid body and each open shell a
    // surface body of its own. One open shell stays the model item,
    // which carries the model's name and colour.
    closed.into_iter().chain(if open.len() == 1 { vec![item] } else { open }).collect()
}

/// The shells of a solid item (the outer shell first, then its voids), and what kind it is.
fn item_shells(ex: &Exchange, item: u64) -> R<(Vec<u64>, Kind)> {
    let e = entity(ex, item)?;
    let p = e.params();
    match e.name() {
        // A FACETED_BREP is a manifold solid whose faces are planar polygons (POLY_LOOPs).
        "MANIFOLD_SOLID_BREP" | "FACETED_BREP" => Ok((vec![rid(p.get(1), "outer shell")?], Kind::Solid)),
        "BREP_WITH_VOIDS" => {
            let mut shells = vec![rid(p.get(1), "outer shell")?];
            let voids = refs(p.get(2), "voids")?;
            if voids.len() >= brep::MAX_SHELLS {
                return Err(format!("{} voids", voids.len()));
            }
            shells.extend(voids);
            Ok((shells, Kind::Voids))
        }
        "SHELL_BASED_SURFACE_MODEL" => {
            let ids = refs(p.get(1), "surface model shells")?;
            if ids.is_empty() || ids.len() > brep::MAX_SHELLS {
                return Err(format!("a surface model with {} shells", ids.len()));
            }
            // Its closed shells, where it has open ones too, are items of their own ([`items`]).
            let (closed, open) = model_shells(ex, &ids);
            Ok((if closed.is_empty() || open.is_empty() { ids } else { open }, Kind::Model))
        }
        // A closed shell of a surface model that has open shells too ([`items`]), or a shell or
        // face no solid or model lists ([`roots`]): a model of that one shell, or of the one face.
        "CLOSED_SHELL" | "ORIENTED_CLOSED_SHELL" | "OPEN_SHELL" | "ORIENTED_OPEN_SHELL" => Ok((vec![item], Kind::Model)),
        n if FACE_ITEMS.contains(&n) => Ok((vec![item], Kind::Model)),
        n => Err(unsupported(n)),
    }
}

/// The faces a shell lists, and whether the shell turns them over (an oriented shell used
/// against its faces).
fn shell_faces(ex: &Exchange, id: u64) -> R<(Vec<u64>, bool)> {
    let e = entity(ex, id)?;
    let p = e.params();
    let (faces, flip) = match e.name() {
        "CLOSED_SHELL" | "OPEN_SHELL" => (refs(p.get(1), "shell faces")?, false),
        // A face read on its own ([`roots`]) is a shell of that face.
        n if FACE_ITEMS.contains(&n) => (vec![id], false),
        "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
            let inner = rid(p.get(2), "oriented shell element")?;
            let inner_e = entity(ex, inner)?;
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
    Ok((faces, flip))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Solid,
    Voids,
    Model,
    /// A solid with faces left out ([`read_solid_without`]): its shells are sheets, but for a
    /// shell still closed, which bounds a region of its own (a void's cavity, where `voids`: the
    /// solid's shells after the first are its voids).
    Open {
        voids: bool,
    },
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
    /// A round face's box on its label (`crate::kernel::form::extent`), once measured.
    extent: Option<[[f64; 2]; 2]>,
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
    /// Line edges whose vertices lie off their LINE, read as the segment between the vertices.
    lines_between_vertices: usize,
    /// Inner loops written running the way the outer loop runs, run the other way.
    inner_loops_turned: usize,
    /// Spline surfaces whose first and last control lines met within the tolerance, closed.
    splines_closed: usize,
    /// Edges whose tolerance was raised to the gap their curve has to a face's surface.
    raised_edges: HashSet<u32>,
    /// The line edges of POLY_LOOPs, by their two CARTESIAN_POINTs (lower id first): faces that
    /// share a side share its edge, as a faceted B-rep's do.
    poly_edges: HashMap<(u64, u64), u32>,
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
            lines_between_vertices: 0,
            inner_loops_turned: 0,
            splines_closed: 0,
            raised_edges: HashSet::new(),
            poly_edges: HashMap::new(),
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
        if !(VERTEX_GAP_MARGIN * gap <= MAX_TOLERANCE) {
            return Err(format!("edge #{edge}: a vertex lies {gap} mm off its curve"));
        }
        if let Some(x) = self.vertices.get_mut(v as usize) {
            x.tolerance = x.tolerance.max(VERTEX_GAP_MARGIN * gap);
        }
        Ok(())
    }

    /// An edge's curve as the file has it may lie off a face's surface by more than the file's
    /// uncertainty states (a B-spline edge whose control points stand 9 µm off its plane, in a
    /// file that states 10 µm, gets no closed-form pcurve, which is checked within half that).
    /// On a surface whose nearest point is in closed form (a plane, cylinder, cone, sphere or
    /// torus) the edge's distance from it is measured along the edge before its pcurve is made,
    /// and the edge's tolerance covers it. A spline face's gaps are measured on the pcurves its
    /// kernel makes (`raise_edges`): an iterative nearest point may settle elsewhere on it.
    fn measure_edges(&mut self, surface: &BSurface, edge_loops: &[Vec<(u32, bool)>]) -> R<()> {
        // Only where the nearest point is in closed form: on a spline the search may settle on
        // another part of the surface, or on its boundary for a point a hair past it, and report a
        // gap the edge does not have (2.9 mm on Poppy Humanoid's leg shells, whose edges lie within
        // 0.003 mm), widening tolerances past what SMLib takes. A spline face's gaps are measured
        // on the pcurves its kernel makes (`raise_edges`).
        if !matches!(
            surface,
            BSurface::Plane { .. }
                | BSurface::PlaneThrough { .. }
                | BSurface::Cylinder { .. }
                | BSurface::Cone { .. }
                | BSurface::Sphere { .. }
                | BSurface::Torus { .. }
        ) {
            return Ok(());
        }
        let on = SurfaceEval::new(surface).map_err(|m| m.to_string())?;
        let starts = crate::kernel::Starts::default();
        for l in edge_loops {
            for &(e, _) in l {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let (curve, [t0, t1]) = match &edge.form {
                    Some((c, i)) => (c, *i),
                    None => (&edge.curve, edge.interval),
                };
                let eval = CurveEval::new(curve).map_err(|m| m.to_string())?;
                let mut gap = 0.0f64;
                for k in 0..=EDGE_GAP_SAMPLES {
                    let t = if k == EDGE_GAP_SAMPLES { t1 } else { t0 + (t1 - t0) * (k as f64 / EDGE_GAP_SAMPLES as f64) };
                    let Some(p) = eval.point(t) else { continue };
                    // A point with no nearest point is the pcurves' to refuse, with why.
                    let Some(s) = on.project_with(&starts, p, None, f64::INFINITY).and_then(|q| on.point(q)) else { continue };
                    gap = gap.max(s.dist(p));
                }
                self.cover_edge(e, gap, pcurve::SURFACE_GAP_MARGIN)?;
            }
        }
        Ok(())
    }

    /// Edge `e`'s tolerance covers `gap` with `margin`: refused if far off.
    fn cover_edge(&mut self, e: u32, gap: f64, margin: f64) -> R<()> {
        let Some(x) = self.edges.get_mut(e as usize) else { return Err("edge reference".into()) };
        if margin * gap <= x.tolerance {
            return Ok(());
        }
        if !(margin * gap <= MAX_TOLERANCE) {
            return Err(format!("an edge lies {gap} mm off the face's surface"));
        }
        x.tolerance = margin * gap;
        let (tolerance, ends) = (x.tolerance, x.vertices);
        self.raised_edges.insert(e);
        // A vertex's tolerance covers every edge that meets it, as in any tolerant record.
        for v in ends {
            if let Some(vx) = self.vertices.get_mut(v as usize) {
                vx.tolerance = vx.tolerance.max(tolerance);
            }
        }
        Ok(())
    }

    /// An edge's curve as the file has it may lie off a face's surface by more than the file's
    /// uncertainty states (an intersection curve written looser than its context). Each use's
    /// gap, between its pcurve on the surface and its edge's curve, is measured along the edge as
    /// the record check measures it, and the edge's tolerance covers it, as a vertex's covers its
    /// gap: the pcurves are as made and the record says how far its geometry agrees.
    fn raise_edges(&mut self, surface: &BSurface, edge_loops: &[Vec<(u32, bool)>], pcs: &[Vec<Curve2>]) -> R<()> {
        let on = SurfaceEval::new(surface).map_err(|m| m.to_string())?;
        for (l, pc) in edge_loops.iter().zip(pcs) {
            for (&(e, _), c) in l.iter().zip(pc) {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let (curve, [t0, t1]) = match &edge.form {
                    Some((c, i)) => (c, *i),
                    None => (&edge.curve, edge.interval),
                };
                let eval = CurveEval::new(curve).map_err(|m| m.to_string())?;
                let mut gap = 0.0f64;
                for k in 0..=EDGE_GAP_SAMPLES {
                    let t = if k == EDGE_GAP_SAMPLES { t1 } else { t0 + (t1 - t0) * (k as f64 / EDGE_GAP_SAMPLES as f64) };
                    // A point a pcurve or surface lacks is the record check's to name.
                    let (Some(q), Some(p)) = (solidcore_ops::pcurve::eval(c, t), eval.point(t)) else { continue };
                    if let Some(s) = on.point(q) {
                        gap = gap.max(s.dist(p));
                    }
                }
                self.cover_edge(e, gap, EDGE_GAP_MARGIN)?;
            }
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
        // A full turn when the edge has one vertex, or two at one point. Two vertices apart bound
        // the arc between them, however short: a 20.4 um arc between vertices a 20.5 um uncertainty
        // does not tell apart is that arc, not a turn around its face.
        let full = closed || ps == pe;
        let mapped = map_edge(&curve, ps, pe, same_sense, closed, full, self.tol).map_err(|m| format!("edge #{id}: {m}"))?;
        if mapped.sense_mismatch {
            self.sense_mismatches += 1;
        }
        let vertices = if mapped.with_step { [vs, ve] } else { [ve, vs] };
        // The gaps as the record will be read: its curve at its interval's ends.
        let eval = CurveEval::new(&mapped.curve).map_err(|m| format!("edge #{id}: {m}"))?;
        let mut gaps = [0.0; 2];
        for ((t, v), g) in mapped.interval.into_iter().zip(vertices).zip(gaps.iter_mut()) {
            let at = eval.point(t).ok_or_else(|| format!("edge #{id}: no point at {t}"))?;
            *g = at.dist(v3(self.point(v)?));
        }
        let (mut curve, mut interval) = (mapped.curve, mapped.interval);
        // Topology first: a straight edge whose vertices lie off its LINE (a direction or a
        // magnitude written wrong) is the segment between its vertices, which the faces it
        // bounds share; any other curve is the edge's shape, and a vertex far off it is refused.
        let far = gaps.iter().any(|g| !(VERTEX_GAP_MARGIN * g <= MAX_TOLERANCE));
        if far && matches!(curve, BCurve::Line { .. }) && !closed && ps != pe {
            let [a, z] = vertices;
            curve = BCurve::Line { start: self.point(a)?, end: self.point(z)? };
            interval = [0.0, 1.0];
            gaps = [0.0; 2];
            self.lines_between_vertices += 1;
        }
        for (v, gap) in vertices.into_iter().zip(gaps) {
            self.raise_vertex(v, gap, id)?;
        }
        let i = index(self.edges.len())?;
        self.edges.push(EdgeB { curve, interval, vertices, tolerance: self.tol, form: None });
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
                Curve::Replica { parent, transform } => return replicate(parent, transform, 0),
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
            "POLY_LOOP" => {
                let mut uses = self.poly_loop(lp_id, edges_so_far)?;
                if !orientation {
                    reverse(&mut uses);
                }
                Ok((LoopB::Edges(uses), outer))
            }
            n => Err(format!("#{lp_id}: {}", unsupported(format!("loop {n}")))),
        }
    }

    /// A POLY_LOOP (ISO 10303-42: a closed polygon of points) as edge uses: a line from each
    /// point to the next and from the last back to the first, a point written twice in a row
    /// skipped. Each point is a vertex and each side an edge, shared with every other loop that
    /// has the same two points, as the faces of a FACETED_BREP share them.
    fn poly_loop(&mut self, lp_id: u64, edges_so_far: &mut usize) -> R<Vec<(u32, bool)>> {
        let lp = entity(self.ex, lp_id)?;
        let mut pts = refs(lp.params().get(1), "poly loop")?;
        pts.dedup();
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        *edges_so_far = edges_so_far.saturating_add(pts.len());
        if pts.len() < 3 || *edges_so_far > MAX_LOOP {
            return Err(format!("poly loop #{lp_id} with {} points", pts.len()));
        }
        let mut uses = Vec::with_capacity(pts.len());
        for (k, &a) in pts.iter().enumerate() {
            let b = *pts.get((k + 1) % pts.len()).ok_or("poly loop point")?;
            let key = (a.min(b), a.max(b));
            let forward = a < b;
            if let Some(&e) = self.poly_edges.get(&key) {
                uses.push((e, forward));
                continue;
            }
            let (va, vb) = (self.point_vertex(key.0)?, self.point_vertex(key.1)?);
            let (pa, pb) = (self.point(va)?, self.point(vb)?);
            if v3(pa).dist(v3(pb)) <= self.tol {
                return Err(format!("poly loop #{lp_id}: a side of no length"));
            }
            let e = index(self.edges.len())?;
            self.edges.push(EdgeB {
                curve: BCurve::Line { start: pa, end: pb },
                interval: [0.0, 1.0],
                vertices: [va, vb],
                tolerance: self.tol,
                form: None,
            });
            self.poly_edges.insert(key, e);
            uses.push((e, forward));
        }
        Ok(uses)
    }

    /// The vertex at CARTESIAN_POINT `id` (a POLY_LOOP's corner), one per point.
    fn point_vertex(&mut self, id: u64) -> R<u32> {
        if let Some(v) = self.vertex_ids.get(&id) {
            return Ok(*v);
        }
        let point = self.reader.point(id)?;
        let i = index(self.vertices.len())?;
        self.vertices.push(Vertex { point, tolerance: self.tol });
        self.vertex_ids.insert(id, i);
        Ok(i)
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
        // A plain FACE carries no surface: it is the plane its loops lie in.
        let implicit = e.name() == "FACE";
        if !matches!(e.name(), "ADVANCED_FACE" | "FACE_SURFACE" | "FACE") {
            return Err(format!("#{id}: {}", unsupported(format!("face {}", e.name()))));
        }
        if !self.face_ids.insert(id) {
            return Err(format!("face #{id} is used twice"));
        }
        *self.budget = self.budget.checked_sub(1).ok_or_else(|| format!("more than {MAX_FILE_FACES} faces in one file"))?;
        if self.faces.len() >= MAX_FACES {
            return Err(format!("more than {MAX_FACES} faces"));
        }
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
        let (step_surface, same_sense) = if implicit {
            if !loops.iter().any(|l| matches!(l, LoopB::Edges(_))) {
                return Err(format!("face #{id}: {}", unsupported("a face without an edge loop")));
            }
            (std::sync::Arc::new(self.loop_plane(&loops).map_err(|m| format!("face #{id}: {m}"))?), true)
        } else {
            let s = self.reader.surface(rid(p.get(2), "face geometry")?).map_err(|m| format!("face #{id}: {m}"))?;
            (s, flag(p.get(3), "face same_sense")?)
        };
        if !loops.iter().any(|l| matches!(l, LoopB::Edges(_))) {
            loops = self.whole_sphere(&step_surface, &loops).map_err(|m| format!("face #{id}: {m}"))?;
        }
        let extent = self.extent(&loops)?;
        let (surface, reversed) = swept::map_surface(&step_surface, &extent, self.tol).map_err(|m| format!("face #{id}: {m}"))?;
        let surface = match closed_within(&surface, self.tol) {
            Some(closed) => {
                self.splines_closed += 1;
                closed
            }
            None => surface,
        };
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
        self.faces.push(FaceB { step: id, surface, loops, outer, forward: sense, extent: None });
        Ok((i, sense))
    }

    /// A sphere face bounded by vertex loops alone, each at one of its poles (ISO 10303-42: a face
    /// of a closed surface bounded by vertices is all of the surface): the whole sphere, its loop
    /// a meridian from the south pole to the north and back, which [`Builder::complete_slits`]
    /// keeps as its seam. Any other face without an edge loop is refused.
    fn whole_sphere(&mut self, surface: &crate::geom::Surface, loops: &[LoopB]) -> R<Vec<LoopB>> {
        let unsupported_here = || unsupported("a face without an edge loop");
        let crate::geom::Surface::Sphere { position, radius } = surface else { return Err(unsupported_here()) };
        let (o, z, x) = (v3(position.origin), v3(position.axis), v3(position.ref_direction));
        let poles = [o - z * *radius, o + z * *radius];
        let mut at = [None, None];
        for l in loops {
            let LoopB::Vertex(v) = l else { return Err(unsupported_here()) };
            let p = v3(self.point(*v)?);
            let tol = self.vertices.get(*v as usize).map_or(self.tol, |x| x.tolerance.max(self.tol));
            let k = poles.iter().position(|q| q.dist(p) <= tol).ok_or_else(unsupported_here)?;
            if let Some(slot) = at.get_mut(k) {
                *slot = Some(*v);
            }
        }
        let mut ends = [0u32; 2];
        for ((end, found), pole) in ends.iter_mut().zip(at).zip(poles) {
            *end = match found {
                Some(v) => v,
                None => {
                    let i = index(self.vertices.len())?;
                    self.vertices.push(Vertex { point: [pole.x, pole.y, pole.z], tolerance: self.tol });
                    i
                }
            };
        }
        let e = index(self.edges.len())?;
        let frame = Frame { origin: position.origin, x_axis: [x.x, x.y, x.z], y_axis: [z.x, z.y, z.z] };
        let half = std::f64::consts::FRAC_PI_2;
        self.edges.push(EdgeB {
            curve: BCurve::Circle { frame, radius: *radius },
            interval: [-half, half],
            vertices: ends,
            tolerance: self.tol,
            form: None,
        });
        Ok(vec![LoopB::Edges(vec![(e, true), (e, false)])])
    }

    /// The plane a plain FACE's loops lie in (ISO 10303-42: a FACE with no face geometry is
    /// bounded by loops in one plane): its normal by Newell's method over the first edge loop's
    /// points (each edge at nine points), so its outer loop runs counter-clockwise about it; its
    /// origin the loop's first point. Refused where the points span no plane or leave it by more
    /// than the tolerance.
    fn loop_plane(&self, loops: &[LoopB]) -> R<crate::geom::Surface> {
        let Some(LoopB::Edges(es)) = loops.iter().find(|l| matches!(l, LoopB::Edges(_))) else { return Err("no edge loop".into()) };
        let mut pts: Vec<Vec3> = Vec::new();
        for &(e, forward) in es {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            for k in 0..8 {
                let f = f64::from(k) / 8.0;
                pts.push(c.point(if forward { lo + (hi - lo) * f } else { hi - (hi - lo) * f }).ok_or("edge point")?);
            }
        }
        let first = *pts.first().ok_or("an empty loop")?;
        let mut normal = Vec3::new(0.0, 0.0, 0.0);
        for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
            normal += (*a - first).cross(*b - first);
        }
        let z = normal.normalized().ok_or_else(|| unsupported("a face with no surface whose loop spans no plane"))?;
        if pts.iter().any(|p| (*p - first).dot(z).abs() > self.tol) {
            return Err(unsupported("a face with no surface whose loop leaves its plane"));
        }
        let far = pts.iter().copied().max_by(|a, b| (*a - first).len().total_cmp(&(*b - first).len())).ok_or("an empty loop")?;
        let d = far - first;
        let x = (d - z * d.dot(z)).normalized().ok_or("a degenerate loop")?;
        Ok(crate::geom::Surface::Plane {
            position: Placement { origin: [first.x, first.y, first.z], axis: [z.x, z.y, z.z], ref_direction: [x.x, x.y, x.z] },
        })
    }

    /// Points along a face's loops (each edge at nine points, and its vertex loops), and along
    /// the edges its loops use twice (its seams).
    fn extent(&self, loops: &[LoopB]) -> R<swept::Extent> {
        let mut ext = swept::Extent::default();
        let mut count: HashMap<u32, usize> = HashMap::new();
        for l in loops {
            match l {
                LoopB::Edges(es) => {
                    let mut walk = Vec::new();
                    for &(e, forward) in es {
                        *count.entry(e).or_default() += 1;
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                        let [lo, hi] = edge.interval;
                        for k in 0..8 {
                            let s = f64::from(k) / 8.0;
                            walk.push(c.point(if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s }).ok_or("edge point")?);
                        }
                    }
                    if let Some(&p) = walk.first() {
                        walk.push(p);
                    }
                    ext.walks.push(walk);
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

    /// The faces of shell `id` but those `left_out`, and whether any was left out.
    fn shell(&mut self, id: u64, left_out: &HashSet<u64>) -> R<(Vec<(u32, bool)>, bool)> {
        let (faces, flip) = shell_faces(self.ex, id)?;
        let mut out = Vec::with_capacity(faces.len());
        let mut skipped = false;
        for f in faces {
            if !left_out.is_empty() && (left_out.contains(&f) || left_out.contains(&self.face_entity(f, 0)?)) {
                skipped = true;
                continue;
            }
            let (face, forward) = self.face(f, 0)?;
            out.push((face, forward != flip));
        }
        Ok((out, skipped))
    }

    /// The ADVANCED_FACE or FACE_SURFACE a shell's face `id` is (through its ORIENTED_FACEs).
    fn face_entity(&self, id: u64, depth: usize) -> R<u64> {
        if depth > MAX_NESTING {
            return Err("oriented faces nested too deeply".into());
        }
        let e = entity(self.ex, id)?;
        if e.name() == "ORIENTED_FACE" {
            return self.face_entity(rid(e.params().get(2), "oriented face element")?, depth + 1);
        }
        Ok(id)
    }

    /// Pcurves, outer loops, domains and the canonical order: the finished record.
    fn finish(mut self, item: u64, name: String, shells: Vec<Vec<(u32, bool)>>, kind: Kind) -> R<Solid> {
        let mut notes = Vec::new();
        // A face bounded by a slit alone is the whole of its closed surface, closed by its seams
        // (a sphere's frame put on the slit), before anything is measured on the frame or split
        // at its poles.
        self.complete_slits()?;
        let slits = self.drop_inner_slits();
        if slits > 0 {
            notes.push(format!("{slits} slits of no area left out of the faces they lie in"));
        }
        let at_poles = self.split_at_poles()?;
        if at_poles > 0 {
            notes.push(format!("{at_poles} edges split where they pass through a face's pole"));
        }
        self.start_splines_clear_of_faces()?;
        let seams = self.add_seams()?;
        if seams > 0 {
            notes.push(format!("{seams} seam edges added to faces that wind around their surface"));
        }
        if self.splines_closed > 0 {
            notes.push(format!(
                "{} spline surfaces whose ends meet within {} mm closed on themselves (their end control lines made one)",
                self.splines_closed, self.tol
            ));
        }
        if self.inner_loops_turned > 0 {
            notes.push(format!("{} inner loops written the way their outer loop runs turned round", self.inner_loops_turned));
        }
        if self.lines_between_vertices > 0 {
            notes.push(format!("{} line edges whose vertices lie off their LINE read as the segment between them", self.lines_between_vertices));
        }
        if self.sense_mismatches > 0 {
            notes.push(format!("{} edges run against their same_sense (read by their vertices)", self.sense_mismatches));
        }
        // Round faces whose forms would run past one turn start on a quarter line of their
        // frames, before their edges are labelled on them.
        self.turn_frames()?;
        // Each edge's label and NURBS form: a circle about a revolved face's axis labelled on that
        // face's frame, so its form lies on the face's quarter grid.
        // A circle round a torus face's tube is labelled from its tube start likewise.
        let mut frames: Vec<Vec<brep::Frame>> = vec![Vec::new(); self.edges.len()];
        let mut tori: Vec<Vec<BSurface>> = vec![Vec::new(); self.edges.len()];
        for f in &self.faces {
            let Some(frame) = crate::kernel::form::frame_of(&f.surface) else { continue };
            let torus = matches!(f.surface, BSurface::Torus { .. });
            for l in &f.loops {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        if let Some(slot) = frames.get_mut(*e as usize) {
                            slot.push(frame.clone());
                        }
                        if torus && let Some(slot) = tori.get_mut(*e as usize) {
                            slot.push(f.surface.clone());
                        }
                    }
                }
            }
        }
        for ((edge, fr), tr) in self.edges.iter_mut().zip(&frames).zip(&tori) {
            let (label, interval, form) = crate::kernel::form::edge_on(&edge.curve, edge.interval, fr, tr, self.tol)
                .ok_or_else(|| unsupported("an arc of more than one turn"))?;
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
        if !self.raised_edges.is_empty() {
            let most = self.raised_edges.iter().filter_map(|&e| self.edges.get(e as usize)).map(|e| e.tolerance).fold(0.0, f64::max);
            notes.push(format!(
                "{} edges lie off their faces' surfaces by more than {} mm: their tolerances are the gaps they have (up to {most:.3e} mm)",
                self.raised_edges.len(),
                self.tol
            ));
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
        // A void's faces face into the cavity, out of the material: it encloses volume of the
        // opposite sign to the outer shell's. The ORIENTED_CLOSED_SHELL flag alone doesn't say
        // so: some exporters write .F. over a shell already facing into the cavity (#3730),
        // SolidCraft .T.; read by the flag, such a void added its cavity's volume to the solid's.
        // A void enclosing volume of the outer shell's sign is turned inside out, the signs the
        // faces' own surfaces and pcurves give (`face_flux` of p / 3, the divergence theorem).
        let shells = if kind == Kind::Voids && closed.iter().all(|c| *c) {
            let volume = |s: &[(u32, bool)]| -> Option<f64> {
                let mut sum = 0.0;
                for &(f, forward) in s {
                    let face = self.faces.get(f as usize)?;
                    let on = face_forms.get(f as usize)?.as_ref().unwrap_or(&face.surface);
                    let eval = SurfaceEval::new(on).ok()?;
                    let edge_loops = face.loops.iter().filter_map(|l| match l {
                        LoopB::Edges(es) => Some(es),
                        LoopB::Vertex(_) => None,
                    });
                    let uses: Vec<Vec<(&Curve2, bool)>> =
                        edge_loops.zip(pcs.get(f as usize)?).map(|(es, pc)| pc.iter().zip(es).map(|(c, (_, fwd))| (c, *fwd)).collect()).collect();
                    let flux = face_flux(&eval, domains.get(f as usize)?[1][0], &uses, &|p| p * (1.0 / 3.0))?;
                    sum += if forward { flux } else { -flux };
                }
                sum.is_finite().then_some(sum)
            };
            let outside = shells.first().and_then(|s| volume(s));
            shells
                .into_iter()
                .enumerate()
                .map(|(k, s)| match (outside, k > 0) {
                    (Some(o), true) if volume(&s).is_some_and(|v| v * o > 0.0) => s.into_iter().map(|(f, fwd)| (f, !fwd)).collect(),
                    _ => s,
                })
                .collect()
        } else {
            shells
        };
        let mut regions = vec![Region { shells: Vec::new(), void: false }];
        match kind {
            Kind::Solid | Kind::Voids => {
                if let Some(k) = closed.iter().position(|c| !c) {
                    return Err(format!("{} shell is not closed", if k == 0 { "the outer" } else { "a void" }));
                }
                if let Some(r) = regions.first_mut() {
                    r.shells.push(0);
                }
                regions.push(Region { shells: (0..index(shells.len())?).collect(), void: false });
                for v in 1..shells.len() {
                    regions.push(Region { shells: vec![index(v)?], void: true });
                }
            }
            Kind::Open { voids } => {
                if let Some(r) = regions.first_mut() {
                    r.shells = (0..index(shells.len())?).collect();
                }
                // A closed shell bounds two regions: the cavity inside a void still whole, or
                // the inside of an outer shell or a model's shell still whole.
                for (k, c) in closed.iter().enumerate() {
                    if *c {
                        regions.push(Region { shells: vec![index(k)?], void: voids && k > 0 });
                    }
                }
            }
            Kind::Model => {
                if let Some(r) = regions.first_mut() {
                    r.shells = (0..index(shells.len())?).collect();
                }
                for (k, c) in closed.iter().enumerate() {
                    if *c {
                        regions.push(Region { shells: vec![index(k)?], void: false });
                    }
                }
            }
        }
        let sheet = matches!(kind, Kind::Open { .. }) || !closed.iter().any(|c| *c);

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
                    if !(VERTEX_GAP_MARGIN * gap <= MAX_TOLERANCE) {
                        return Err(format!("face #{}: a vertex loop's vertex lies {gap} mm off its surface", face.step));
                    }
                    x.tolerance = x.tolerance.max(VERTEX_GAP_MARGIN * gap);
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
        solidcore_ops::record_check::validate(&brep).map_err(|e| format!("the record is invalid: {e}"))?;
        solidcore_ops::record_check::agreement(&brep).map_err(|e| format!("the record's geometry disagrees: {e}"))?;
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

    /// A face's box on its label over its edge loops (`crate::kernel::form::extent`), a boundary
    /// point on a pole within the face's tolerance (the largest of its edges' and vertices': a
    /// cone's apex or a sphere's pole a file places a few µm off); `None` where the boundary does
    /// not project onto it.
    fn face_extent(&self, fi: usize) -> R<Option<[[f64; 2]; 2]>> {
        let face = self.faces.get(fi).ok_or("face reference")?;
        let vertex_tolerance = |v: u32| self.vertices.get(v as usize).map(|x| x.tolerance).ok_or("vertex reference");
        let (mut boundary, mut reach) = (Vec::with_capacity(face.loops.len()), self.tol);
        for l in &face.loops {
            let es = match l {
                LoopB::Edges(es) => es,
                LoopB::Vertex(v) => {
                    reach = reach.max(vertex_tolerance(*v)?);
                    continue;
                }
            };
            let mut b = Vec::with_capacity(es.len());
            for &(e, forward) in es {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let [v0, v1] = edge.vertices;
                reach = reach.max(edge.tolerance).max(vertex_tolerance(v0)?).max(vertex_tolerance(v1)?);
                b.push((&edge.curve, edge.interval, forward));
            }
            boundary.push(b);
        }
        Ok(crate::kernel::form::extent(&face.surface, &boundary, reach))
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
        let form = match crate::kernel::form::frame_of(&label) {
            Some(_) => {
                let extent = match face.extent {
                    Some(e) => Some(e),
                    None => self.face_extent(fi)?,
                };
                let extent = extent.ok_or_else(|| unsupported("a face whose boundary does not lie on its surface"))?;
                let (f, d) = crate::kernel::form::surface_within(&label, extent, tol).ok_or_else(|| unsupported("a face of more than one turn"))?;
                Some((f, d, extent))
            }
            None => None,
        };
        let (form, box_on_form, extent) = match form {
            Some((f, d, e)) => (Some(f), Some(d), Some(e)),
            None => (None, None, None),
        };
        let surface = form.clone().unwrap_or_else(|| label.clone());
        // How far each edge lies off this surface, so its closed form is checked at that.
        self.measure_edges(&label, &edge_loops)?;
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
        let mut pcs =
            pcurve::pcurves_within(&surface, &uses_of(&self.edges, &edge_loops, made.as_ref())?, tol, box_on_form).map_err(|e| e.to_string())?;
        // A form ends on a quarter line its face's boundary runs past by up to the tolerance: the
        // pcurves are put on its range, as the face's domain is ([`face_domain`]), and the edges'
        // tolerances cover the move.
        if let (Some(f), Some(e)) = (&form, extent) {
            onto_form(f, &label, e, tol, &mut pcs);
        }
        self.raise_edges(&surface, &edge_loops, &pcs)?;
        // Signed areas of the loops that close in (u, v).
        let periods = natural_periods(&surface);
        // Each use starts near where the one before it ends, along a bounded direction that
        // does not wrap: a loop around a closed spline surface without a seam edge jumps across
        // its domain.
        let natural = solidcore_ops::label::surface_natural(&surface);
        // At a pole (where the surface collapses to a point across a direction, to within the
        // tolerance of the vertex the two uses meet at) a loop may jump in that direction: the
        // seam's two uses meet there a period apart, and a side collapsed to within the vertex's
        // tolerance (a corner patch with one side shrunk to a few µm) is run along between them,
        // its boundary iso line. As the record check (`brep::geometry`) takes it.
        let poles = SurfaceEval::new(&surface).ok();
        let at_pole = |uv: [f64; 2], reach: f64| -> R<bool> {
            Ok(match &poles {
                Some(e) => e.pole_within(uv, reach)?.iter().any(|p| *p),
                None => false,
            })
        };
        for (l, pc) in edge_loops.iter().zip(&pcs) {
            let mut ends = Vec::with_capacity(l.len());
            for (&(_, forward), c) in l.iter().zip(pc) {
                let [a, b] = c.interval().ok_or("pcurve interval")?;
                let (s, e) = (solidcore_ops::pcurve::eval(c, a).ok_or("pcurve start")?, solidcore_ops::pcurve::eval(c, b).ok_or("pcurve end")?);
                ends.push(if forward { (s, e) } else { (e, s) });
            }
            for k in 0..ends.len() {
                let (Some((start, _)), Some((_, end))) = (ends.get(k), ends.get(if k == 0 { ends.len() - 1 } else { k - 1 })) else { continue };
                // The vertex use k starts at, where the one before it ends.
                let &(e, forward) = l.get(k).ok_or("edge use")?;
                let v = self.edges.get(e as usize).ok_or("edge reference")?.vertices;
                let v = if forward { v[0] } else { v[1] };
                let reach = self.vertices.get(v as usize).ok_or("vertex reference")?.tolerance.max(tol);
                if at_pole(*start, reach)? || at_pole(*end, reach)? {
                    continue;
                }
                for d in 0..2 {
                    // Only a bounded direction has a seam to jump; half its width is no gap.
                    let Some(Natural::Range(a, b)) = natural.get(d) else { continue };
                    if !(b - a).is_finite() {
                        continue;
                    }
                    let (Some(sd), Some(ed)) = (start.get(d), end.get(d)) else { continue };
                    if !((sd - ed).abs() <= 0.5 * (b - a)) {
                        // Whether the surface closes across `d` there: its two boundaries meet.
                        let ends_at = |x: f64| {
                            let mut q = *start;
                            if let Some(c) = q.get_mut(d) {
                                *c = x;
                            }
                            poles.as_ref().and_then(|s| s.point(q))
                        };
                        let closed = matches!((ends_at(*a), ends_at(*b)), (Some(p), Some(q)) if p.dist(q) <= reach);
                        return Err(unsupported(if closed {
                            "a loop across a closed spline surface's seam without a seam edge".to_string()
                        } else {
                            format!(
                                "a loop that jumps across its spline surface's {} range where the surface neither closes nor collapses to a point",
                                if d == 0 { "u" } else { "v" }
                            )
                        }));
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
        let clockwise = shapes.get(outer).copied().flatten().is_some_and(|a| a < 0.0);
        // A face bounded by seam uses alone (a whole torus within its two seams) has loops that
        // run each edge both ways: run backwards they are the same uses, so they say nothing of
        // the face's side. One running clockwise is run the other way; the face keeps its side.
        let seams_only = edge_loops.iter().all(|l| seam_only(l));
        let flip = can_turn && clockwise && !seams_only;
        let rerun = flip || (clockwise && seams_only);
        if rerun {
            for l in &mut face.loops {
                if let LoopB::Edges(es) = l {
                    reverse(es);
                }
            }
        }
        if flip {
            face.forward = !face.forward;
        }
        let mut pcs: Vec<Vec<Curve2>> = if rerun { pcs.into_iter().map(|l| l.into_iter().rev().collect()).collect() } else { pcs };
        // An inner bound written the way the outer one runs (its FACE_BOUND orientation wrong):
        // on a face whose loops all close in (u, v), each loop but the outer runs clockwise, so
        // such a loop is run the other way. Its uses and their pcurves are the same.
        if can_turn && !seams_only && shapes.iter().all(Option::is_some) {
            for (i, area) in shapes.iter().enumerate() {
                let Some(a) = *area else { continue };
                let a = if rerun { -a } else { a };
                if i == outer || !(a > 0.0) {
                    continue;
                }
                if let Some(LoopB::Edges(es)) = positions.get(i).and_then(|&p| face.loops.get_mut(p)) {
                    reverse(es);
                }
                if let Some(l) = pcs.get_mut(i) {
                    l.reverse();
                }
                self.inner_loops_turned += 1;
            }
        }
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
            u.push(EdgeUse { edge: e, curve, interval, forward, tolerance: edge.tolerance, given });
        }
        uses.push(u);
    }
    Ok(uses)
}

/// A spline surface's parameter box: its knot ranges.
fn natural_box(s: &BSurface) -> Option<[[f64; 2]; 2]> {
    match solidcore_ops::label::surface_natural(s) {
        [Natural::Range(a, b), Natural::Range(c, d)] if [a, b, c, d].iter().all(|x| x.is_finite()) => Some([[a, b], [c, d]]),
        _ => None,
    }
}

/// Every edge of the loop is used in it exactly twice, once each way: a loop of seam uses alone.
fn seam_only(uses: &[(u32, bool)]) -> bool {
    let mut ways: HashMap<u32, [u32; 2]> = HashMap::with_capacity(uses.len() / 2);
    for &(e, forward) in uses {
        if let Some(n) = ways.entry(e).or_default().get_mut(usize::from(forward)) {
            *n += 1;
        }
    }
    !uses.is_empty() && ways.values().all(|n| *n == [1, 1])
}

/// The loop run the other way.
fn reverse(uses: &mut [(u32, bool)]) {
    uses.reverse();
    for u in uses {
        u.1 = !u.1;
    }
}

fn natural_periods(s: &BSurface) -> [bool; 2] {
    solidcore_ops::label::surface_natural(s).map(|n| matches!(n, Natural::Periodic))
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
            pts.push(solidcore_ops::pcurve::eval(c, t)?);
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

/// The angles `tolerance` (mm) makes along the two directions of a face on `label` over the
/// height `v`: on the widest circle of the face about the axis (a cylinder or cone), and on a
/// sphere's meridian or a torus's tube. `form::surface_within` ends a face's form on a quarter
/// line its domain ends within these angles of; zero along a direction that is no angle.
fn form_reach(label: &BSurface, v: [f64; 2], tolerance: f64) -> [f64; 2] {
    let reach = |radius: f64| if tolerance > 0.0 && radius > 0.0 && radius.is_finite() { tolerance / radius } else { 0.0 };
    match label {
        BSurface::Cylinder { radius, .. } => [reach(radius.abs()), 0.0],
        BSurface::Cone { radius_at_origin, half_angle, .. } => {
            let slope = half_angle.tan();
            [reach(v.iter().map(|h| (radius_at_origin + h * slope).abs()).fold(0.0, f64::max)), 0.0]
        }
        BSurface::Sphere { radius, .. } => [reach(radius.abs()), reach(radius.abs())],
        BSurface::Torus { major_radius, minor_radius, .. } => [reach(major_radius.abs() + minor_radius.abs()), reach(minor_radius.abs())],
        _ => [0.0, 0.0],
    }
}

/// A face's pcurves on the form `form::surface_within` made for it (`form`, over `domain` on
/// the label `label`), put on the form's range. Where the domain ends within `tolerance` of a
/// quarter's end the form ends there, but the boundary still lies where its edges do, as much as
/// `tolerance` past it: a pcurve control point past the form's end by no more than the form
/// parameter of that angle (less than the angle: a quarter's parameter changes by at most 1/√2
/// per radian, [`form_reach`]) is moved onto the end. A B-spline is a convex combination of its
/// control points, so each pcurve then lies on the form, and the face's domain, which ends at
/// the form's, holds it; the pcurve moves by no more than its points did, a gap the edges'
/// tolerances are measured to cover (`raise_edges`). A point further out is left as it is, for
/// the record check to refuse. The number of control point coordinates moved.
fn onto_form(form: &BSurface, label: &BSurface, domain: [[f64; 2]; 2], tolerance: f64, pcs: &mut [Vec<Curve2>]) -> usize {
    let reach = form_reach(label, domain[1], tolerance);
    let ranges = solidcore_ops::label::surface_natural(form);
    let mut moved = 0;
    let mut onto = |p: &mut [f64; 2]| {
        for (d, reach) in reach.iter().enumerate() {
            let (Some(Natural::Range(a, b)), Some(x)) = (ranges.get(d), p.get_mut(d)) else { continue };
            let end = if *x < *a {
                *a
            } else if *x > *b {
                *b
            } else {
                continue;
            };
            if (*x - end).abs() <= *reach {
                *x = end;
                moved += 1;
            }
        }
    };
    for c in pcs.iter_mut().flatten() {
        match c {
            Curve2::Spline { points, .. } => points.iter_mut().for_each(&mut onto),
            Curve2::RationalSpline { homogeneous, .. } => {
                for h in homogeneous.iter_mut().filter(|h| h[2] > 0.0) {
                    let mut p = [h[0] / h[2], h[1] / h[2]];
                    let before = p;
                    onto(&mut p);
                    if p != before {
                        (h[0], h[1]) = (p[0] * h[2], p[1] * h[2]);
                    }
                }
            }
        }
    }
    moved
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
            for ((lo, hi), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *lo = lo.min(x);
                *hi = hi.max(x);
            }
        }
    }
    let natural = solidcore_ops::label::surface_natural(surface);
    let mut out = [[0.0; 2]; 2];
    for (d, (a, b)) in lo.into_iter().zip(hi).enumerate() {
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
            // A direction not closed on itself: the box is the reader's to choose within the
            // natural range (all of it for a plane, or along a cylinder's axis), and SMLib
            // refuses a surface whose opposite sides lie within its tolerance of each other
            // (`SmSurface` test #6, "partially coincident boundaries"), so a sliver face's box
            // is at least `OPEN_DOMAIN_SPAN` across where its range has room: widened about its
            // middle, and shifted off a bound it would cross. Pcurves wholly outside the range
            // stay an empty box.
            Some(Natural::Range(n0, n1)) => {
                x = x.max(*n0);
                y = y.min(*n1);
                if x < y && y - x < OPEN_DOMAIN_SPAN {
                    let span = OPEN_DOMAIN_SPAN.min(*n1 - *n0);
                    // the box's latest start: the range's own where the range is narrower than
                    // the span (`n1 - span` there is `n0`, which rounding may put below it)
                    let last = if span < OPEN_DOMAIN_SPAN { *n0 } else { *n1 - span };
                    // the range is the surface's, from the file: refused where it leaves no
                    // room (crossed, or not a number), never clamped into one
                    if !(*n0 <= last) {
                        return Err(format!("a face's natural range [{n0}, {n1}] leaves no room for a box {span} across"));
                    }
                    let lo = (0.5 * (x + y) - 0.5 * span).clamp(*n0, last);
                    (x, y) = (lo, (lo + span).min(*n1));
                }
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
    let homogeneous: Vec<[f64; 4]> = match &b.weights {
        None => b.points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        Some(w) => {
            if w.len() != b.points.len() {
                return Err("B-spline weights and points differ in number".into());
            }
            b.points.iter().zip(w).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect()
        }
    };
    let (knots, homogeneous) = clamped(b.degree, knots, homogeneous).map_err(|e| format!("an unclamped B-spline curve: {e}"))?;
    Ok(match &b.weights {
        None => BCurve::Spline { knots, points: homogeneous.iter().map(|h| [h[0], h[1], h[2]]).collect() },
        Some(_) => BCurve::RationalSpline { knots, homogeneous },
    })
}

/// A spline of `degree` on `knots` over `homogeneous` control points with an unclamped knot
/// vector (a periodic spline written with uniform knots, its first points repeated at its end) as
/// the same spline over [knots[degree], knots[n]] clamped there, exactly (knot insertion): its
/// parameters unchanged, its ends its own points, as every evaluator and pcurve takes a spline,
/// and a closed one's first and last points one. A clamped one comes back as it is.
fn clamped(degree: usize, knots: Vec<f64>, homogeneous: Vec<[f64; 4]>) -> Result<(Vec<f64>, Vec<[f64; 4]>), String> {
    let (p, n) = (degree, homogeneous.len());
    let (lo, hi) = (knots.get(p).copied(), knots.get(n).copied());
    let repeats = |at: &[f64], x: Option<f64>| x.is_some_and(|x| at.iter().all(|k| *k == x));
    match (knots.get(..=p), knots.get(n..), lo, hi) {
        (Some(head), Some(tail), Some(a), Some(z)) if !(repeats(head, lo) && repeats(tail, hi)) => {
            solidcore::eval::pcurve::segment(p, &knots, &homogeneous, a, z).map_err(|e| e.to_string())
        }
        _ => Ok((knots, homogeneous)),
    }
}

/// Each of `lines`, a spline of `degree` on `knots`, clamped ([`clamped`]), and the knots they
/// share after it.
fn clamped_lines(degree: usize, knots: Vec<f64>, lines: Vec<Vec<[f64; 4]>>) -> R<(Vec<f64>, Vec<Vec<[f64; 4]>>)> {
    let mut out = Vec::new();
    let mut shared = None;
    for line in lines {
        let (k, c) = clamped(degree, knots.clone(), line).map_err(|e| format!("an unclamped B-spline surface: {e}"))?;
        shared = Some(k);
        out.push(c);
    }
    Ok((shared.unwrap_or(knots), out))
}

/// `grid`'s columns as rows.
fn transposed(grid: &[Vec<[f64; 4]>]) -> Vec<Vec<[f64; 4]>> {
    let width = grid.first().map_or(0, Vec::len);
    (0..width).map(|j| grid.iter().filter_map(|r| r.get(j).copied()).collect()).collect()
}

fn map_bspline_surface(b: &BSplineSurface) -> R<BSurface> {
    let nu = b.points.len();
    let nv = b.points.first().map_or(0, Vec::len);
    let u_knots = full_knots(&b.u_knots, &b.u_multiplicities, nu, b.u_degree)?;
    let v_knots = full_knots(&b.v_knots, &b.v_multiplicities, nv, b.v_degree)?;
    if b.points.iter().any(|r| r.len() != nv) {
        return Err("ragged B-spline surface".into());
    }
    if let Some(w) = &b.weights
        && (w.len() != nu || w.iter().any(|r| r.len() != nv))
    {
        return Err("B-spline surface weights and points differ in number".into());
    }
    // Control point (i, j) is row i (along u), entry j (along v), homogeneous.
    let weight = |i: usize, j: usize| b.weights.as_ref().and_then(|w| w.get(i)?.get(j).copied()).unwrap_or(1.0);
    let homogeneous = |i: usize, j: usize, p: &[f64; 3]| {
        let w = weight(i, j);
        [p[0] * w, p[1] * w, p[2] * w, w]
    };
    let rows: Vec<Vec<[f64; 4]>> =
        b.points.iter().enumerate().map(|(i, r)| r.iter().enumerate().map(|(j, p)| homogeneous(i, j, p)).collect()).collect();
    // Unclamped knots in either direction are clamped exactly, line by line: every row is a
    // spline in v on the v knots, every column one in u on the u knots, and inserting the same
    // knots into each gives the surface's.
    let (v_knots, rows) = clamped_lines(b.v_degree, v_knots, rows)?;
    let (u_knots, columns) = clamped_lines(b.u_degree, u_knots, transposed(&rows))?;
    let rows = transposed(&columns);
    Ok(match &b.weights {
        None => BSurface::Spline { u_knots, v_knots, points: rows.iter().map(|r| r.iter().map(|h| [h[0], h[1], h[2]]).collect()).collect() },
        Some(_) => BSurface::RationalSpline { u_knots, v_knots, homogeneous: rows },
    })
}

/// A spline surface whose first and last control lines along one parameter are the same to
/// within `tol` (mm) but not exactly (a file that closes a surface on itself to its own
/// uncertainty, not to the last digit), with those two lines made one, their mean: the surface
/// moves by at most half the gap, and it closes on itself, so a face on it reads as on any
/// closed surface (a seam where its loops wind round it, its pcurves on one side of where the
/// surface starts). Only where the knots are clamped at both ends of that parameter (elsewhere
/// the end lines are not the surface's ends), and only where the surface moves by at most `tol`,
/// weights included. A surface no wider than a few `tol` across that parameter, one meeting
/// itself both ways, or one the kernel already takes as closed is left as it is (`None`).
fn closed_within(s: &BSurface, tol: f64) -> Option<BSurface> {
    let (u_knots, v_knots, rows, rational): (&Vec<f64>, &Vec<f64>, Vec<Vec<[f64; 4]>>, bool) = match s {
        BSurface::Spline { u_knots, v_knots, points } => {
            (u_knots, v_knots, points.iter().map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect(), false)
        }
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => (u_knots, v_knots, homogeneous.clone(), true),
        _ => return None,
    };
    let point = |h: &[f64; 4]| (h[3] > 0.0).then(|| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
    // The end control lines are the surface's ends only where its knots are clamped there
    // (each end knot repeated degree + 1 times).
    let clamped = |knots: &[f64], count: usize| -> bool {
        let Some(p) = knots.len().checked_sub(count + 1) else { return false };
        let (Some(head), Some(tail)) = (knots.get(..=p), knots.get(knots.len() - 1 - p..)) else { return false };
        head.windows(2).all(|w| w.first() == w.last()) && tail.windows(2).all(|w| w.first() == w.last())
    };
    let counts = [rows.len(), rows.first().map_or(0, Vec::len)];
    let ends_clamped = [clamped(u_knots, counts[0]), clamped(v_knots, counts[1])];
    // The control net's size: no point of the surface lies farther than it from a control point.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in rows.iter().flatten().filter_map(point) {
        for (k, x) in [p.x, p.y, p.z].into_iter().enumerate() {
            if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                (*l, *h) = (l.min(x), h.max(x));
            }
        }
    }
    let size = v3(hi).dist(v3(lo));
    let least_weight = rows.iter().flatten().map(|h| h[3]).fold(f64::INFINITY, f64::min);
    // Each direction's two end lines and a line across the middle, as lists of control points.
    let columns = transposed(&rows);
    let lines = |d: usize| -> Option<(&Vec<[f64; 4]>, &Vec<[f64; 4]>, &Vec<[f64; 4]>)> {
        let set = if d == 0 { &rows } else { &columns };
        Some((set.first()?, set.get(set.len() / 2)?, set.last()?))
    };
    let meets = |d: usize| -> Option<bool> {
        let (a, mid, b) = lines(d)?;
        if a.len() != b.len() || a.is_empty() || !ends_clamped.get(d).copied().unwrap_or(false) {
            return Some(false);
        }
        let (mut gap, mut width, mut weights) = (0.0_f64, 0.0_f64, 0.0_f64);
        for ((x, y), m) in a.iter().zip(b).zip(mid) {
            let (p, q, r) = (point(x)?, point(y)?, point(m)?);
            gap = gap.max(p.dist(q));
            width = width.max(p.dist(r));
            weights = weights.max((x[3] - y[3]).abs());
        }
        // The mean moves each control point by half the gap, so the surface by no more (its
        // points are convex combinations of them); halving a weight difference δ moves it by at
        // most δ / 2 × the net's size / its least weight. Together within the tolerance.
        let by_weights = 0.5 * weights * size / least_weight;
        Some(gap > 0.0 && gap <= tol && width > 10.0 * tol && 0.5 * gap + by_weights <= tol)
    };
    let d = match (meets(0)?, meets(1)?) {
        (true, false) => 0,
        (false, true) => 1,
        _ => return None,
    };
    // Already closed as the kernel takes it (its ends the same to rounding).
    if SurfaceEval::new(s).ok()?.domain().period.get(d).copied().flatten().is_some() {
        return None;
    }
    let mean = |x: &[f64; 4], y: &[f64; 4]| [0.5 * (x[0] + y[0]), 0.5 * (x[1] + y[1]), 0.5 * (x[2] + y[2]), 0.5 * (x[3] + y[3])];
    let mut rows = rows;
    if d == 0 {
        let (Some(a), Some(b)) = (rows.first().cloned(), rows.last().cloned()) else { return None };
        let m: Vec<[f64; 4]> = a.iter().zip(&b).map(|(x, y)| mean(x, y)).collect();
        if let Some(r) = rows.first_mut() {
            r.clone_from(&m);
        }
        if let Some(r) = rows.last_mut() {
            *r = m;
        }
    } else {
        for r in &mut rows {
            let (Some(x), Some(y)) = (r.first().copied(), r.last().copied()) else { return None };
            let m = mean(&x, &y);
            if let Some(p) = r.first_mut() {
                *p = m;
            }
            if let Some(p) = r.last_mut() {
                *p = m;
            }
        }
    }
    let (u_knots, v_knots) = (u_knots.clone(), v_knots.clone());
    Some(if rational {
        BSurface::RationalSpline { u_knots, v_knots, homogeneous: rows }
    } else {
        BSurface::Spline { u_knots, v_knots, points: rows.iter().map(|r| r.iter().map(|h| [h[0], h[1], h[2]]).collect()).collect() }
    })
}

/// A CURVE_REPLICA's curve: the parent (under its SURFACE_CURVE and TRIMMED_CURVE wrappers)
/// moved by `t`, as the same kind of curve, with the parent's parameter. A mirroring operator
/// would turn a conic over, so it is not supported.
fn replicate(parent: &Curve, t: &Transform, depth: usize) -> R<Curve> {
    if depth > MAX_NESTING {
        return Err("curve replicas nested too deeply".into());
    }
    if !t.is_proper() {
        return Err(unsupported("a CURVE_REPLICA that mirrors"));
    }
    let place = |p: &Placement| Placement { origin: t.point(p.origin), axis: t.direction(p.axis), ref_direction: t.direction(p.ref_direction) };
    let k = t.scale;
    Ok(match parent {
        Curve::Line { origin, direction, magnitude } => {
            Curve::Line { origin: t.point(*origin), direction: t.direction(*direction), magnitude: magnitude * k }
        }
        Curve::Circle { position, radius } => Curve::Circle { position: place(position), radius: radius * k },
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => {
            Curve::Ellipse { position: place(position), semi_axis_1: semi_axis_1 * k, semi_axis_2: semi_axis_2 * k }
        }
        Curve::Hyperbola { position, semi_axis, semi_imag_axis } => {
            Curve::Hyperbola { position: place(position), semi_axis: semi_axis * k, semi_imag_axis: semi_imag_axis * k }
        }
        Curve::Parabola { position, focal_dist } => Curve::Parabola { position: place(position), focal_dist: focal_dist * k },
        Curve::BSpline(b) => {
            let mut moved = (**b).clone();
            moved.points = b.points.iter().map(|p| t.point(*p)).collect();
            Curve::BSpline(Box::new(moved))
        }
        Curve::Polyline(points) => Curve::Polyline(points.iter().map(|p| t.point(*p)).collect()),
        Curve::SurfaceCurve { curve_3d: inner, .. } | Curve::Trimmed { basis: inner, .. } => replicate(inner, t, depth + 1)?,
        Curve::Replica { parent, transform } => replicate(&replicate(parent, transform, depth + 1)?, t, depth + 1)?,
        Curve::OffsetCurve3d { .. } => return Err(unsupported("a CURVE_REPLICA of an OFFSET_CURVE_3D")),
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
/// way it runs. `closed`: the same vertex at both ends; `full`: the ends are one point (a full
/// turn on a periodic curve).
fn map_edge(curve: &Curve, ps: P3, pe: P3, same_sense: bool, closed: bool, full: bool, tol: f64) -> R<Mapped> {
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
            conic(BCurve::Circle { frame: frame(position), radius: *radius }, position, [*radius; 2], ps, pe, same_sense, full)
        }
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => {
            let radii = [*semi_axis_1, *semi_axis_2];
            conic(BCurve::Ellipse { frame: frame(position), radii }, position, radii, ps, pe, same_sense, full)
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
        Curve::Replica { parent, transform } => map_edge(&replicate(parent, transform, 0)?, ps, pe, same_sense, closed, full, tol),
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
fn conic(curve: BCurve, position: &Placement, radii: [f64; 2], ps: P3, pe: P3, same_sense: bool, full: bool) -> R<Mapped> {
    let y = position.y();
    let param = |p: P3| {
        let q = sub(p, position.origin);
        angle_of(dot(q, position.ref_direction) / radii[0], dot(q, y) / radii[1])
    };
    let (ts, te) = (param(ps), param(pe));
    // Along the curve from `from` to `to`, increasing.
    let span = |from: f64, to: f64| if full { TAU } else { turn(to - from) };
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
    // A vertex at the seam: its parameter there, or its point within the tolerance of the
    // curve's ends (a vertex a few µm off the seam projects a hair inside either end).
    let (vf, vt) = if same_sense { (ps, pe) } else { (pe, ps) };
    let near_seam = |v: P3| v3(v).dist(pa).min(v3(v).dist(pb)) <= tol;
    if !(from + eps < to) {
        let (start_at_end, end_at_start) = (b - from <= eps, to - a <= eps);
        let (start_near, end_near) = (start_at_end || near_seam(vf), end_at_start || near_seam(vt));
        // Both ends at the seam yet not its two ends exactly: an edge a few µm long across the
        // seam, which no interval of the curve is (taking both ends to the seam would make it
        // the whole curve).
        if start_near && end_near && !(start_at_end && end_at_start) {
            return Err(unsupported("a short edge across a closed spline's seam"));
        }
        if start_near {
            from = a;
        }
        if end_near {
            to = b;
        }
    }
    if (from - to).abs() <= eps {
        return Err("a degenerate edge on a closed spline".into());
    }
    if from > to {
        let curve = crate::kernel::restart_curve(&curve, from)?;
        return Ok(Mapped { curve, interval: [from, to + (b - a)], with_step: same_sense, sense_mismatch: false });
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
    // Keep the best sampled parameter: an exact point on an interior knot must
    // not be replaced by a less accurate refinement beside that knot.
    let t = [a, b, *ts.get(best)?, t].into_iter().min_by(|x, y| dist(*x).total_cmp(&dist(*y)))?;
    // Prefer the domain's ends when they are just as near (a vertex at the seam).
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
