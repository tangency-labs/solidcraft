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
//! placement (a warning says so).

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::joints::{JointKind, MAX_JOINTS};
use solidcraft_doc::{Document, IDENTITY, MAX_PLACEMENTS, Mat, ModelState, Occurrence, WorldNames, apply_point, mat_inverse, mat_mul};
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
        "no parameters → occurrences: [{occurrence (0: the design's own bodies), name, component, path, group, fixed, frame \
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

/// World transform and canonical occurrence path (top down), as `joints::occurrence_world` walks them.
fn occurrence_world_and_path(occ: u64, occ_by_id: &HashMap<u64, &Occurrence>, first_occ_by_comp: &HashMap<u64, &Occurrence>) -> (Mat, Vec<u64>) {
    let mut world = IDENTITY;
    let mut path = Vec::new();
    let mut cur = occ_by_id.get(&occ).copied();
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        world = mat_mul(&o.transform, &world);
        path.push(o.id);
        if o.parent == 0 {
            break;
        }
        cur = first_occ_by_comp.get(&o.parent).copied();
    }
    path.reverse();
    (world, path)
}

fn out_of_range() -> EngineError {
    bad(CMD, "mass properties exceed supported numeric range")
}

impl Session {
    /// Mass, centre of mass and inertia of every occurrence and rigid group of the design, as
    /// `assembly.mass_properties` returns them (kg, mm, kg·mm²), pending moves included.
    pub fn mass_properties(&self) -> Result<AssemblyMassResult> {
        let mut doc = self.doc.active_assembly().into_owned();
        for occurrence in &mut doc.occurrences {
            if let Some(transform) = self.pending_moves.get(&occurrence.id) {
                occurrence.transform = *transform;
            }
        }
        let state = self.model.state();
        assembly(&doc, &state)
    }
}

/// A body measured once: closed, and if so its volume (mm³), centre (mm) and spread, all in its
/// component's frame before its offset.
struct Shape {
    closed: bool,
    volume: f64,
    centroid: Vec3,
    gyration: Option<Tensor3>,
    measure_error: Option<String>,
    inertia_error: Option<String>,
}

/// [`Session::mass_properties`] of a document and its evaluated state.
pub fn assembly(doc: &Document, state: &ModelState) -> Result<AssemblyMassResult> {
    if doc.occurrences.len() >= MAX_PLACEMENTS {
        return Err(bad(CMD, format!("assembly exceeds {MAX_PLACEMENTS} occurrences")));
    }
    let mut warnings = Vec::new();
    // Index 0 is the design itself; then the occurrences by id.
    let mut ids: Vec<u64> = doc.occurrences.iter().map(|o| o.id).filter(|id| *id != 0).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.insert(0, 0);
    let index: HashMap<u64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let mut groups = Groups((0..ids.len()).collect());
    // A rigid group holds its parts as rigid joints would.
    let all = doc.joints_with_rigid_groups();
    let live: Vec<_> = all.iter().filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    let jointed: HashSet<u64> = live.iter().flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    for j in live.iter().filter(|j| j.kind == JointKind::Rigid) {
        if let (Some(&a), Some(&b)) = (index.get(&j.a.occurrence), index.get(&j.b.occurrence)) {
            groups.join(a, b);
        }
    }
    let mut occ_by_id: HashMap<u64, &Occurrence> = HashMap::with_capacity(doc.occurrences.len());
    let mut first_occ_by_comp: HashMap<u64, &Occurrence> = HashMap::new();
    let mut occ_count_by_comp: HashMap<u64, usize> = HashMap::new();
    let mut has_children: HashSet<u64> = HashSet::new();
    for o in &doc.occurrences {
        occ_by_id.entry(o.id).or_insert(o);
        first_occ_by_comp.entry(o.component).or_insert(o);
        *occ_count_by_comp.entry(o.component).or_default() += 1;
        has_children.insert(o.parent);
    }
    for o in doc.occurrences.iter().filter(|o| o.parent != 0 && !jointed.contains(&o.id)) {
        if let (Some(&child), Some(&parent)) = (index.get(&o.id), first_occ_by_comp.get(&o.parent).and_then(|p| index.get(&p.id))) {
            groups.join(child, parent);
        }
    }
    for c in doc.components.iter().filter(|c| has_children.contains(&c.id)) {
        let copies = occ_count_by_comp.get(&c.id).copied().unwrap_or(0);
        if copies > 1 {
            warnings.push(format!(
                "`{}` is placed {copies} times: the occurrences inside it are listed at its first placement only, where joints place them",
                c.name
            ));
        }
    }

    // Each placed body measured once, on worker threads.
    let owners = doc.model_body_components(state);
    let world_names = WorldNames::new(doc, state);
    let placed: HashSet<u64> = ids.iter().map(|id| if *id == 0 { 0 } else { occ_by_id.get(id).map_or(u64::MAX, |o| o.component) }).collect();
    let measured: Vec<usize> = (0..state.bodies.len()).filter(|i| owners.get(*i).is_some_and(|c| placed.contains(c))).collect();
    let mut bodies_by_comp: HashMap<u64, Vec<usize>> = HashMap::new();
    for (k, &owner) in owners.iter().enumerate() {
        bodies_by_comp.entry(owner).or_default().push(k);
    }
    let shapes = solidcraft_kernel::map_on_threads(&measured, 16, |&i| -> Shape {
        let Some(body) = state.bodies.get(i) else {
            return Shape {
                closed: false,
                volume: 0.0,
                centroid: Vec3::ZERO,
                gyration: None,
                measure_error: Some("body index out of range".into()),
                inertia_error: None,
            };
        };
        let closed = body.is_closed();
        if !closed {
            return Shape { closed, volume: 0.0, centroid: Vec3::ZERO, gyration: None, measure_error: None, inertia_error: None };
        }
        let sheet = state.surface_sheets.iter().find(|sheet| sheet.body == body.definition_name() && !sheet.perforations.is_empty());
        let volume_centroid: Result<(f64, Vec3)> = if sheet.is_some() {
            super::inspect::measure_body(body, Some(state)).map(|measured| (measured.volume, measured.centroid))
        } else {
            solidcraft_kernel::measure_mass(&body.body).map(|measured| (measured.volume, measured.centroid)).map_err(EngineError::from)
        };
        let (volume, centroid) = match volume_centroid {
            Ok(pair) => pair,
            Err(err) => {
                return Shape {
                    closed: false,
                    volume: 0.0,
                    centroid: Vec3::ZERO,
                    gyration: None,
                    measure_error: Some(err.to_string()),
                    inertia_error: None,
                };
            }
        };
        let (gyration, inertia_error) = match solidcraft_io::mass_properties(&body.body) {
            Ok(Some(mut moments)) => {
                if let Some(sheet) = sheet {
                    match sheet.correct_perforation_mass_properties(&mut moments, &IDENTITY) {
                        Ok(()) => (moments.gyration(), None),
                        Err(err) => (None, Some(err.to_string())),
                    }
                } else {
                    (moments.gyration(), None)
                }
            }
            Ok(None) => (None, None),
            Err(err) => (None, Some(err.to_string())),
        };
        Shape { closed, volume, centroid, gyration, measure_error: None, inertia_error }
    });
    let mut shape: BTreeMap<usize, Shape> = BTreeMap::new();
    for (i, s) in measured.into_iter().zip(shapes) {
        shape.insert(
            i,
            s.unwrap_or_else(|| Shape {
                closed: false,
                volume: 0.0,
                centroid: Vec3::ZERO,
                gyration: None,
                measure_error: Some("measuring this body failed".into()),
                inertia_error: None,
            }),
        );
    }

    let mut occurrences = Vec::with_capacity(ids.len());
    let mut totals: Vec<Option<MassBody>> = Vec::with_capacity(ids.len());
    let mut worlds: Vec<Mat> = Vec::with_capacity(ids.len());
    for (i, &id) in ids.iter().enumerate() {
        let (component, name, world, path, fixed) = if id == 0 {
            (0, doc.name.clone(), IDENTITY, Vec::new(), true)
        } else {
            // Every id came from `doc.occurrences`, so the occurrence is there.
            let o = occ_by_id.get(&id).copied();
            let (component, name, grounded) = o.map_or((u64::MAX, String::new(), false), |o| (o.component, o.name.clone(), o.grounded));
            let (world, path) = occurrence_world_and_path(id, &occ_by_id, &first_occ_by_comp);
            (component, name, world, path, grounded)
        };
        if world.iter().flatten().any(|x| !x.is_finite()) {
            return Err(out_of_range());
        }
        let (mut bodies, mut parts, mut skipped) = (Vec::new(), Vec::new(), 0);
        for k in bodies_by_comp.get(&component).into_iter().flatten().copied() {
            let Some(body) = state.bodies.get(k) else { continue };
            let Some(s) = shape.get(&k) else { continue };
            let density = doc.density(&body.name);
            let mut entry = BodyMassEntry {
                name: world_names.name(&body.name, component, &path),
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
                    if let Some(err) = &s.inertia_error {
                        warnings.push(format!("body `{}`: could not compute inertia ({err})", entry.name));
                    } else {
                        warnings.push(format!("body `{}`: its mesh does not close, so its inertia is unknown", entry.name));
                    }
                }
                parts.push(part);
                entry.volume_mm3 = Some(volume);
                entry.mass_kg = Some(mass);
            } else {
                skipped += 1;
                if let Some(err) = &s.measure_error {
                    warnings.push(format!("body `{}` could not be measured ({err}): it adds no mass", entry.name));
                } else {
                    warnings.push(format!("body `{}` is not a closed solid: it adds no mass", entry.name));
                }
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
        worlds.push(world);
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
            occurrences: members.iter().filter_map(|&m| ids.get(m).copied()).collect(),
            fixed,
            frame_occurrence,
            frame: frame_of(&world),
            mass: total.map(|t| rigid_mass(&t, &world)),
        });
    }
    Ok(AssemblyMassResult { occurrences, groups: out_groups, warnings, param_warnings: Vec::new() })
}

fn run(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = s.mass_properties()?;
    serde_json::to_value(r).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "mass_properties_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
