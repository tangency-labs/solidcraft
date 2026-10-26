//! STEP import (ISO 10303-21 files, AP203/AP214/AP242 geometry).
//!
//! Our own reader, written from the public standards: [`p21`] parses the exchange structure,
//! [`solidcraft_step::topology`] reads each solid into its schema 4 record, which SMLib takes
//! as the body ([`parallel`]), and this module walks the product structure (products, shape
//! representations, assemblies with their placements, units, colours). STEP files are
//! untrusted: every count and nesting depth is capped, and what cannot be read becomes a
//! warning rather than a failure wherever the rest of the file is still usable.

pub(crate) mod p21;
mod parallel;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_step::read::R;

use crate::body::Body;
use crate::{KernelError, Result};
use p21::{Entity, Exchange, Param};

/// Deepest assembly nesting followed.
const MAX_ASSEMBLY_DEPTH: usize = 32;
/// Most body instances in one import.
const MAX_BODIES: usize = 10_000;
/// Most product occurrences and mapped representations visited (a shared sub-assembly used
/// twice at every level would otherwise grow exponentially).
const MAX_VISITS: usize = 100_000;
/// Most planned solids converted in one go (bounds the geometry held at once).
const MAX_WINDOW: usize = 64;
/// Most representation entries walked while deciding which items are converted exactly once. A
/// larger file keeps every conversion instead of paying for the walk.
const MAX_CLOSURE_WORK: usize = 1 << 16;

/// A body read from a STEP file.
#[derive(Clone, Debug)]
pub struct ImportedBody {
    /// Immutable STEP source item, scoped to its embedded payload.
    pub source_item: u64,
    /// Original STEP face entity of each kernel face, including seam pieces.
    pub face_origin: Vec<u64>,
    /// Solid name (the brep's label), else the product name.
    pub name: String,
    pub body: Body,
    /// Surface colour (linear-ish RGB 0..1) from the file's presentation styles.
    pub color: Option<[f32; 3]>,
    /// Product names from the root to the part that holds the body (the last is the part's own
    /// name; empty for a solid outside any product).
    pub path: Vec<String>,
    /// The name of the assembly occurrence (instance) that holds the body: the usage's name in
    /// the file, else its id. `None` for a body of a root product or outside any product.
    pub instance: Option<String>,
    /// The body is a closed solid (false: faces were left out, or the file had a surface model).
    pub closed: bool,
    /// Faces read from the file (each body face is one of them: `face_origin`).
    pub file_faces: usize,
}

/// One product occurrence in the file's assembly tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepNode {
    pub name: String,
    /// Placement in the parent (column-major 4×4, millimetres).
    pub transform: [[f64; 4]; 4],
    /// Indices into [`StepImport::bodies`] of this product's own bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<StepNode>,
}

/// Result of [`step_import`].
#[derive(Clone, Debug)]
pub struct StepImport {
    pub bodies: Vec<ImportedBody>,
    /// Root products (an assembly tree; a plain part is a single root).
    pub tree: Vec<StepNode>,
    /// What could not be read (unsupported entities, faces left out…).
    pub warnings: Vec<String>,
    /// FILE_SCHEMA (e.g. `AUTOMOTIVE_DESIGN`, `CONFIG_CONTROL_DESIGN`, `AP242_…`).
    pub schema: String,
    /// FILE_NAME originating system.
    pub system: String,
}

/// A solid the walk found, converted later.
struct Planned {
    item: u64,
    units: Units,
    batch: usize,
}

/// Where the solids of a batch go: their placement and names.
struct Batch {
    m: M4,
    path: Vec<String>,
    product: String,
    instance: Option<String>,
}

/// The units of a representation context (GLOBAL_UNIT_ASSIGNED_CONTEXT) and its distance
/// uncertainty (mm) as the file states it.
type Units = solidcraft_step::units::Context;

/// Units of a representation context, read by [`solidcraft_step::units::context`] (millimetres
/// and radians when the file names none).
fn context_units(ex: &Exchange, ctx: Option<u64>, cache: &mut HashMap<u64, Units>) -> Units {
    let Some(id) = ctx else { return Units::default() };
    *cache.entry(id).or_insert_with(|| solidcraft_step::units::context(ex, id))
}

/// Items and context of a representation entity.
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

/// A placement: a column-major 4×4 matrix (columns 0–2 the images of x, y and z, column 3 the
/// translation), millimetres.
type M4 = [[f64; 4]; 4];

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `a · b`.
fn mat_mul(a: &M4, b: &M4) -> M4 {
    let mut out = [[0.0; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, cell) in out_col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

/// The determinant of a placement's linear part (its first three columns).
fn linear_det(m: &M4) -> f64 {
    let [a, b, c] = [m[0], m[1], m[2]];
    a[0] * (b[1] * c[2] - b[2] * c[1]) - b[0] * (a[1] * c[2] - a[2] * c[1]) + c[0] * (a[1] * b[2] - a[2] * b[1])
}

/// The inverse of a placement: a rotation, scaled (a transformation operator may scale), and a
/// translation. Its linear part is inverted by its adjugate, and the translation taken back
/// through that. Placements are checked non-singular where they are read
/// ([`Reader::placement_matrix`]; a frame's axes are orthonormal).
fn placement_inverse(m: &M4) -> M4 {
    let det = linear_det(m);
    let [a, b, c] = [m[0], m[1], m[2]];
    // The rows of the inverse are the cross products of the columns, over the determinant.
    let cross = |u: [f64; 4], v: [f64; 4]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let rows = [cross(b, c), cross(c, a), cross(a, b)];
    let mut out = IDENTITY;
    for (i, row) in rows.iter().enumerate() {
        for (j, col) in out.iter_mut().take(3).enumerate() {
            col[i] = row[j] / det;
        }
    }
    for i in 0..3 {
        out[3][i] = -(0..3).map(|k| out[k][i] * m[3][k]).sum::<f64>();
    }
    out
}

#[cfg(test)]
fn translation(t: [f64; 3]) -> M4 {
    let mut m = IDENTITY;
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}

/// The matrix of an AXIS2_PLACEMENT: its frame's axes and origin.
fn frame_matrix(p: &solidcraft_step::geom::Placement) -> M4 {
    let (x, y, z, o) = (p.ref_direction, p.y(), p.axis, p.origin);
    [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [o[0], o[1], o[2], 1.0]]
}

struct Reader {
    ex: Arc<Exchange>,
    units: HashMap<u64, Units>,
    warnings: BTreeMap<String, usize>,
    /// Representation → its items and context.
    reps: HashMap<u64, (Vec<u64>, Option<u64>)>,
    /// Plain representation relationships (same product, no placement).
    rep_links: HashMap<u64, Vec<u64>>,
    /// Product definition → its shape representations.
    pd_reps: HashMap<u64, Vec<u64>>,
    /// Product definition → child occurrences (NAUO ids).
    children: HashMap<u64, Vec<u64>>,
    /// NAUO → placement relationship entity.
    placements: HashMap<u64, u64>,
    /// Styled item → colour.
    colors: HashMap<u64, [f32; 3]>,
    /// Opacity of styled items with a transparency (1 = opaque).
    opacity: HashMap<u64, f32>,
    /// Items that gave no part: reported once when the file could not be read, else (the
    /// body could not be made) at every instance, as the message says.
    unreadable: HashMap<u64, Option<String>>,
    /// Items the file converts exactly once (see `index`): keeping their parts would hold a
    /// second copy of the whole import for nothing.
    once: HashSet<u64>,
    /// An item placed more than once: its first instance's display, shared by every instance
    /// (with the placement that first instance had), so the part is meshed once.
    shown: HashMap<u64, (Arc<crate::body::SharedDisplay>, M4)>,
    /// How many planned solids place each item.
    uses: HashMap<u64, usize>,
    /// An item placed more than once: read and oriented where the file defines it, so each
    /// later instance is only placed (orienting meshes the whole part).
    oriented: HashMap<u64, Arc<parallel::Part>>,
    used_items: HashSet<u64>,
    /// Items already instanced for the product being read.
    node_items: HashSet<u64>,
    /// Occurrences and mapped representations visited so far.
    visits: usize,
    /// Items whose meshing was checked.
    unmeshed_checked: HashSet<u64>,
    bodies: Vec<ImportedBody>,
    /// Solids the walk found, in file order, to be converted into bodies afterwards (so the
    /// conversion, the long part, can be done a piece at a time: [`StepReading`]).
    plan: Vec<Planned>,
    /// What the solids of one batch (consecutive solids of one representation) share.
    batches: Vec<Batch>,
    /// Planned solids converted so far.
    converted: usize,
    /// The body each converted planned solid became (`None`: it failed).
    made: Vec<Option<usize>>,
    workers: usize,
    /// Whoever waits for the import (progress, Cancel).
    watch: Option<Arc<dyn crate::watch::Watch>>,
    /// Solid items in the file, the measure of the conversion's progress.
    solid_items: usize,
    /// The watch cancelled the import: the walk stops and the import fails.
    cancelled: bool,
}

impl Reader {
    fn new(ex: Arc<Exchange>, workers: usize) -> Self {
        Self {
            ex,
            units: HashMap::new(),
            warnings: BTreeMap::new(),
            reps: HashMap::new(),
            rep_links: HashMap::new(),
            pd_reps: HashMap::new(),
            children: HashMap::new(),
            placements: HashMap::new(),
            colors: HashMap::new(),
            opacity: HashMap::new(),
            unreadable: HashMap::new(),
            once: HashSet::new(),
            shown: HashMap::new(),
            uses: HashMap::new(),
            oriented: HashMap::new(),
            used_items: HashSet::new(),
            node_items: HashSet::new(),
            visits: 0,
            unmeshed_checked: HashSet::new(),
            bodies: Vec::new(),
            plan: Vec::new(),
            batches: Vec::new(),
            converted: 0,
            made: Vec::new(),
            workers,
            watch: None,
            solid_items: 0,
            cancelled: false,
        }
    }

    /// Report progress; true once the import is cancelled (the walk then unwinds).
    fn stop(&mut self) -> bool {
        if self.cancelled {
            return true;
        }
        if let Some(w) = &self.watch {
            if w.cancelled() {
                self.cancelled = true;
                return true;
            }
            w.progress("converting", self.converted.min(self.solid_items), self.solid_items);
        }
        false
    }

    fn warn(&mut self, msg: impl Into<String>) {
        *self.warnings.entry(msg.into()).or_default() += 1;
    }

    fn index(&mut self) {
        let ex = self.ex.clone();
        let ex = &*ex;
        let mut pds: HashMap<u64, u64> = HashMap::new(); // PDS → definition
        let mut sdrs: Vec<(u64, u64)> = Vec::new(); // (PDS, rep)
        let mut cdsrs: Vec<(u64, u64)> = Vec::new(); // (relationship, PDS)
        let mut nauos: Vec<(u64, u64, u64)> = Vec::new(); // (id, parent, child)
        // Item → how many shape representations list it, and the one representation the item
        // comes from, to tell items the file converts exactly once (see `once`).
        let mut item_reps: HashMap<u64, usize> = HashMap::new();
        let mut item_rep: HashMap<u64, u64> = HashMap::new();
        // True as soon as the file can walk a product definition twice: every occurrence of a
        // component makes the reader convert the component's items again.
        let mut repeated = false;
        let mut ids: Vec<&u64> = ex.entities.keys().collect();
        ids.sort();
        for &id in ids {
            let Some(e) = ex.get(id) else { continue };
            if let Some(r) = representation(e) {
                for item in &r.0 {
                    *item_reps.entry(*item).or_default() += 1;
                    item_rep.entry(*item).or_insert(id);
                }
                self.reps.insert(id, r);
            }
            let p = e.params();
            let rf = |i: usize| p.get(i).and_then(Param::as_ref_id);
            match e.name() {
                "PRODUCT_DEFINITION_SHAPE" | "PROPERTY_DEFINITION" => {
                    if let Some(d) = rf(2) {
                        pds.insert(id, d);
                    }
                }
                "SHAPE_DEFINITION_REPRESENTATION" => {
                    if let (Some(d), Some(r)) = (rf(0), rf(1)) {
                        sdrs.push((d, r));
                    }
                }
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" => {
                    repeated = true;
                    if let (Some(r), Some(d)) = (rf(0), rf(1)) {
                        cdsrs.push((r, d));
                    }
                }
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE" | "ASSEMBLY_COMPONENT_USAGE" | "SPECIFIED_HIGHER_USAGE_OCCURRENCE" => {
                    repeated = true;
                    if let (Some(a), Some(b)) = (rf(3), rf(4)) {
                        nauos.push((id, a, b));
                    }
                }
                "SHAPE_REPRESENTATION_RELATIONSHIP" | "REPRESENTATION_RELATIONSHIP" if !e.is_complex() => {
                    if let (Some(a), Some(b)) = (rf(2), rf(3)) {
                        self.rep_links.entry(a).or_default().push(b);
                        self.rep_links.entry(b).or_default().push(a);
                    }
                }
                "STYLED_ITEM" | "OVER_RIDING_STYLED_ITEM" => {
                    if let Some(item) = rf(2)
                        && let Some(c) = self.find_colour(p.get(1))
                    {
                        self.colors.entry(item).or_insert(c);
                        if let Some(t) = self.find_transparency(p.get(1)) {
                            self.opacity.entry(item).or_insert((1.0 - t).clamp(0.0, 1.0));
                        }
                    }
                }
                _ => {}
            }
        }
        for (d, r) in sdrs {
            if let Some(&pd) = pds.get(&d) {
                self.pd_reps.entry(pd).or_default().push(r);
            }
        }
        for (r, d) in cdsrs {
            if let Some(&nauo) = pds.get(&d) {
                self.placements.insert(nauo, r);
            }
        }
        for (id, parent, child) in nauos {
            if parent != child {
                self.children.entry(parent).or_default().push(id);
            }
        }
        // A file without component occurrences walks each product definition once, so a
        // representation reached from exactly one of them is walked once too. The walk follows
        // MAPPED_ITEM targets as the reader does: a mapping in one product can walk a
        // representation a second time from another product's node. An item that only one such
        // walk lists is converted exactly once and needs no cache entry; anything else keeps
        // every conversion, as before. A file too large to walk cheaply keeps every conversion
        // as well.
        if !repeated {
            let mut entries: HashMap<u64, usize> = HashMap::new();
            let mut seen: HashSet<u64> = HashSet::new();
            let mut queue: Vec<u64> = Vec::new();
            let mut work = 0usize;
            let mut within_budget = true;
            'pds: for reps in self.pd_reps.values() {
                seen.clear();
                queue.clear();
                queue.extend(self.rep_closure(reps));
                while let Some(r) = queue.pop() {
                    if !seen.insert(r) {
                        continue;
                    }
                    work += 1;
                    if work > MAX_CLOSURE_WORK {
                        within_budget = false;
                        break 'pds;
                    }
                    *entries.entry(r).or_default() += 1;
                    for item in self.reps.get(&r).map(|x| &x.0).into_iter().flatten() {
                        queue.extend(self.mapped_targets(*item));
                    }
                }
            }
            if within_budget {
                let mut once = HashSet::new();
                for (&item, &n) in &item_reps {
                    if n != 1 {
                        continue;
                    }
                    let Some(rep) = item_rep.get(&item) else { continue };
                    if entries.get(rep).copied() == Some(1) {
                        once.insert(item);
                    }
                }
                self.once = once;
            }
        }
    }

    /// First colour reachable from a style list (bounded search).
    /// SURFACE_STYLE_TRANSPARENT under a style list (0 = opaque, 1 = invisible).
    fn find_transparency(&self, styles: Option<&Param>) -> Option<f32> {
        let mut queue: Vec<u64> = styles?.as_list()?.iter().filter_map(Param::as_ref_id).collect();
        let mut seen = HashSet::new();
        while let Some(id) = queue.pop() {
            if seen.len() > 64 || !seen.insert(id) {
                continue;
            }
            let e = self.ex.get(id)?;
            if e.name() == "SURFACE_STYLE_TRANSPARENT" {
                return e.params().first().and_then(Param::as_f64).filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 1.0) as f32);
            }
            if e.name() == "COLOUR_RGB" {
                continue;
            }
            for r in &e.records {
                for p in &r.params {
                    match p {
                        Param::Ref(x) => queue.push(*x),
                        Param::List(v) => queue.extend(v.iter().filter_map(Param::as_ref_id)),
                        _ => {}
                    }
                }
            }
        }
        None
    }

    fn find_colour(&self, styles: Option<&Param>) -> Option<[f32; 3]> {
        let mut queue: Vec<u64> = styles?.as_list()?.iter().filter_map(Param::as_ref_id).collect();
        let mut seen = HashSet::new();
        while let Some(id) = queue.pop() {
            if seen.len() > 64 || !seen.insert(id) {
                continue;
            }
            let e = self.ex.get(id)?;
            if e.name() == "COLOUR_RGB" {
                let p = e.params();
                let c = |i: usize| p.get(i).and_then(Param::as_f64).map(|x| x.clamp(0.0, 1.0) as f32);
                return Some([c(1)?, c(2)?, c(3)?]);
            }
            if e.name() == "DRAUGHTING_PRE_DEFINED_COLOUR" {
                return Some(match e.params().first().and_then(Param::as_str).unwrap_or("") {
                    "red" => [1.0, 0.0, 0.0],
                    "green" => [0.0, 1.0, 0.0],
                    "blue" => [0.0, 0.0, 1.0],
                    "yellow" => [1.0, 1.0, 0.0],
                    "magenta" => [1.0, 0.0, 1.0],
                    "cyan" => [0.0, 1.0, 1.0],
                    "black" => [0.0, 0.0, 0.0],
                    _ => [1.0, 1.0, 1.0],
                });
            }
            fn walk(p: &Param, out: &mut Vec<u64>, depth: usize) {
                match p {
                    Param::Ref(r) => out.push(*r),
                    Param::List(v) | Param::Typed(_, v) if depth < 4 => v.iter().for_each(|x| walk(x, out, depth + 1)),
                    _ => {}
                }
            }
            for r in &e.records {
                for p in &r.params {
                    walk(p, &mut queue, 0);
                }
            }
        }
        None
    }

    fn units_of(&mut self, rep: u64) -> Units {
        let ctx = self.reps.get(&rep).and_then(|r| r.1);
        context_units(&self.ex, ctx, &mut self.units)
    }

    /// The placement an item states: an AXIS2_PLACEMENT's frame, or a
    /// CARTESIAN_TRANSFORMATION_OPERATOR_3D, uniform or not: its axes, scales and origin (an
    /// omitted axis is the global one, an omitted scale 1, an omitted second or third scale the
    /// first).
    fn placement_matrix(&mut self, item: u64, u: Units) -> R<M4> {
        let reader = solidcraft_step::read::Reader::new(&self.ex, u.units);
        let e = self.ex.get(item).ok_or_else(|| format!("missing entity #{item}"))?;
        if matches!(e.name(), "CARTESIAN_TRANSFORMATION_OPERATOR_3D" | "CARTESIAN_TRANSFORMATION_OPERATOR_3D_NON_UNIFORM") {
            let p = e.params();
            let o = reader.point(p.get(4).and_then(Param::as_ref_id).ok_or("transformation without origin")?)?;
            let direction = |i: usize, global: [f64; 3]| -> R<[f64; 3]> {
                match p.get(i) {
                    None | Some(Param::Null) => Ok(global),
                    Some(Param::Ref(id)) => reader.direction(*id),
                    _ => Err("invalid transformation direction".into()),
                }
            };
            let scale = |i: usize, default: f64| -> R<f64> {
                match p.get(i) {
                    None | Some(Param::Null) => Ok(default),
                    Some(v) => v.as_f64().filter(|x| x.is_finite() && *x != 0.0).ok_or_else(|| "invalid transformation scale".into()),
                }
            };
            let sx = scale(5, 1.0)?;
            let (sy, sz) = (scale(7, sx)?, scale(8, sx)?);
            let (x, y, z) = (direction(2, [1.0, 0.0, 0.0])?, direction(3, [0.0, 1.0, 0.0])?, direction(6, [0.0, 0.0, 1.0])?);
            let col = |d: [f64; 3], s: f64| [d[0] * s, d[1] * s, d[2] * s, 0.0];
            let m = [col(x, sx), col(y, sy), col(z, sz), [o[0], o[1], o[2], 1.0]];
            if !m.iter().flatten().all(|x| x.is_finite()) {
                return Err("invalid transformation scale".into());
            }
            let det = linear_det(&m);
            if !det.is_finite() || det == 0.0 {
                return Err("singular transformation".into());
            }
            return Ok(m);
        }
        Ok(frame_matrix(&reader.frame(item)?))
    }

    /// Representations of a product (its own shape representations and those linked to them).
    fn rep_closure(&self, roots: &[u64]) -> Vec<u64> {
        let mut seen: Vec<u64> = Vec::new();
        let mut stack: Vec<u64> = roots.to_vec();
        while let Some(r) = stack.pop() {
            if seen.contains(&r) || seen.len() > 1024 {
                continue;
            }
            seen.push(r);
            if let Some(l) = self.rep_links.get(&r) {
                stack.extend(l.iter().copied());
            }
        }
        seen
    }

    /// Representations a MAPPED_ITEM walks (empty for any other item).
    fn mapped_targets(&self, item: u64) -> Vec<u64> {
        let Some(e) = self.ex.get(item).filter(|e| e.name() == "MAPPED_ITEM") else { return Vec::new() };
        let mapped = e
            .params()
            .get(1)
            .and_then(Param::as_ref_id)
            .and_then(|map| self.ex.get(map))
            .and_then(|map| map.params().get(1))
            .and_then(Param::as_ref_id);
        mapped.map(|rep| self.rep_closure(&[rep])).unwrap_or_default()
    }

    fn label(&self, item: u64) -> String {
        let name = self.ex.get(item).and_then(|e| e.params().first()).and_then(Param::as_str).unwrap_or("").trim().to_string();
        if name.is_empty() { format!("solid #{item}") } else { format!("solid '{name}'") }
    }

    /// Bodies of a representation's items (following mapped items), placed by `m`.
    fn rep_bodies(&mut self, rep: u64, m: &M4, path: &[String], product: &str, instance: Option<&str>, out: &mut Vec<usize>, depth: usize) {
        if depth > MAX_ASSEMBLY_DEPTH {
            self.warn("mapped items nested too deeply");
            return;
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.warn(format!("more than {MAX_VISITS} component instances: the rest are left out"));
            return;
        }
        let u = self.units_of(rep);
        let mut items = self.reps.get(&rep).map(|r| r.0.clone()).unwrap_or_default().into_iter().peekable();
        while let Some(item) = items.next() {
            if self.stop() {
                return;
            }
            let Some(e) = self.ex.get(item) else { continue };
            match e.name() {
                "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP" | "SHELL_BASED_SURFACE_MODEL" => {
                    // Only batch consecutive solids: mapped items must retain traversal and
                    // first-use unit/cache order. Bound temporary geometry independently of file size.
                    let mut batch = vec![item];
                    let capacity = MAX_BODIES.saturating_sub(self.bodies_so_far()).min(64);
                    while batch.len() < capacity
                        && items.peek().is_some_and(|id| {
                            self.ex.get(*id).is_some_and(|e| {
                                matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP" | "SHELL_BASED_SURFACE_MODEL")
                            })
                        })
                    {
                        if let Some(next) = items.next() {
                            batch.push(next);
                        }
                    }
                    let id = self.batches.len();
                    self.batches.push(Batch { m: *m, path: path.to_vec(), product: product.to_string(), instance: instance.map(str::to_string) });
                    for item in batch {
                        self.used_items.insert(item);
                        if !self.node_items.insert(item) {
                            continue;
                        }
                        if self.bodies_so_far() >= MAX_BODIES {
                            self.warn(format!("more than {MAX_BODIES} bodies: the rest are left out"));
                            return;
                        }
                        // The node lists the planned solid; it becomes a body index once converted.
                        out.push(self.plan.len());
                        self.plan.push(Planned { item, units: u, batch: id });
                        *self.uses.entry(item).or_default() += 1;
                    }
                }
                "MAPPED_ITEM" => {
                    let p = e.params();
                    let (Some(map), Some(target)) = (p.get(1).and_then(Param::as_ref_id), p.get(2).and_then(Param::as_ref_id)) else { continue };
                    let Some(me) = self.ex.get(map) else { continue };
                    let (Some(origin), Some(mapped)) =
                        (me.params().first().and_then(Param::as_ref_id), me.params().get(1).and_then(Param::as_ref_id))
                    else {
                        continue;
                    };
                    let mu = self.units_of(mapped);
                    match (self.placement_matrix(target, u), self.placement_matrix(origin, mu)) {
                        (Ok(t), Ok(o)) => {
                            let mm = mat_mul(m, &mat_mul(&t, &placement_inverse(&o)));
                            for r in self.rep_closure(&[mapped]) {
                                self.rep_bodies(r, &mm, path, product, instance, out, depth + 1);
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => self.warn(format!("mapped item #{item}: {e}")),
                    }
                }
                _ => {}
            }
        }
    }

    /// Bodies made, and planned ones still to convert (the most a failure can leave out).
    fn bodies_so_far(&self) -> usize {
        self.bodies.len() + self.plan.len() - self.converted
    }

    /// Convert the next batch of planned solids into bodies; false once every one is done.
    /// `single`: one solid at a time (the caller wants short steps and has no worker threads).
    fn convert_next(&mut self, single: bool) -> bool {
        if self.converted >= self.plan.len() {
            return false;
        }
        if self.stop() {
            return false;
        }
        let first = self.converted;
        // The next solids, across instances and parts, converted together: parts still to
        // orient are shared out between the workers. A part that comes back within the window
        // waits for the next one, which places the orientation its first instance found.
        let mut last = first + 1;
        if !single {
            let mut fresh: HashSet<u64> = self.plan.get(first).map(|p| p.item).into_iter().collect();
            while let Some(p) = self.plan.get(last)
                && last - first < MAX_WINDOW
            {
                if !self.oriented.contains_key(&p.item) && !fresh.insert(p.item) {
                    break;
                }
                last += 1;
            }
        }
        self.converted = last;
        let ex = self.ex.clone();
        let mut sources = Vec::new();
        let mut jobs = Vec::new();
        for i in first..last {
            self.made.push(None);
            let Some(p) = self.plan.get(i) else { continue };
            let (item, units) = (p.item, p.units);
            if let Some(again) = self.unreadable.get(&item) {
                if let Some(msg) = again.clone() {
                    let label = self.label(item);
                    self.warn(format!("{label}: not imported: {msg}"));
                }
                continue;
            }
            let source = match self.oriented.get(&item) {
                Some(part) => parallel::Source::Oriented(part.clone()),
                None => parallel::Source::Item { item, units },
            };
            let placement = self.batches.get(p.batch).map_or(IDENTITY, |b| b.m);
            sources.push((i, item));
            jobs.push(parallel::Job { source, placement, check_mesh: !self.unmeshed_checked.contains(&item) });
        }
        // Reading and orienting new parts is shared out between the workers.
        let prepared = parallel::bodies(&ex, &jobs, self.workers);
        for ((i, item), made) in sources.into_iter().zip(prepared) {
            let Some(e) = ex.get(item) else { continue };
            let Some(batch) = self.plan.get(i).and_then(|p| self.batches.get(p.batch)) else { continue };
            let (m, path, product, instance) = (batch.m, batch.path.clone(), batch.product.clone(), batch.instance.clone());
            match made {
                Ok(parallel::Made { body, missing, part, read }) => {
                    // What the read of the item found, reported once.
                    if let Some(failed) = read {
                        let label = self.label(item);
                        for f in &failed {
                            self.warn(format!("{label}: {f}"));
                        }
                        if !part.closed {
                            self.warn(format!("{label}: imported as an open body (the shell is not closed)"));
                        }
                    }
                    if !self.once.contains(&item) {
                        self.oriented.entry(item).or_insert_with(|| part.clone());
                    }
                    let (closed, file_faces, face_origin) = (part.closed, part.file_faces, part.face_origin.clone());
                    let own = e.params().first().and_then(Param::as_str).unwrap_or("").trim().to_string();
                    let name = if own.is_empty() { product.clone() } else { own };
                    // Faces the mesher gives up on (or that the budget cut short) are reported.
                    if self.unmeshed_checked.insert(item) && missing > 0 {
                        self.warn(format!("solid '{name}': {missing} face(s) could not be meshed and are not shown"));
                    }
                    if let Some(slot) = self.made.get_mut(i) {
                        *slot = Some(self.bodies.len());
                    }
                    let color = self.colors.get(&item).copied();
                    // See-through bodies and faces with colours of their own.
                    let faces = face_origin
                        .iter()
                        .enumerate()
                        .filter_map(|(i, f)| {
                            let c = self.colors.get(f)?;
                            Some(crate::FacePaint { face: i, color: *c, opacity: self.opacity.get(f).copied().unwrap_or(1.0) })
                        })
                        .collect();
                    let paint = crate::Paint { opacity: self.opacity.get(&item).copied().unwrap_or(1.0), faces };
                    let body = body.with_color(color).with_paint(Some(paint));
                    let identities = face_origin
                        .iter()
                        .map(|face| Some(crate::FaceSource::Imported { item, face: *face, payload_sha256: None, producer: None }))
                        .collect();
                    let body = body.with_face_sources(Some(std::sync::Arc::new(identities)));
                    // Every instance of a part placed more than once shows its first instance's
                    // mesh, placed (the first one too: meshing its own would mesh the part twice).
                    let body = match self.shown.get(&item) {
                        Some((shared, first)) => body.with_display_of(shared.clone(), mat_mul(&m, &placement_inverse(first))),
                        None if self.uses.get(&item).is_some_and(|n| *n > 1) => {
                            let shared = Arc::new(crate::body::SharedDisplay::new(body.clone()));
                            self.shown.insert(item, (shared.clone(), m));
                            body.with_display_of(shared, IDENTITY)
                        }
                        None => body,
                    };
                    self.bodies.push(ImportedBody {
                        name,
                        body,
                        color,
                        path: path.clone(),
                        instance: instance.clone(),
                        closed,
                        file_faces,
                        source_item: item,
                        face_origin,
                    });
                }
                Err(parallel::Failure { read, msg }) => {
                    let label = self.label(item);
                    self.warn(format!("{label}: not imported: {msg}"));
                    // Later instances don't read the item again.
                    if !self.oriented.contains_key(&item) {
                        self.unreadable.insert(item, (!read).then_some(msg));
                    }
                }
            }
        }
        true
    }

    /// Planned-solid indices (a node's `bodies` after the walk) as body indices, leaving out
    /// the ones that failed.
    fn remap(&self, planned: &[usize]) -> Vec<usize> {
        planned.iter().filter_map(|i| self.made.get(*i).copied().flatten()).collect()
    }

    fn remap_node(&self, n: &mut StepNode) {
        n.bodies = self.remap(&n.bodies);
        for c in &mut n.children {
            self.remap_node(c);
        }
    }

    /// The name of an assembly occurrence: the usage's name, else its id.
    fn occurrence_name(&self, nauo: u64) -> Option<String> {
        let p = self.ex.get(nauo)?.params();
        let name = p.get(1).and_then(Param::as_str).filter(|s| !s.trim().is_empty()).or_else(|| p.first().and_then(Param::as_str))?;
        Some(name.trim().to_string()).filter(|n| !n.is_empty())
    }

    fn product_name(&self, pd: u64) -> String {
        let name = (|| {
            let pdf = self.ex.get(self.ex.get(pd)?.params().get(2)?.as_ref_id()?)?;
            let prod = self.ex.get(pdf.params().get(2)?.as_ref_id()?)?;
            let p = prod.params();
            let n = p.get(1).and_then(Param::as_str).filter(|s| !s.trim().is_empty()).or_else(|| p.first().and_then(Param::as_str))?;
            Some(n.trim().to_string())
        })();
        name.filter(|n| !n.is_empty()).unwrap_or_else(|| "Import".into())
    }

    /// Placement of a child occurrence in its parent.
    fn occurrence_matrix(&mut self, nauo: u64, child_reps: &[u64]) -> M4 {
        let Some(&rel) = self.placements.get(&nauo) else { return IDENTITY };
        let ex = self.ex.clone();
        let r = (|| -> R<M4> {
            let e = ex.get(rel).ok_or("missing placement")?;
            let rr = e.record("REPRESENTATION_RELATIONSHIP").ok_or("placement without representations")?;
            let (r1, r2) =
                (rr.get(2).and_then(Param::as_ref_id).ok_or("bad placement")?, rr.get(3).and_then(Param::as_ref_id).ok_or("bad placement")?);
            let op = e
                .record("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
                .and_then(|x| x.first())
                .and_then(Param::as_ref_id)
                .ok_or("placement without transformation")?;
            let ope = ex.get(op).ok_or("missing transformation")?;
            let (u1, u2) = (self.units_of(r1), self.units_of(r2));
            if ope.name() == "ITEM_DEFINED_TRANSFORMATION" {
                let p = ope.params();
                let (i1, i2) = (
                    p.get(2).and_then(Param::as_ref_id).ok_or("bad transformation")?,
                    p.get(3).and_then(Param::as_ref_id).ok_or("bad transformation")?,
                );
                let m1 = self.placement_matrix(i1, u1)?;
                let m2 = self.placement_matrix(i2, u2)?;
                // rep_1 is normally the component: map its item onto the assembly's item.
                let child_is_1 = child_reps.contains(&r1) || !child_reps.contains(&r2);
                Ok(if child_is_1 { mat_mul(&m2, &placement_inverse(&m1)) } else { mat_mul(&m1, &placement_inverse(&m2)) })
            } else {
                self.placement_matrix(op, u2)
            }
        })();
        match r {
            Ok(m) => m,
            Err(e) => {
                self.warn(format!("component placement #{rel}: {e} (placed at the origin)"));
                IDENTITY
            }
        }
    }

    fn node(&mut self, pd: u64, local: M4, world: M4, path: &[String], instance: Option<&str>, depth: usize, stack: &mut Vec<u64>) -> StepNode {
        let name = self.product_name(pd);
        let mut node = StepNode { name: name.clone(), transform: local, bodies: Vec::new(), children: Vec::new() };
        if self.stop() {
            return node;
        }
        if depth > MAX_ASSEMBLY_DEPTH || stack.contains(&pd) {
            self.warn("assembly nested too deeply or recursive: some components were left out");
            return node;
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.warn(format!("more than {MAX_VISITS} component instances: the rest are left out"));
            return node;
        }
        stack.push(pd);
        let mut path = path.to_vec();
        path.push(name.clone());
        let own = self.pd_reps.get(&pd).cloned().unwrap_or_default();
        let reps = self.rep_closure(&own);
        let mut bodies = Vec::new();
        self.node_items.clear();
        for r in &reps {
            self.rep_bodies(*r, &world, &path, &name, instance, &mut bodies, 0);
        }
        node.bodies = bodies;
        for nauo in self.children.get(&pd).cloned().unwrap_or_default() {
            let Some(child) = self.ex.get(nauo).and_then(|e| e.params().get(4)).and_then(Param::as_ref_id) else { continue };
            let child_own = self.pd_reps.get(&child).cloned().unwrap_or_default();
            let child_reps = self.rep_closure(&child_own);
            let m = self.occurrence_matrix(nauo, &child_reps);
            let w = mat_mul(&world, &m);
            let instance = self.occurrence_name(nauo);
            let c = self.node(child, m, w, &path, instance.as_deref(), depth + 1, stack);
            node.children.push(c);
        }
        stack.pop();
        node
    }
}

/// Faces whose boundary runs the wrong way round their normal, as a strict reader sees them:
/// each face's normal is its surface's own parametric normal (∂S/∂u × ∂S/∂v, ISO 10303-42),
/// reversed when `same_sense` is false, and the outer loop must run counter-clockwise seen from
/// that side. These are the faces the STEP reader turns over ([`solidcraft_step::topology`]),
/// numbered as the solid's body numbers them. Returns one message per bad face; an error when
/// the file does not parse or has no solid the reader can read.
pub fn step_orientation_errors(text: &str) -> std::result::Result<Vec<String>, String> {
    let ex = p21::parse(text)?;
    let read = solidcraft_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibTrims);
    if read.solids.is_empty() {
        return Err(read.failures.first().map_or_else(|| "the file has no solids".into(), |f| format!("solid #{}: {}", f.item, f.message)));
    }
    let mut out = Vec::new();
    for solid in &read.solids {
        let name = if solid.name.trim().is_empty() { format!("solid #{}", solid.item) } else { solid.name.trim().to_string() };
        out.extend(solid.turned.iter().map(|f| format!("{name}: face {f} runs clockwise around its normal")));
    }
    Ok(out)
}

/// Check a STEP file's structure: it parses, no entity id is defined twice, every reference
/// names a defined entity, nothing but root entities (relationships, definitions,
/// presentation) is left unreferenced, and ids are dense (`#1`…`#n`). Returns the entity count.
pub fn step_validate(text: &str) -> std::result::Result<usize, String> {
    let ex = p21::parse(text)?;
    if let Some(d) = ex.duplicates.first() {
        return Err(format!("{} duplicate entity ids (first #{d})", ex.duplicates.len()));
    }
    fn refs(p: &Param, out: &mut Vec<u64>) {
        match p {
            Param::Ref(r) => out.push(*r),
            Param::List(v) | Param::Typed(_, v) => v.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }
    let mut ids: Vec<&u64> = ex.entities.keys().collect();
    ids.sort();
    for id in &ids {
        let mut out = Vec::new();
        if let Some(e) = ex.get(**id) {
            e.records.iter().flat_map(|r| &r.params).for_each(|p| refs(p, &mut out));
        }
        if let Some(r) = out.iter().find(|r| ex.get(**r).is_none()) {
            return Err(format!("#{id} refers to undefined #{r}"));
        }
    }
    // Unreferenced entities must be roots of the exchange (relationships, definitions,
    // presentation); anything else is dead data.
    let mut used = std::collections::HashSet::new();
    for e in ex.entities.values() {
        let mut out = Vec::new();
        e.records.iter().flat_map(|r| &r.params).for_each(|p| refs(p, &mut out));
        used.extend(out);
    }
    const ROOTS: &[&str] = &[
        "SHAPE_DEFINITION_REPRESENTATION",
        "APPLICATION_PROTOCOL_DEFINITION",
        "PRODUCT_RELATED_PRODUCT_CATEGORY",
        "PRODUCT_CATEGORY_RELATIONSHIP",
        "SHAPE_REPRESENTATION_RELATIONSHIP",
        "REPRESENTATION_RELATIONSHIP",
        "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
        "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION",
        "DRAUGHTING_MODEL",
        "PROPERTY_DEFINITION_REPRESENTATION",
        "PRESENTATION_LAYER_ASSIGNMENT",
    ];
    for id in &ids {
        if used.contains(*id) {
            continue;
        }
        let Some(e) = ex.get(**id) else { continue };
        if !e.records.iter().any(|r| ROOTS.contains(&r.name.as_str()) || r.name.starts_with("APPLIED_")) {
            return Err(format!("#{id} ({}) is not referenced by anything", e.name()));
        }
    }
    let n = ex.entities.len();
    if ids.last().is_some_and(|m| **m != n as u64) || ids.first().is_some_and(|m| **m != 1) {
        return Err(format!("entity ids are not dense: {n} entities, #{}…#{}", ids.first().map_or(0, |x| **x), ids.last().map_or(0, |x| **x)));
    }
    Ok(n)
}

/// A recent import: held weakly, so it lives only as long as someone uses it (the command that
/// read the file, until the timeline has evaluated it), and strongly only when it is small.
struct Recent {
    key: (u64, usize),
    import: std::sync::Weak<StepImport>,
    /// Keeps a small import alive (never read).
    _small: Option<Arc<StepImport>>,
}

/// Recent imports, so a file read for a command and then evaluated in the timeline is only
/// read once.
static RECENT: std::sync::Mutex<Vec<Recent>> = std::sync::Mutex::new(Vec::new());

/// Files up to this size stay cached after their last user is gone (a few cheap re-reads
/// saved); a larger import is freed with the design that holds it.
const KEEP_SMALL_BYTES: usize = 1 << 20;

/// [`step_import`] through a small cache of recent results (keyed by the text). Keep the result
/// while the same text is still to be read elsewhere (in the timeline): that read reuses it.
pub fn step_import_shared(text: &str) -> Result<std::sync::Arc<StepImport>> {
    let key = text_key(text);
    if let Ok(cache) = RECENT.lock()
        && let Some(imp) = cache.iter().filter(|r| r.key == key).find_map(|r| r.import.upgrade())
    {
        return Ok(imp);
    }
    let imp = std::sync::Arc::new(step_import(text)?);
    remember(key, &imp, text.len() <= KEEP_SMALL_BYTES);
    Ok(imp)
}

/// The cache key of a STEP text: its hash and length.
fn text_key(text: &str) -> (u64, usize) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    (h.finish(), text.len())
}

/// Keep `imp` in the recent imports (strongly only when `small`).
fn remember(key: (u64, usize), imp: &Arc<StepImport>, small: bool) {
    if let Ok(mut cache) = RECENT.lock() {
        cache.retain(|r| r.key != key && r.import.strong_count() > 0);
        cache.push(Recent { key, import: Arc::downgrade(imp), _small: small.then(|| imp.clone()) });
        while cache.len() > 4 {
            cache.remove(0);
        }
    }
}

/// Imports the cache still holds alive, and how many STEP bytes they came from (tests and
/// diagnostics: a closed design must not leave its import behind).
pub fn step_cache_alive() -> (usize, usize) {
    RECENT.lock().map(|c| c.iter().filter(|r| r.import.strong_count() > 0).fold((0, 0), |(n, b), r| (n + 1, b + r.key.1))).unwrap_or((0, 0))
}

/// Read a STEP file: its solids (as bodies, placed by the assembly structure) and the tree.
pub fn step_import(text: &str) -> Result<StepImport> {
    step_import_with_workers(text, parallel::workers())
}

fn step_import_with_workers(text: &str, workers: usize) -> Result<StepImport> {
    let mut reading = StepReading::with_workers(text, workers)?;
    while reading.step() {}
    reading.finish()
}

/// A STEP file being read a piece at a time, for a caller that can't wait for the whole read
/// in one go (the web app, which has one thread and must keep drawing): [`StepReading::new`]
/// parses the file and walks its structure, each [`StepReading::step`] converts the next few
/// solids into bodies, and [`StepReading::finish_shared`] gives the import, cached as
/// [`step_import_shared`] would, so a command reading the same text next finds it read.
pub struct StepReading {
    rd: Reader,
    tree: Vec<StepNode>,
    key: (u64, usize),
    small: bool,
    /// One solid per step (no worker threads to share a batch between).
    single: bool,
}

impl StepReading {
    /// Parse `text` and walk it; the solids are converted by [`Self::step`].
    pub fn new(text: &str) -> Result<StepReading> {
        Self::with_workers(text, parallel::workers())
    }

    fn with_workers(text: &str, workers: usize) -> Result<StepReading> {
        let key = text_key(text);
        let watch = crate::watch::current();
        let cancelled = || watch.as_ref().is_some_and(|w| w.cancelled());
        let ex = match p21::parse(text) {
            Ok(ex) => Arc::new(ex),
            Err(_) if cancelled() => return Err(crate::watch::cancelled_error("STEP import")),
            Err(e) => return Err(KernelError::Invalid(e)),
        };
        let mut rd = Reader::new(ex.clone(), workers);
        rd.solid_items = ex
            .entities
            .values()
            .filter(|e| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP" | "SHELL_BASED_SURFACE_MODEL"))
            .count();
        rd.watch = watch.clone();
        rd.index();
        // Roots: product definitions that are nobody's component.
        let child_pds: HashSet<u64> =
            rd.children.values().flatten().filter_map(|n| ex.get(*n).and_then(|e| e.params().get(4)).and_then(Param::as_ref_id)).collect();
        let mut roots: Vec<u64> =
            ex.entities.iter().filter(|(id, e)| e.name() == "PRODUCT_DEFINITION" && !child_pds.contains(id)).map(|(id, _)| *id).collect();
        roots.sort();
        let identity = IDENTITY;
        let mut tree = Vec::new();
        for pd in roots {
            tree.push(rd.node(pd, identity, identity, &[], None, 0, &mut Vec::new()));
        }
        // Solids outside any product structure (bare geometry files).
        let mut loose: Vec<u64> = rd
            .reps
            .iter()
            .flat_map(|(r, (items, _))| items.iter().map(move |i| (*r, *i)))
            .filter(|(_, i)| !rd.used_items.contains(i))
            .filter(|(_, i)| {
                ex.get(*i)
                    .is_some_and(|e| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP" | "SHELL_BASED_SURFACE_MODEL"))
            })
            .map(|(r, _)| r)
            .collect();
        loose.sort();
        loose.dedup();
        if !loose.is_empty() {
            let mut bodies = Vec::new();
            rd.node_items = rd.used_items.clone();
            rd.visits = 0;
            for r in loose {
                rd.rep_bodies(r, &identity, &[], "Import", None, &mut bodies, 0);
            }
            tree.push(StepNode { name: "Import".into(), transform: identity, bodies, children: Vec::new() });
        }
        let single = workers <= 1;
        Ok(StepReading { rd, tree, key, small: text.len() <= KEEP_SMALL_BYTES, single })
    }

    /// Convert the next solid (or batch of solids); false once all are converted (or the
    /// watch cancelled the import).
    pub fn step(&mut self) -> bool {
        self.rd.convert_next(self.single)
    }

    /// Solids converted, of the solids to convert.
    pub fn progress(&self) -> (usize, usize) {
        (self.rd.converted, self.rd.plan.len())
    }

    /// The import, once [`Self::step`] has converted every solid.
    pub fn finish(mut self) -> Result<StepImport> {
        while self.step() {}
        let rd = &mut self.rd;
        // Cancelled: what was read so far is dropped here, not cached.
        if rd.cancelled || rd.watch.as_ref().is_some_and(|w| w.cancelled()) {
            return Err(crate::watch::cancelled_error("STEP import"));
        }
        let mut tree = std::mem::take(&mut self.tree);
        for n in &mut tree {
            rd.remap_node(n);
        }
        tree.retain(|n| !n.bodies.is_empty() || !n.children.is_empty());
        let ex = rd.ex.clone();
        if let Some(first) = ex.duplicates.first() {
            rd.warn(format!("{} entity ids are defined twice (first #{first}); the first definitions were used", ex.duplicates.len()));
        }
        let warnings: Vec<String> = rd.warnings.iter().map(|(m, n)| if *n > 1 { format!("{m} (×{n})") } else { m.clone() }).collect();
        if rd.bodies.is_empty() {
            let why = warnings.first().cloned().unwrap_or_else(|| "the file has no solids".into());
            return Err(KernelError::Failed(format!("STEP import: no bodies ({why})")));
        }
        let bodies = std::mem::take(&mut rd.bodies);
        Ok(StepImport { bodies, tree, warnings, schema: ex.schemas.join(", "), system: ex.originating_system.clone() })
    }

    /// [`Self::finish`], and the import is cached for the next read of the same text.
    pub fn finish_shared(self) -> Result<Arc<StepImport>> {
        let (key, small) = (self.key, self.small);
        let imp = Arc::new(self.finish()?);
        remember(key, &imp, small);
        Ok(imp)
    }
}

#[cfg(test)]
mod conic_tests;
#[cfg(test)]
mod tests;
