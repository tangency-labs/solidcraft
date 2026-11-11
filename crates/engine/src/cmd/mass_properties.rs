//! Mass, centre of mass and inertia tensor per occurrence and per rigid group (#9715): what a
//! multibody dynamics solver needs of each moving part.
//!
//! Bodies are measured as `component.properties` measures them (docs/assembly-properties.md):
//! volume and centre from the kernel's measurement, the inertia from how the welded export mesh
//! spreads that volume (`solidcraft_geom::inertia`), at the body's material density (the default
//! material's when none is assigned). Each body is measured once however often it is placed;
//! each placement moves its centre and turns its tensor, and an occurrence's bodies combine by
//! the parallel-axis theorem. An open body adds no mass and is counted.
//!
//! Rigid groups are the occurrences that move as one body: those an unsuppressed rigid joint
//! ties together, and a nested occurrence that no joint places with the occurrence it sits in
//! (the joint solver leaves it where its parent puts it). A group holding the design itself or
//! a grounded occurrence is fixed. Occurrences are taken where the joint solver places them, so
//! an occurrence inside a subassembly placed more than once is listed at the subassembly's first
//! placement (a warning says so). With `every_placement`, it is listed again for each further
//! placement, by its `path`, so the subassembly weighs what it holds wherever it is placed. A
//! further placement stands in the groups as the first one does: what a joint is on there is
//! not taken as one body with what it is inside. A design with more placements than the walk
//! of them takes is refused then, not weighed in part.
//! Inside a linked subassembly, the copy that a flexible occurrence's joints move is a group of
//! its own, and what they do not move belongs to the occurrence's group.

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::joints::{self, JointKind, MAX_JOINTS};
use solidcraft_doc::{Document, IDENTITY, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, Mat, ModelState, apply_point, mat_inverse, mat_mul};
use solidcraft_geom::inertia::{self, Tensor3};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::mass_properties::{AssemblyMassParams, AssemblyMassResult, BodyMassEntry, MassFrame, OccurrenceMass, RigidGroupMass, RigidMass};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "assembly.mass_properties";
const CONTRACT: TypedContract<AssemblyMassParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Mass Properties", crate::checked!("assembly.mass_properties", run))
    .icon("measure")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<AssemblyMassResult>)
    .params(
        "every_placement?: bool (default false: an occurrence inside a subassembly placed more than once is listed at the \
         subassembly's first placement only, with a warning; true: at each of its placements, by its path, in the \
         groups as at the first; refused for a design of 100000 placements or more) → occurrences: \
         [{occurrence (0: the design's own bodies), name, component, path, group, fixed, frame \
         {origin, x, y, z}, bodies: [{name, material, density_g_cm3, closed, volume_mm3, mass_kg}], open_bodies_skipped, mass}], \
         groups: [{group, occurrences, fixed, frame_occurrence, frame, mass}] (occurrences an unsuppressed rigid joint ties \
         together, and a nested occurrence no joint places with its parent; fixed: holds the design or a grounded occurrence), \
         warnings. mass: {mass_kg, center_of_mass_mm (world), center_of_mass_local_mm, inertia_world_kg_mm2, \
         inertia_local_kg_mm2 (about the centre of mass, in world and frame axes)} or null when massless. Read-only; pending \
         moves count; each body at its material's density (the default material's when none is assigned)",
    )];

/// One rigid body's mass properties: mass (kg), centre of mass (mm, world) and the inertia
/// tensor about it (kg·mm², world axes; `None` when a closed body's mesh did not close).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MassBody {
    pub mass: f64,
    pub center: Vec3,
    pub inertia: Option<Tensor3>,
}

impl MassBody {
    pub fn is_finite(&self) -> bool {
        self.mass.is_finite() && self.center.is_finite() && self.inertia.is_none_or(|i| i.iter().flatten().all(|x| x.is_finite()))
    }

    /// Bodies taken as one: the masses add, the centre is mass-weighted, and each tensor moves to
    /// the common centre by the parallel-axis theorem. The tensor is `None` when any part's is.
    /// `None` when the total mass is not positive or a value is not finite.
    pub fn combine(parts: &[MassBody]) -> Option<MassBody> {
        let mass: f64 = parts.iter().map(|p| p.mass).sum();
        if !(mass > 0.0 && mass.is_finite()) {
            return None;
        }
        let center = parts.iter().fold(Vec3::ZERO, |acc, p| acc + p.center * p.mass) * (1.0 / mass);
        let inertia = parts.iter().try_fold([[0.0f64; 3]; 3], |sum, p| {
            let i = inertia::shifted(&p.inertia?, p.mass, p.center - center);
            Some([0, 1, 2].map(|r| [0, 1, 2].map(|k| sum[r][k] + i[r][k])))
        });
        let out = MassBody { mass, center, inertia };
        out.is_finite().then_some(out)
    }
}

/// The 3 × 3 linear part of a placement, row by row (`Mat` is column-major).
fn linear(m: &Mat) -> Tensor3 {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| m[j][i]))
}

fn transpose(a: &Tensor3) -> Tensor3 {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| a[j][i]))
}

fn det3(a: &Tensor3) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// The rotation (or reflection) a placement's axes make, when they are orthonormal.
fn rotation(m: &Mat) -> Option<Tensor3> {
    let a = linear(m);
    let ata = inertia::transformed(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], &transpose(&a));
    let ok = (0..3).all(|i| (0..3).all(|j| (ata[i][j] - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-9));
    ok.then_some(a)
}

fn xyz(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn frame_of(m: &Mat) -> MassFrame {
    let axis = |k: usize| {
        let v = Vec3::new(m[k][0], m[k][1], m[k][2]);
        let len = v.len();
        xyz(if len > 0.0 && len.is_finite() { v * (1.0 / len) } else { v })
    };
    MassFrame { origin: [m[3][0], m[3][1], m[3][2]], x: axis(0), y: axis(1), z: axis(2) }
}

/// A world [`MassBody`] as the frame `m` sees it.
fn rigid_mass(b: &MassBody, m: &Mat) -> RigidMass {
    let local = mat_inverse(m).map(|inv| apply_point(&inv, b.center)).unwrap_or(b.center);
    RigidMass {
        mass_kg: b.mass,
        center_of_mass_mm: xyz(b.center),
        center_of_mass_local_mm: xyz(local),
        inertia_world_kg_mm2: b.inertia,
        // Rᵀ I R: the world tensor in the frame's axes.
        inertia_local_kg_mm2: b.inertia.zip(rotation(m)).map(|(i, r)| inertia::transformed(&i, &transpose(&r))),
    }
}

/// Union-find over occurrence indices; the lower index roots a group, so the design (index 0)
/// roots its own.
struct Groups(Vec<usize>);

impl Groups {
    fn root(&self, mut i: usize) -> usize {
        // Bounded: each step goes to a lower index.
        for _ in 0..self.0.len() {
            match self.0.get(i) {
                Some(&parent) if parent != i => i = parent,
                _ => break,
            }
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.root(a), self.root(b));
        if let Some(slot) = self.0.get_mut(ra.max(rb)) {
            *slot = ra.min(rb);
        }
    }
}

/// Occurrence ids from the top of the assembly down, as `joints::occurrence_world` walks them.
fn canonical_path(doc: &Document, occ: u64) -> Vec<u64> {
    let mut path = Vec::new();
    let mut cur = doc.occurrences.iter().find(|o| o.id == occ);
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        path.push(o.id);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    path.reverse();
    path
}

fn out_of_range() -> EngineError {
    bad(CMD, "mass properties exceed supported numeric range")
}

impl Session {
    /// Mass, centre of mass and inertia of every occurrence and rigid group of the design, as
    /// `assembly.mass_properties` returns them (kg, mm, kg·mm²), pending moves included.
    pub fn mass_properties(&self) -> Result<AssemblyMassResult> {
        self.mass_properties_listing(false)
    }

    /// [`Self::mass_properties`]. With `every_placement`, an occurrence inside a subassembly
    /// placed more than once is listed at each of its placements, not at the first one only.
    pub fn mass_properties_listing(&self, every_placement: bool) -> Result<AssemblyMassResult> {
        let mut doc = (*self.doc).clone();
        for occurrence in &mut doc.occurrences {
            if let Some(transform) = self.pending_moves.get(&occurrence.id) {
                occurrence.transform = *transform;
            }
        }
        let state = self.model.state();
        assembly(&doc, &state, every_placement)
    }
}

/// A body measured once: closed, and if so its volume (mm³), centre (mm) and spread, all in its
/// component's frame before its offset.
struct Shape {
    closed: bool,
    volume: f64,
    centroid: Vec3,
    gyration: Option<Tensor3>,
}

/// Every placement of the design ([`Document::placements`]), for a listing that must have them
/// all. That walk gives up past [`MAX_PLACEMENTS`] placements and goes no deeper than
/// [`MAX_PLACEMENT_DEPTH`] levels, without a word, and what is listed from part of a design
/// weighs part of it: an error when the walk stopped at either.
fn all_placements(doc: &Document) -> Result<Vec<(u64, Vec<u64>, Mat)>> {
    let all = doc.placements();
    // The design itself is the first entry, so more entries than the limit is where the walk
    // gives up. A branch is left at the depth limit when what stands there places something.
    let cut_short = all.len() > MAX_PLACEMENTS
        || all.iter().any(|(component, path, _)| path.len() > MAX_PLACEMENT_DEPTH && doc.occurrences.iter().any(|o| o.parent == *component));
    if cut_short {
        return Err(bad(
            CMD,
            format!(
                "the design has more placements than `every_placement` lists ({MAX_PLACEMENTS} or more, or more than {} levels deep): leave it out to list each occurrence at its first placement",
                MAX_PLACEMENT_DEPTH + 1
            ),
        ));
    }
    Ok(all)
}

/// [`Session::mass_properties_listing`] of a document and its evaluated state.
pub fn assembly(doc: &Document, state: &ModelState, every_placement: bool) -> Result<AssemblyMassResult> {
    if doc.occurrences.len() >= MAX_PLACEMENTS {
        return Err(bad(CMD, format!("assembly exceeds {MAX_PLACEMENTS} occurrences")));
    }
    let mut warnings = Vec::new();
    // Index 0 is the design itself; then the occurrences by id, each at its first placement
    // (through the first occurrence of each component above it, where joints place it); then,
    // when asked for, the further placements of the occurrences inside a component that is
    // placed more than once, so that such a component weighs what it holds wherever it is placed.
    let mut ids: Vec<u64> = doc.occurrences.iter().map(|o| o.id).filter(|id| *id != 0).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.insert(0, 0);
    let index: HashMap<u64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut paths: Vec<Vec<u64>> = ids.iter().map(|id| if *id == 0 { Vec::new() } else { canonical_path(doc, *id) }).collect();
    let mut worlds: Vec<Mat> = ids.iter().map(|id| if *id == 0 { IDENTITY } else { joints::occurrence_world(doc, *id) }).collect();
    let mut further: HashMap<Vec<u64>, usize> = HashMap::new();
    if every_placement && doc.occurrences.iter().any(|o| o.parent != 0) {
        for (_, path, world) in all_placements(doc)? {
            let Some(first) = path.last().and_then(|last| index.get(last)).copied() else { continue };
            if paths.get(first) == Some(&path) {
                continue;
            }
            further.insert(path.clone(), ids.len());
            ids.push(path.last().copied().unwrap_or(0));
            paths.push(path);
            worlds.push(world);
        }
    }
    if !every_placement {
        for c in doc.components.iter().filter(|c| doc.occurrences.iter().any(|o| o.parent == c.id)) {
            let copies = doc.occurrences.iter().filter(|o| o.component == c.id).count();
            if copies > 1 {
                warnings.push(format!(
                    "`{}` is placed {copies} times: the occurrences inside it are listed at its first placement only, where joints place them",
                    c.name
                ));
            }
        }
    }
    // The entry of the placement at `path`.
    let entry = |paths: &[Vec<u64>], path: &[u64]| -> Option<usize> {
        let first = path.last().and_then(|last| index.get(last)).copied().filter(|at| paths.get(*at).is_some_and(|own| own.as_slice() == path));
        first.or_else(|| further.get(path).copied())
    };

    let mut groups = Groups((0..ids.len()).collect());
    let live: Vec<_> = doc.assembly.joints.iter().filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    // What each joint side moves with: its occurrence; inside a linked subassembly, the copy
    // that a flexible occurrence's joints move, or the subassembly's occurrence.
    let inside = if doc.links.is_empty() { Vec::new() } else { doc.sides_inside(&live.iter().flat_map(|j| [&j.a, &j.b]).collect::<Vec<_>>()) };
    let anchor = |k: usize, side: &joints::JointOrigin| -> Option<usize> {
        match inside.get(k).copied().unwrap_or(joints::Inside::Outside) {
            joints::Inside::Outside => index.get(&side.occurrence).copied(),
            joints::Inside::With { top } => index.get(&top).copied(),
            joints::Inside::Moved { inner, .. } => {
                let mut full = canonical_path(doc, side.occurrence);
                full.extend(&side.inside);
                let cut = full.iter().position(|id| *id == inner)?;
                entry(&paths, full.get(..=cut)?)
            }
            joints::Inside::Elsewhere { .. } => None,
        }
    };
    let anchors: Vec<(Option<usize>, Option<usize>)> = live.iter().enumerate().map(|(k, j)| (anchor(2 * k, &j.a), anchor(2 * k + 1, &j.b))).collect();
    let mut jointed: HashSet<usize> = anchors.iter().flat_map(|(a, b)| [*a, *b]).flatten().collect();
    for (j, (a, b)) in live.iter().zip(&anchors) {
        if j.kind == JointKind::Rigid
            && let (Some(a), Some(b)) = (*a, *b)
        {
            groups.join(a, b);
        }
    }
    // Listed at every placement, what a joint is on at one placement is on that joint wherever
    // it is placed again: the joint solver places a component's occurrences once, and every
    // placement of the component shows them so. A joint side names the last occurrences of
    // its placement (the occurrence it is on; inside a flexible occurrence, that one down to the
    // copy its joints move), and the further placements of the same thing end in those too.
    // Without this a further placement would be one rigid body with what it is inside, whatever
    // joints move its parts.
    if !further.is_empty() {
        let mut ending: HashMap<u64, Vec<usize>> = HashMap::new();
        for (at, path) in paths.iter().enumerate() {
            if let Some(last) = path.last() {
                ending.entry(*last).or_default().push(at);
            }
        }
        // How many occurrences at the end of entry `at`'s placement joint side `k` names.
        let named = |k: usize, at: usize| -> usize {
            let path = paths.get(at).map_or(&[][..], Vec::as_slice);
            match inside.get(k).copied().unwrap_or(joints::Inside::Outside) {
                joints::Inside::Moved { top, .. } => path.iter().rposition(|id| *id == top).map_or(path.len(), |from| path.len() - from),
                _ => 1,
            }
        };
        // The entries whose placements end as entry `at`'s does in its last `tail` occurrences:
        // that entry, and the same thing wherever it is placed again.
        let placed_as = |at: usize, tail: usize| -> Vec<usize> {
            let Some(end) = paths.get(at).and_then(|path| path.get(path.len().saturating_sub(tail)..)) else { return Vec::new() };
            let Some(last) = end.last() else { return Vec::new() };
            ending.get(last).into_iter().flatten().copied().filter(|other| paths.get(*other).is_some_and(|path| path.ends_with(end))).collect()
        };
        for (k, (j, (a, b))) in live.iter().zip(&anchors).enumerate() {
            let sides = [(2 * k, *a), (2 * k + 1, *b)].map(|(side, at)| at.map(|at| (at, named(side, at))));
            for (at, tail) in sides.iter().flatten() {
                jointed.extend(placed_as(*at, *tail));
            }
            // A rigid joint ties its sides together at each placement of what holds them both.
            if j.kind == JointKind::Rigid
                && let [Some((a, tail_a)), Some((b, tail_b))] = sides
                && let (Some(first_a), Some(first_b)) = (paths.get(a), paths.get(b))
            {
                let shared = first_a.iter().zip(first_b).take_while(|(x, y)| x == y).count();
                let (Some(below_a), Some(below_b), Some(end_b)) =
                    (first_a.get(shared..), first_b.get(shared..), first_b.get(first_b.len().saturating_sub(tail_b)..))
                else {
                    continue;
                };
                for again in placed_as(a, tail_a) {
                    // The placement of what holds both sides that this one is under.
                    let Some(holder) = paths.get(again).and_then(|path| path.strip_suffix(below_a)) else { continue };
                    let there: Vec<u64> = holder.iter().chain(below_b).copied().collect();
                    if let Some(mate) = entry(&paths, &there).filter(|mate| paths.get(*mate).is_some_and(|path| path.ends_with(end_b))) {
                        groups.join(again, mate);
                    }
                }
            }
        }
    }
    // A placement no joint is on moves with the placement it is inside.
    for (child, path) in paths.iter().enumerate().skip(1) {
        if path.len() < 2 || jointed.contains(&child) {
            continue;
        }
        if let Some(parent) = path.get(..path.len() - 1).and_then(|above| entry(&paths, above)) {
            groups.join(child, parent);
        }
    }

    // Each placed body measured once, on worker threads.
    let owners = doc.model_body_components(state);
    let placed: HashSet<u64> =
        ids.iter().map(|id| if *id == 0 { 0 } else { doc.occurrences.iter().find(|o| o.id == *id).map_or(u64::MAX, |o| o.component) }).collect();
    let measured: Vec<usize> = (0..state.bodies.len()).filter(|i| owners.get(*i).is_some_and(|c| placed.contains(c))).collect();
    let shapes = solidcraft_kernel::map_on_threads(&measured, 16, |&i| -> Result<Shape> {
        let body = state.bodies.get(i).ok_or_else(|| EngineError::Other("body index out of range".into()))?;
        let closed = crate::cmd::inspect::encloses_volume(&body.body);
        if !closed {
            return Ok(Shape { closed, volume: 0.0, centroid: Vec3::ZERO, gyration: None });
        }
        let sheet = state.sheets.iter().find(|sheet| sheet.body == body.definition_name() && !sheet.perforations.is_empty());
        let (volume, centroid) = if sheet.is_some() {
            let measured = super::inspect::measure_body(body, Some(state))?;
            (measured.volume, measured.centroid)
        } else {
            let measured = solidcraft_kernel::measure_mass(&body.body)?;
            (measured.volume, measured.centroid)
        };
        let mut moments = solidcraft_io::mass_properties(&body.body)?;
        if let (Some(sheet), Some(moments)) = (sheet, moments.as_mut()) {
            sheet.correct_perforation_mass_properties(moments, &IDENTITY)?;
        }
        let gyration = moments.and_then(|p| p.gyration());
        Ok(Shape { closed, volume, centroid, gyration })
    });
    let mut shape: BTreeMap<usize, Shape> = BTreeMap::new();
    for (i, s) in measured.into_iter().zip(shapes) {
        shape.insert(i, s.ok_or_else(|| bad(CMD, "measuring a body failed"))??);
    }

    let mut occurrences = Vec::with_capacity(ids.len());
    let mut totals: Vec<Option<MassBody>> = Vec::with_capacity(ids.len());
    for (i, &id) in ids.iter().enumerate() {
        let (world, path) = (worlds.get(i).copied().unwrap_or(IDENTITY), paths.get(i).cloned().unwrap_or_default());
        let (component, name, fixed) = if id == 0 {
            (0, doc.name.clone(), true)
        } else {
            // Every id came from `doc.occurrences`, so the occurrence is there.
            let o = doc.occurrences.iter().find(|o| o.id == id);
            o.map_or((u64::MAX, String::new(), false), |o| (o.component, o.name.clone(), o.grounded))
        };
        if world.iter().flatten().any(|x| !x.is_finite()) {
            return Err(out_of_range());
        }
        let (mut bodies, mut parts, mut skipped) = (Vec::new(), Vec::new(), 0);
        for (k, body) in state.bodies.iter().enumerate().filter(|(k, _)| owners.get(*k) == Some(&component)) {
            let Some(s) = shape.get(&k) else { continue };
            let density = doc.density(&body.name);
            let mut entry = BodyMassEntry {
                name: body.name.clone(),
                material: doc.body_material(&body.name),
                density_g_cm3: density,
                closed: s.closed,
                ..BodyMassEntry::default()
            };
            if s.closed {
                let at = doc.model_body_offset(body).map_or(world, |offset| mat_mul(&world, offset));
                let l = linear(&at);
                let volume = s.volume * det3(&l).abs();
                // g/cm³ × mm³ is mg; kg is 1e-6 of that.
                let mass = volume * density * 1e-6;
                let inertia = s.gyration.map(|g| inertia::from_gyration(&inertia::transformed(&g, &l), mass));
                let part = MassBody { mass, center: apply_point(&at, s.centroid), inertia };
                if !(part.is_finite() && volume.is_finite()) {
                    return Err(out_of_range());
                }
                if inertia.is_none() {
                    warnings.push(format!("body `{}`: its mesh does not close, so its inertia is unknown", body.name));
                }
                parts.push(part);
                entry.volume_mm3 = Some(volume);
                entry.mass_kg = Some(mass);
            } else {
                skipped += 1;
                warnings.push(format!("body `{}` is not a closed solid: it adds no mass", body.name));
            }
            bodies.push(entry);
        }
        let total = MassBody::combine(&parts);
        if total.is_none() && parts.iter().map(|p| p.mass).sum::<f64>() > 0.0 {
            return Err(out_of_range());
        }
        if total.is_some() && rotation(&world).is_none() {
            warnings.push(format!("`{name}` is scaled: its inertia in its own axes is left out"));
        }
        totals.push(total);
        occurrences.push(OccurrenceMass {
            occurrence: id,
            name,
            component,
            path,
            group: i,
            fixed,
            frame: frame_of(&world),
            bodies,
            open_bodies_skipped: skipped,
            mass: total.map(|t| rigid_mass(&t, &world)),
        });
    }

    // Groups in order of their lowest occurrence id (their union-find root).
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..occurrences.len() {
        by_root.entry(groups.root(i)).or_default().push(i);
    }
    let mut out_groups = Vec::with_capacity(by_root.len());
    for (g, members) in by_root.values().enumerate() {
        let fixed = members.iter().any(|&m| occurrences.get(m).is_some_and(|o| o.fixed));
        let frame_occurrence = members.first().and_then(|&m| ids.get(m)).copied().unwrap_or(0);
        let world = members.first().and_then(|&m| worlds.get(m)).copied().unwrap_or(IDENTITY);
        let parts: Vec<MassBody> = members.iter().filter_map(|&m| totals.get(m).copied().flatten()).collect();
        let total = MassBody::combine(&parts);
        if total.is_none() && !parts.is_empty() {
            return Err(out_of_range());
        }
        for &m in members {
            if let Some(o) = occurrences.get_mut(m) {
                o.group = g;
            }
        }
        out_groups.push(RigidGroupMass {
            group: g,
            occurrences: members.iter().filter_map(|&m| ids.get(m).copied()).collect::<std::collections::BTreeSet<u64>>().into_iter().collect(),
            fixed,
            frame_occurrence,
            frame: frame_of(&world),
            mass: total.map(|t| rigid_mass(&t, &world)),
        });
    }
    Ok(AssemblyMassResult { occurrences, groups: out_groups, warnings, param_warnings: Vec::new() })
}

/// `assembly.mass_properties`: reads `every_placement` and replies with what
/// `Session::mass_properties_listing` works out.
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let every_placement = match p.get("every_placement") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(every)) => *every,
        Some(_) => return Err(bad(CMD, "`every_placement` must be true or false")),
    };
    let r = s.mass_properties_listing(every_placement)?;
    serde_json::to_value(r).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "mass_properties_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
