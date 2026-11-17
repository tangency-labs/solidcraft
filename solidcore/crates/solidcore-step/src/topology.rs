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
//! use with none gets its pcurve from the kernel ([`read_solids_with`], SMLib's own trims), except
//! on a face that winds round its axis more than a turn, whose pcurves are its edges' own angle
//! and height, followed round the axis (`crate::assembly`'s `wound`). The file's own PCURVE
//! entities are not used.
//!
//! Input is hostile: every reference is checked, nesting and counts are capped, and the work of
//! a whole file is bounded (shared shells count each time a solid uses them).

use crate::vec::Vec3Geom as _;
use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

pub use crate::assembly::Solid;
use crate::assembly::{
    Assembly, Bound, EdgeB, Kind, MAX_SPLINE_DEGREE, MAX_SPLINE_POINTS, MAX_TOLERANCE, VERTEX_GAP_MARGIN, index, reverse, spline_curve,
    spline_surface, transposed, unsupported, v3,
};
use crate::kernel::{CurveEval, SurfaceEval};
use solidcore_ops::pcurve::Trims;
use solidcore_record::Vec3;
use solidcore_record::brep::{self, Brep, Curve as BCurve, Frame, Natural, Surface as BSurface, Vertex};

use crate::geom::{BSplineCurve, BSplineSurface, Curve, P3, Placement, Transform, dot, scale, sub};
use crate::p21::{Entity, Exchange, Param};
use crate::read::{R, Reader};
use crate::units;

pub(crate) mod swept;

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
/// Samples along a curve when projecting a vertex onto it.
const MAX_SAMPLES: usize = 4096;
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
        match read_with(ex, reader, item, context, &mut budget, trims, &HashSet::new(), false) {
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
    read_with(ex, &mut reader, item, context, &mut budget, None, &HashSet::new(), false)
}

/// [`read_solid`], with `trims` making the pcurves of uses that have no closed form.
pub fn read_solid_with(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims), &HashSet::new(), false)
}

/// [`read_solid_with`] with the faces `left_out` (file face entities: the ADVANCED_FACE or
/// FACE_SURFACE, or the ORIENTED_FACE a shell lists) left out of their shells: what is left of a
/// solid that cannot be read or made whole, its faces still sharing their edges. With any face
/// left out it is a surface model of open shells (`sheet`), none of them bounding a region.
pub fn read_solid_without(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims, left_out: &HashSet<u64>) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims), left_out, false)
}

/// [`read_solid_with`] as a surface model of open shells (`sheet`) with every face: what a solid
/// whose shell the file leaves open (an edge only one face uses) comes to, its faces sharing the
/// edges they do.
pub fn read_solid_as_sheets(ex: &Exchange, item: u64, context: units::Context, trims: &dyn Trims) -> R<Solid> {
    let mut reader = Reader::new(ex, context.units);
    let mut budget = MAX_FILE_FACES;
    read_with(ex, &mut reader, item, context, &mut budget, Some(trims), &HashSet::new(), true)
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

/// A solid's tolerance over its shortest edge between two vertices, at most: SMLib takes an edge
/// shorter than half its tolerance for no edge at all, and two edges closer than it for one.
///
/// Those rules ask only for a factor below 1; a quarter is measured, not derived. In a file of
/// 96 solids, two with a shortest edge of 3.4 um fail the full check at a half (SmFace Test #34,
/// partially coincident edges, twice each) and at 0.9, and are valid at a quarter.
const FEATURE_MARGIN: f64 = 0.25;

/// The tolerance a solid read at `stated` should have been read at instead, when it has
/// features the stated one cannot tell apart: [`FEATURE_MARGIN`] of the shortest distance
/// between the two vertices of an edge. `None` when `stated` already resolves every edge.
///
/// A file's uncertainty is one number for everything in it, and exporters write a round one
/// (Fusion: 0.01 mm) beside geometry far finer (edges of 0.2 to 10 um in the same file). Taken
/// as the solid's tolerance, such edges read as zero-length edges, coincident edges and loops
/// that turn the wrong way, and the kernel refuses every operation on the solid.
fn feature_tolerance(brep: &Brep, stated: f64) -> Option<f64> {
    let mut shortest = f64::INFINITY;
    for e in &brep.edges {
        let [a, b] = e.vertices;
        if a == b {
            continue;
        }
        let (Some(p), Some(q)) = (brep.vertices.get(a as usize), brep.vertices.get(b as usize)) else { continue };
        let d = v3(p.point).dist(v3(q.point));
        if d > 0.0 && d < shortest {
            shortest = d;
        }
    }
    // An edge as long as the stated tolerance or longer is one that tolerance tells apart.
    if shortest >= stated {
        return None;
    }
    let fine = (FEATURE_MARGIN * shortest).max(MIN_TOLERANCE);
    (fine < stated).then_some(fine)
}

fn read_with<'a>(
    ex: &'a Exchange,
    reader: &mut Reader<'a>,
    item: u64,
    context: units::Context,
    budget: &mut usize,
    trims: Option<&dyn Trims>,
    left_out: &HashSet<u64>,
    sheets: bool,
) -> R<Solid> {
    let stated = tolerance(&context);
    let before = *budget;
    let first = read_at(ex, reader, item, stated, budget, trims, left_out, sheets)?;
    let Some(fine) = feature_tolerance(&first.brep, stated) else { return Ok(first) };
    // Read again at the tolerance its own features set; the faces count against the budget once.
    let after = *budget;
    *budget = before;
    match read_at(ex, reader, item, fine, budget, trims, left_out, sheets) {
        Ok(mut solid) => {
            solid.notes.push(format!(
                "the file states a tolerance of {stated} mm, but the part has edges as short as {:.3e} mm: read at {fine:.3e} mm",
                fine / FEATURE_MARGIN
            ));
            Ok(solid)
        }
        // What cannot be read finer stays as the file states it, and says why.
        Err(why) => {
            *budget = after;
            let mut solid = first;
            solid.notes.push(format!(
                "the file states a tolerance of {stated} mm, but the part has edges as short as {:.3e} mm: reading it at {fine:.3e} mm failed ({why}), so it is read as stated",
                fine / FEATURE_MARGIN
            ));
            Ok(solid)
        }
    }
}

fn read_at<'a>(
    ex: &'a Exchange,
    reader: &mut Reader<'a>,
    item: u64,
    tol: f64,
    budget: &mut usize,
    trims: Option<&dyn Trims>,
    left_out: &HashSet<u64>,
    sheets: bool,
) -> R<Solid> {
    let e = entity(ex, item)?;
    let name = e.params().first().and_then(Param::as_str).unwrap_or("").to_string();
    let mut b = Builder::new(ex, reader, tol, budget, trims);
    // The file's faces, edges, vertices and their geometry read into the builder.
    let (ids, kind) = item_shells(ex, item)?;
    let mut shells = Vec::with_capacity(ids.len());
    let mut skipped = false;
    for s in ids {
        let (faces, any) = b.shell(s, left_out)?;
        skipped |= any;
        shells.push(faces);
    }
    b.finish(item, name, shells, if skipped || sheets { Kind::Open { voids: kind == Kind::Voids } } else { kind })
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

/// The STEP side of reading a solid: the file, and which of its entities are already read into
/// the [`Assembly`] that makes the record.
struct Builder<'b, 'a> {
    ex: &'a Exchange,
    reader: &'b mut Reader<'a>,
    budget: &'b mut usize,
    asm: Assembly<'b>,
    vertex_ids: HashMap<u64, u32>,
    /// STEP edge → schema edge, and whether the schema edge runs with the STEP edge.
    edge_ids: HashMap<u64, (u32, bool)>,
    face_ids: HashSet<u64>,
    /// The line edges of POLY_LOOPs, by their two CARTESIAN_POINTs (lower id first): faces that
    /// share a side share its edge, as a faceted B-rep's do.
    poly_edges: HashMap<(u64, u64), u32>,
    /// Vertices joined at one point ([`Builder::meet`]) as a union-find: each vertex's parent,
    /// a vertex past its end its own root. Edges and vertex loops are put through it once, when
    /// the file is read ([`Builder::join_vertices`]); until then a vertex is resolved at use.
    vertex_parent: Vec<u32>,
    /// The VERTEX_POINTs of each joined set, by its root (a vertex not listed: its own one).
    vertex_members: HashMap<u32, Vec<u64>>,
    /// Each vertex's VERTEX_POINT.
    vertex_step: HashMap<u32, u64>,
    /// The other ends of the file's EDGE_CURVEs at each VERTEX_POINT (an edge's own two ends
    /// are never joined), read at the first join.
    edge_partners: Option<HashMap<u64, Vec<u64>>>,
}

impl<'b, 'a> Builder<'b, 'a> {
    fn new(ex: &'a Exchange, reader: &'b mut Reader<'a>, tol: f64, budget: &'b mut usize, trims: Option<&'b dyn Trims>) -> Self {
        Builder {
            ex,
            reader,
            budget,
            asm: Assembly::new(tol, trims),
            vertex_ids: HashMap::new(),
            edge_ids: HashMap::new(),
            face_ids: HashSet::new(),
            poly_edges: HashMap::new(),
            vertex_parent: Vec::new(),
            vertex_members: HashMap::new(),
            vertex_step: HashMap::new(),
            edge_partners: None,
        }
    }

    fn vertex(&mut self, id: u64) -> R<u32> {
        if let Some(&v) = self.vertex_ids.get(&id) {
            return Ok(self.find(v));
        }
        let e = entity(self.ex, id)?;
        if e.name() != "VERTEX_POINT" {
            return Err(format!("#{id}: {}", unsupported(format!("vertex {}", e.name()))));
        }
        let point = self.reader.point(rid(e.params().get(1), "vertex point")?)?;
        let i = index(self.asm.vertices.len())?;
        self.asm.vertices.push(Vertex { point, tolerance: self.asm.tol });
        self.vertex_ids.insert(id, i);
        self.vertex_step.insert(i, id);
        Ok(i)
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
        let (ps, pe) = (self.asm.point(vs)?, self.asm.point(ve)?);
        let closed = vs == ve;
        // A full turn when the edge has one vertex, or two at one point. Two vertices apart bound
        // the arc between them, however short: a 20.4 um arc between vertices a 20.5 um uncertainty
        // does not tell apart is that arc, not a turn around its face.
        let full = closed || ps == pe;
        let mapped = map_edge(&curve, ps, pe, same_sense, closed, full, self.asm.tol).map_err(|m| format!("edge #{id}: {m}"))?;
        if mapped.sense_mismatch {
            self.asm.sense_mismatches += 1;
        }
        let vertices = if mapped.with_step { [vs, ve] } else { [ve, vs] };
        // The gaps as the record will be read: its curve at its interval's ends.
        let eval = CurveEval::new(&mapped.curve).map_err(|m| format!("edge #{id}: {m}"))?;
        let mut gaps = [0.0; 2];
        for ((t, v), g) in mapped.interval.into_iter().zip(vertices).zip(gaps.iter_mut()) {
            let at = eval.point(t).ok_or_else(|| format!("edge #{id}: no point at {t}"))?;
            *g = at.dist(v3(self.asm.point(v)?));
        }
        let (mut curve, mut interval) = (mapped.curve, mapped.interval);
        // Topology first: a straight edge whose vertices lie off its LINE (a direction or a
        // magnitude written wrong) is the segment between its vertices, which the faces it
        // bounds share; any other curve is the edge's shape, and a vertex far off it is refused.
        let far = gaps.iter().any(|g| !(VERTEX_GAP_MARGIN * g <= MAX_TOLERANCE));
        if far && matches!(curve, BCurve::Line { .. }) && !closed && ps != pe {
            let [a, z] = vertices;
            curve = BCurve::Line { start: self.asm.point(a)?, end: self.asm.point(z)? };
            interval = [0.0, 1.0];
            gaps = [0.0; 2];
            self.asm.lines_between_vertices += 1;
        }
        for (v, gap) in vertices.into_iter().zip(gaps) {
            self.asm.raise_vertex(v, gap, id)?;
        }
        let i = index(self.asm.edges.len())?;
        self.asm.edges.push(EdgeB { curve, interval, vertices, tolerance: self.asm.tol, form: None });
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

    /// The root of vertex `v`'s joined set ([`Builder::meet`]), halving the path to it.
    fn find(&mut self, v: u32) -> u32 {
        let mut v = v;
        while let Some(&p) = self.vertex_parent.get(v as usize) {
            if p == v {
                break;
            }
            let up = self.vertex_parent.get(p as usize).copied().unwrap_or(p);
            if let Some(x) = self.vertex_parent.get_mut(v as usize) {
                *x = up;
            }
            v = up;
        }
        v
    }

    /// The VERTEX_POINTs of the joined set whose root is `root`.
    fn member_ids(&self, root: u32) -> Vec<u64> {
        self.vertex_members.get(&root).cloned().unwrap_or_else(|| self.vertex_step.get(&root).copied().into_iter().collect())
    }

    /// [`Builder::member_ids`], taken out of the table.
    fn take_members(&mut self, root: u32) -> Vec<u64> {
        self.vertex_members.remove(&root).unwrap_or_else(|| self.vertex_step.get(&root).copied().into_iter().collect())
    }

    fn members(&self, root: u32) -> usize {
        self.vertex_members.get(&root).map_or(1, Vec::len)
    }

    /// Whether an EDGE_CURVE of the file, read yet or not, runs from a VERTEX_POINT of `small`'s
    /// set to one of `other`'s (both roots).
    fn edge_between(&mut self, small: u32, other: u32) -> bool {
        if self.edge_partners.is_none() {
            let mut partners: HashMap<u64, Vec<u64>> = HashMap::new();
            for e in self.ex.entities.values() {
                if e.name() != "EDGE_CURVE" {
                    continue;
                }
                let p = e.params();
                if let (Some(a), Some(z)) = (p.get(1).and_then(Param::as_ref_id), p.get(2).and_then(Param::as_ref_id))
                    && a != z
                {
                    partners.entry(a).or_default().push(z);
                    partners.entry(z).or_default().push(a);
                }
            }
            self.edge_partners = Some(partners);
        }
        let mut ends = Vec::new();
        for id in self.member_ids(small) {
            for p in self.edge_partners.as_ref().and_then(|m| m.get(&id)).into_iter().flatten() {
                if let Some(&v) = self.vertex_ids.get(p) {
                    ends.push(v);
                }
            }
        }
        ends.into_iter().any(|v| self.find(v) == other)
    }

    /// Whether vertices `a` and `b` are one: the same vertex, or two whose points coincide
    /// within their own tolerances (the kernel's [`solidcore_ops::vertex_meet::points_meet`]; a
    /// file may write one corner as two VERTEX_POINTs at one point). Coincident ones are joined
    /// into one set, its root's tolerance covering both, so the faces and the edges they share
    /// meet at one vertex. Two vertices that are the two ends of one EDGE_CURVE of the file are
    /// never joined, whether that edge is read yet or not: its own ends are apart.
    fn meet(&mut self, a: u32, b: u32) -> R<bool> {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return Ok(true);
        }
        let (keep, drop) = if self.members(a) >= self.members(b) { (a, b) } else { (b, a) };
        let (k, d) =
            (self.asm.vertices.get(keep as usize).ok_or("vertex reference")?, self.asm.vertices.get(drop as usize).ok_or("vertex reference")?);
        let Some(tolerance) = solidcore_ops::vertex_meet::points_meet(k.point, k.tolerance, d.point, d.tolerance) else {
            return Ok(false);
        };
        if self.edge_between(drop, keep) {
            return Ok(false);
        }
        while self.vertex_parent.len() < self.asm.vertices.len() {
            let i = index(self.vertex_parent.len())?;
            self.vertex_parent.push(i);
        }
        *self.vertex_parent.get_mut(drop as usize).ok_or("vertex reference")? = keep;
        let moved = self.take_members(drop);
        let mut kept = self.take_members(keep);
        kept.extend(moved);
        self.vertex_members.insert(keep, kept);
        if let Some(x) = self.asm.vertices.get_mut(keep as usize) {
            x.tolerance = tolerance;
        }
        Ok(true)
    }

    /// Edges and vertex loops on the roots of their vertices' joined sets ([`Builder::meet`]),
    /// once the file is read; each root's tolerance covers every vertex joined into it, as
    /// each one's has grown since.
    fn join_vertices(&mut self) -> R<()> {
        if self.vertex_members.is_empty() {
            return Ok(());
        }
        for i in 0..self.asm.vertices.len() {
            let v = index(i)?;
            let r = self.find(v);
            if r == v {
                continue;
            }
            let (x, y) = (self.asm.vertices.get(r as usize).ok_or("vertex reference")?, self.asm.vertices.get(i).ok_or("vertex reference")?);
            let tolerance = solidcore_ops::vertex_meet::points_meet(x.point, x.tolerance, y.point, y.tolerance).ok_or("joined vertices apart")?;
            if let Some(x) = self.asm.vertices.get_mut(r as usize) {
                x.tolerance = tolerance;
            }
        }
        for i in 0..self.asm.edges.len() {
            let [a, z] = self.asm.edges.get(i).ok_or("edge reference")?.vertices;
            let ends = [self.find(a), self.find(z)];
            if let Some(e) = self.asm.edges.get_mut(i) {
                e.vertices = ends;
            }
        }
        for f in 0..self.asm.faces.len() {
            for l in 0..self.asm.faces.get(f).map_or(0, |x| x.loops.len()) {
                if let Some(&Bound::Vertex(v)) = self.asm.faces.get(f).and_then(|x| x.loops.get(l)) {
                    let r = self.find(v);
                    if let Some(Bound::Vertex(v)) = self.asm.faces.get_mut(f).and_then(|x| x.loops.get_mut(l)) {
                        *v = r;
                    }
                }
            }
        }
        Ok(())
    }

    /// The finished record ([`Assembly::finish`]), its edges and vertex loops first put on the
    /// roots of their vertices' joined sets ([`Builder::join_vertices`]).
    fn finish(mut self, item: u64, name: String, shells: Vec<Vec<(u32, bool)>>, kind: Kind) -> R<Solid> {
        self.join_vertices()?;
        self.asm.finish(item, name, shells, kind)
    }

    /// A use's start and end vertices, each its joined set's root ([`Builder::meet`]).
    fn ends(&mut self, (edge, forward): (u32, bool)) -> R<[u32; 2]> {
        let e = self.asm.edges.get(edge as usize).ok_or("edge reference")?;
        let [a, z] = e.vertices;
        let [a, z] = [self.find(a), self.find(z)];
        Ok(if forward { [a, z] } else { [z, a] })
    }

    /// A face bound: its loop as the face uses it (bound orientation applied), and whether it
    /// is the FACE_OUTER_BOUND.
    fn bound(&mut self, id: u64, edges_so_far: &mut usize) -> R<(Bound, bool)> {
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
                    if let Some(l) = last
                        && !self.meet(l, s)?
                    {
                        return Err(format!("edge loop #{lp_id} is not connected"));
                    }
                    last = Some(e);
                }
                let first_start = match uses.first() {
                    Some(&first) => Some(self.ends(first)?[0]),
                    None => None,
                };
                if let (Some(start), Some(last)) = (first_start, last)
                    && !self.meet(last, start)?
                {
                    return Err(format!("edge loop #{lp_id} is not closed"));
                }
                if !orientation {
                    reverse(&mut uses);
                }
                Ok((Bound::Edges(uses), outer))
            }
            "VERTEX_LOOP" => Ok((Bound::Vertex(self.vertex(rid(lp.params().get(1), "vertex loop")?)?), outer)),
            "POLY_LOOP" => {
                let mut uses = self.poly_loop(lp_id, edges_so_far)?;
                if !orientation {
                    reverse(&mut uses);
                }
                Ok((Bound::Edges(uses), outer))
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
            let (pa, pb) = (self.asm.point(va)?, self.asm.point(vb)?);
            if v3(pa).dist(v3(pb)) <= self.asm.tol {
                return Err(format!("poly loop #{lp_id}: a side of no length"));
            }
            let e = index(self.asm.edges.len())?;
            self.asm.edges.push(EdgeB {
                curve: BCurve::Line { start: pa, end: pb },
                interval: [0.0, 1.0],
                vertices: [va, vb],
                tolerance: self.asm.tol,
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
        let i = index(self.asm.vertices.len())?;
        self.asm.vertices.push(Vertex { point, tolerance: self.asm.tol });
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
        if self.asm.faces.len() >= MAX_FACES {
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
            if !loops.iter().any(|l| matches!(l, Bound::Edges(_))) {
                return Err(format!("face #{id}: {}", unsupported("a face without an edge loop")));
            }
            (std::sync::Arc::new(self.loop_plane(&loops).map_err(|m| format!("face #{id}: {m}"))?), true)
        } else {
            let s = self.reader.surface(rid(p.get(2), "face geometry")?).map_err(|m| format!("face #{id}: {m}"))?;
            (s, flag(p.get(3), "face same_sense")?)
        };
        if !loops.iter().any(|l| matches!(l, Bound::Edges(_))) {
            loops = self.whole_sphere(&step_surface, &loops).map_err(|m| format!("face #{id}: {m}"))?;
        }
        let extent = self.asm.extent(&loops)?;
        let (surface, reversed) = swept::map_surface(&step_surface, &extent, self.asm.tol).map_err(|m| format!("face #{id}: {m}"))?;
        // Loops relative to the record's natural normal: the face's normal is STEP's surface
        // normal when same_sense, and the record's is STEP's unless `reversed`.
        let sense = same_sense != reversed;
        if !sense {
            for l in &mut loops {
                if let Bound::Edges(uses) = l {
                    reverse(uses);
                }
            }
        }
        let i = self.asm.add_face(id, surface, loops, outer, sense)?;
        Ok((i, sense))
    }

    /// A sphere face bounded by vertex loops alone, each at one of its poles (ISO 10303-42: a face
    /// of a closed surface bounded by vertices is all of the surface): the whole sphere, its loop
    /// a meridian from the south pole to the north and back, which [`Assembly::complete_slits`]
    /// keeps as its seam. Any other face without an edge loop is refused.
    fn whole_sphere(&mut self, surface: &crate::geom::Surface, loops: &[Bound]) -> R<Vec<Bound>> {
        let crate::geom::Surface::Sphere { position, radius } = surface else { return Err(unsupported("a face without an edge loop")) };
        // Each pole vertex as its joined set's root ([`Builder::meet`]).
        let loops: Vec<Bound> = loops.iter().map(|l| if let &Bound::Vertex(v) = l { Bound::Vertex(self.find(v)) } else { l.clone() }).collect();
        self.asm.whole_sphere(position.origin, position.axis, position.ref_direction, *radius, &loops)
    }

    /// The plane a plain FACE's loops lie in (ISO 10303-42: a FACE with no face geometry is
    /// bounded by loops in one plane): its normal by Newell's method over the first edge loop's
    /// points (each edge at nine points), so its outer loop runs counter-clockwise about it; its
    /// origin the loop's first point. Refused where the points span no plane or leave it by more
    /// than the tolerance.
    fn loop_plane(&self, loops: &[Bound]) -> R<crate::geom::Surface> {
        let Some(Bound::Edges(es)) = loops.iter().find(|l| matches!(l, Bound::Edges(_))) else { return Err("no edge loop".into()) };
        let mut pts: Vec<Vec3> = Vec::new();
        for &(e, forward) in es {
            let edge = self.asm.edges.get(e as usize).ok_or("edge reference")?;
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
        if pts.iter().any(|p| (*p - first).dot(z).abs() > self.asm.tol) {
            return Err(unsupported("a face with no surface whose loop leaves its plane"));
        }
        let far = pts.iter().copied().max_by(|a, b| (*a - first).len().total_cmp(&(*b - first).len())).ok_or("an empty loop")?;
        let d = far - first;
        let x = (d - z * d.dot(z)).normalized().ok_or("a degenerate loop")?;
        Ok(crate::geom::Surface::Plane {
            position: Placement { origin: [first.x, first.y, first.z], axis: [z.x, z.y, z.z], ref_direction: [x.x, x.y, x.z] },
        })
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
    spline_curve(b.degree, knots, &b.points, b.weights.as_deref())
}

fn map_bspline_surface(b: &BSplineSurface) -> R<BSurface> {
    let nu = b.points.len();
    let nv = b.points.first().map_or(0, Vec::len);
    let u_knots = full_knots(&b.u_knots, &b.u_multiplicities, nu, b.u_degree)?;
    let v_knots = full_knots(&b.v_knots, &b.v_multiplicities, nv, b.v_degree)?;
    spline_surface([b.u_degree, b.v_degree], [u_knots, v_knots], &b.points, b.weights.as_deref())
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
pub(crate) fn closed_within(s: &BSurface, tol: f64) -> Option<BSurface> {
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
    let knots = match &curve {
        BCurve::Spline { knots, .. } | BCurve::RationalSpline { knots, .. } => knots.clone(),
        BCurve::Polyline { points } => (0..points.len()).map(|i| i as f64).collect(),
        _ => Vec::new(),
    };
    if closed {
        if !curve_closed {
            let interval = loop_on_open_curve(&eval, a, b, &knots, ps, tol)?;
            return Ok(Mapped { curve, interval, with_step: true, sense_mismatch: false });
        }
        // The whole curve, from its one vertex round to it again. A vertex at the curve's seam
        // (within the tolerance of its ends) keeps the curve as it is; a vertex elsewhere on it
        // (a loop whose vertex the file puts half way round) starts the curve there, exactly, so
        // the edge's ends are its vertex.
        let t = project(&eval, a, b, &knots, v3(ps)).ok_or("no parameter for the edge's vertex")?;
        let eps = 1e-9 * (b - a);
        // A vertex at a knot (where the file's curve joins its pieces) starts the curve at that
        // knot, not a rounding beside it: a span a rounding long beside a knot run makes a run
        // longer than the degree allows once the two are taken for one. Only a knot whose point
        // is the vertex, within the tolerance: a rounding in parameter can be far in length on a
        // large curve.
        let at_vertex = |k: f64| eval.point(k).is_some_and(|q| q.dist(v3(ps)) <= tol);
        let t = knots
            .iter()
            .copied()
            .filter(|k| (k - t).abs() <= eps && at_vertex(*k))
            .min_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()))
            .unwrap_or(t);
        if t - a <= eps || b - t <= eps || v3(ps).dist(pa).min(v3(ps).dist(pb)) <= tol {
            return Ok(Mapped { curve, interval: [a, b], with_step: true, sense_mismatch: false });
        }
        let curve = crate::kernel::restart_curve(&curve, t)?;
        return Ok(Mapped { curve, interval: [t, t + (b - a)], with_step: true, sense_mismatch: false });
    }
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

/// A closed edge on an open curve that runs on past it: a full turn written as a spline that
/// goes round more than once (a periodic curve unrolled, as a record keeps a turn whose seam
/// moved), so the curve passes the edge's vertex twice. The edge is the loop between those two
/// parameters; a curve that passes the vertex once, or three times, does not say which loop.
fn loop_on_open_curve(eval: &CurveEval, a: f64, b: f64, knots: &[f64], p: P3, tol: f64) -> R<[f64; 2]> {
    let open = || "a closed edge on an open curve".to_string();
    let p = v3(p);
    let near = tol.max(1e-9 * (1.0 + p.len()));
    let at = |t: f64| eval.point(t).map_or(f64::INFINITY, |q| q.dist(p));
    // Every pass: the nearest point of each stretch of the domain not yet near a pass found,
    // a hundredth of the domain kept clear on each side of one.
    let margin = (b - a) * 0.01;
    let mut hits: Vec<f64> = Vec::new();
    for _ in 0..4 {
        let mut bounds = vec![a];
        for h in &hits {
            bounds.extend([h - margin, h + margin]);
        }
        bounds.push(b);
        let found: Vec<f64> = bounds
            .chunks(2)
            .filter_map(|r| match r {
                [lo, hi] if lo < hi => project(eval, *lo, *hi, knots, p).filter(|t| at(*t) <= near),
                _ => None,
            })
            .collect();
        if found.is_empty() {
            break;
        }
        hits.extend(found);
        hits.sort_by(f64::total_cmp);
    }
    // Hits with no point away from the vertex between them are one pass.
    let leaves = |x: f64, y: f64| (1..8).any(|k| at(x + (y - x) * k as f64 / 8.0) > 2.0 * near);
    let mut passes: Vec<f64> = Vec::new();
    for t in hits {
        if passes.last().is_none_or(|&last| leaves(last, t)) {
            passes.push(t);
        }
    }
    match passes.as_slice() {
        [x, y] => Ok([*x, *y]),
        _ => Err(open()),
    }
}

/// The parameter in [a, b] of the curve's point nearest `p`: the nearest of samples along each
/// knot span, refined by golden-section search between its neighbours.
pub(crate) fn project(c: &CurveEval, a: f64, b: f64, knots: &[f64], p: Vec3) -> Option<f64> {
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
