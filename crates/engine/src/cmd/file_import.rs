//! Where an imported file's bodies go (#1386). Every import (STEP, IGES, 3MF, STL, OBJ) becomes a
//! new component named after the file, placed in the active component at the file's origin, and
//! its feature belongs to that component. A STEP assembly keeps its structure as components
//! nested under it, named after the file's products; a part (or sub-assembly) the file places
//! more than once becomes one component with an occurrence per placement, its later instances
//! not built again by the Import feature (`skip`). The contents of an imported assembly are held
//! together by rigid joints, so the import moves as one; `component.unlock` releases them.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use serde_json::{Value, json};
use solidcraft_doc::joints::{Joint, JointKind, JointOrigin, Snap};
use solidcraft_doc::kernel::{StepImport, StepNode};
use solidcraft_doc::{FeatureKind, IDENTITY, Mat};
use solidcraft_geom::Vec3;

use crate::{EngineError, Result, Session};

/// Most components one import makes below its own (the rest of the file stays in the parent).
const MAX_COMPONENTS: usize = 5_000;
/// Deepest nesting followed (below the document's placement depth limit).
const MAX_DEPTH: usize = 32;

/// A component the import makes below its own.
struct Slot {
    name: String,
}

/// A placement of a slot in another.
struct Place {
    slot: usize,
    parent: Option<usize>,
    transform: Mat,
}

/// How a STEP file's tree becomes components.
#[derive(Default)]
struct Plan {
    slots: Vec<Slot>,
    places: Vec<Place>,
    /// Body index → the slot it goes to, and the transform from that slot's frame to the import
    /// component's (where the Import feature builds it).
    bodies: Vec<(usize, usize, Mat)>,
    /// Bodies not built: later instances.
    skip: Vec<u64>,
}

/// A placement STEP assemblies can carry over as an occurrence: finite, rigid, not mirrored.
fn rigid(m: &Mat) -> bool {
    crate::cmd::component::valid_position(m)
}

/// What an import's file says about its structure.
#[derive(Clone, Copy)]
pub(crate) enum Tree<'a> {
    /// Nothing: every body in the import's component.
    Flat,
    /// A STEP file's products and placements.
    Step(&'a StepImport),
    /// A mesh file's components (glTF nodes), bodies already in their component's frame
    /// (checked with [`mesh_plan_fits`] first).
    Mesh(&'a solidcraft_io::MeshPlan),
}

/// Can the components of `plan` be made as it says? (Otherwise its bodies are built in place.)
pub(crate) fn mesh_plan_fits(plan: &solidcraft_io::MeshPlan) -> bool {
    Plan::of_mesh(plan).is_some()
}

impl Plan {
    /// A glTF file's node plan, as it is; `None` when an occurrence can't hold it (a placement
    /// that isn't rigid, or components out of order), so the caller builds the bodies in place.
    fn of_mesh(plan: &solidcraft_io::MeshPlan) -> Option<Plan> {
        let mut made = 0usize;
        let mut places = Vec::with_capacity(plan.places.len());
        for p in &plan.places {
            // Components are made in the order of their first placement, inside one made before.
            if p.slot > made || p.slot >= plan.slots.len() || p.parent.is_some_and(|q| q >= made || q == p.slot) || !rigid(&p.transform) {
                return None;
            }
            if p.slot == made {
                made += 1;
            }
            places.push(Place { slot: p.slot, parent: p.parent, transform: p.transform });
        }
        if plan.bodies.iter().flatten().any(|s| *s >= made) {
            return None;
        }
        let slots = plan.slots.iter().take(made).map(|n| Slot { name: n.clone() }).collect();
        let bodies = plan.bodies.iter().enumerate().filter_map(|(b, slot)| slot.map(|s| (b, s, IDENTITY))).collect();
        Some(Plan { slots, places, bodies, skip: Vec::new() })
    }

    fn of(imp: &StepImport) -> Plan {
        let mut plan = Plan::default();
        // One root product is the import's own component; several are components under it.
        let nodes: Vec<&StepNode> = match imp.tree.as_slice() {
            [root] => root.children.iter().collect(),
            roots => roots.iter().collect(),
        };
        let mut seen: HashMap<u64, usize> = HashMap::new();
        for n in nodes {
            plan.walk(imp, n, None, &IDENTITY, &mut seen, 0);
        }
        plan.skip.sort_unstable();
        plan.skip.dedup();
        plan
    }

    fn walk(&mut self, imp: &StepImport, node: &StepNode, parent: Option<usize>, parent_world: &Mat, seen: &mut HashMap<u64, usize>, depth: usize) {
        if !has_bodies(node, 0) {
            return;
        }
        let world = solidcraft_doc::mat_mul(parent_world, &node.transform);
        let inverse = solidcraft_doc::mat_inverse(&world);
        // Too deep, too many, or placed in a way an occurrence can't be: the bodies stay where
        // the parent is (they are built in place, so nothing moves).
        let (Some(inverse), true, true, true) = (inverse, depth < MAX_DEPTH, self.slots.len() < MAX_COMPONENTS, subtree_rigid(node, depth)) else {
            self.flatten(node, parent, parent_world);
            return;
        };
        let key = node_key(imp, node, 0);
        if let Some(&slot) = seen.get(&key) {
            // Another placement of a part already made: an occurrence of its component, and the
            // file's copy of its bodies is not built.
            self.places.push(Place { slot, parent, transform: node.transform });
            collect_bodies(node, &mut self.skip, 0);
            return;
        }
        let slot = self.slots.len();
        self.slots.push(Slot { name: node.name.trim().to_string() });
        seen.insert(key, slot);
        self.places.push(Place { slot, parent, transform: node.transform });
        for b in &node.bodies {
            self.bodies.push((*b, slot, inverse));
        }
        for c in &node.children {
            self.walk(imp, c, Some(slot), &world, seen, depth + 1);
        }
    }

    /// Every body below `node` goes to `parent`.
    fn flatten(&mut self, node: &StepNode, parent: Option<usize>, parent_world: &Mat) {
        let Some(parent) = parent else { return };
        let Some(inverse) = solidcraft_doc::mat_inverse(parent_world) else { return };
        let mut all = Vec::new();
        collect_bodies(node, &mut all, 0);
        self.bodies.extend(all.into_iter().map(|b| (b as usize, parent, inverse)));
    }
}

fn has_bodies(node: &StepNode, depth: usize) -> bool {
    !node.bodies.is_empty() || (depth < 64 && node.children.iter().any(|c| has_bodies(c, depth + 1)))
}

fn subtree_rigid(node: &StepNode, depth: usize) -> bool {
    rigid(&node.transform) && (depth >= 64 || node.children.iter().all(|c| subtree_rigid(c, depth + 1)))
}

fn collect_bodies(node: &StepNode, out: &mut Vec<u64>, depth: usize) {
    out.extend(node.bodies.iter().map(|b| *b as u64));
    if depth < 64 {
        for c in &node.children {
            collect_bodies(c, out, depth + 1);
        }
    }
}

/// What makes two placements the same part: the product's name, the file items its bodies come
/// from and, below it, the same parts placed the same way.
fn node_key(imp: &StepImport, node: &StepNode, depth: usize) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node.name.hash(&mut h);
    for b in &node.bodies {
        imp.bodies.get(*b).map(|b| b.source_item).hash(&mut h);
    }
    if depth < 64 {
        for c in &node.children {
            node_key(imp, c, depth + 1).hash(&mut h);
            for v in c.transform.iter().flatten() {
                // Placements equal to within rounding are the same placement.
                ((v * 1e6).round() as i64).hash(&mut h);
            }
        }
    }
    h.finish()
}

/// The import's component, as the command reports it.
pub(crate) struct Placed {
    pub component: u64,
    pub occurrence: Option<u64>,
    pub name: String,
    /// Components made below it (an assembly's parts and sub-assemblies).
    pub components: usize,
    /// Rigid joints holding its contents together.
    pub joints: Vec<u64>,
}

impl Placed {
    pub fn report(&self, out: &mut Value) {
        out["component"] = json!(self.component);
        out["component_name"] = json!(self.name);
        out["occurrence"] = json!(self.occurrence);
        if self.components > 0 {
            out["subcomponents"] = json!(self.components);
            out["rigid_joints"] = json!(self.joints.len());
        }
    }
}

/// The component an import makes: named `name`, in the active component at its origin.
fn new_component(s: &mut Session, name: &str) -> Result<(u64, Option<u64>)> {
    let parent = s.active_component;
    let id = s.doc_mut().add_component(Some(name), parent)?;
    Ok((id, s.doc.occurrence_of(id).map(|o| o.id)))
}

/// The name of an imported file's component: `name` if given, else the file's name without its
/// extension (`bracket.step` → "bracket").
pub(crate) fn component_name(path: &str, name: Option<&str>) -> String {
    if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
        return n.chars().take(128).collect();
    }
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(128).collect())
        .unwrap_or_else(|| "Import".into())
}

/// Add an import feature `kind` (named `feature_name`, with the file's `body_names`) inside a
/// new component named `name`, and for a STEP assembly (`imp`) the components of its tree.
/// Evaluates the design. Returns the feature and the component.
pub(crate) fn add_in_component(
    s: &mut Session,
    mut kind: FeatureKind,
    feature_name: &str,
    body_names: Vec<String>,
    name: &str,
    tree: Tree<'_>,
) -> Result<(u64, Placed)> {
    let (top, top_occ) = new_component(s, name)?;
    let (plan, imp) = match tree {
        Tree::Flat => (Plan::default(), None),
        Tree::Step(imp) => (Plan::of(imp), Some(imp)),
        Tree::Mesh(m) => (Plan::of_mesh(m).ok_or_else(|| EngineError::Other("import: the file's node tree can't be placed".into()))?, None),
    };
    // The components, each made at its first placement; the others are further occurrences.
    let mut comps: Vec<u64> = Vec::with_capacity(plan.slots.len());
    let mut first_occ: Vec<Option<u64>> = Vec::with_capacity(plan.slots.len());
    let mut joints: Vec<(u64, u64, Mat)> = Vec::new();
    for place in &plan.places {
        let parent_comp = place.parent.and_then(|p| comps.get(p).copied()).unwrap_or(top);
        let parent_occ = match place.parent {
            Some(p) => first_occ.get(p).copied().flatten(),
            None => top_occ,
        };
        let occ = if let Some(&comp) = comps.get(place.slot) {
            s.doc_mut().add_occurrence(comp, parent_comp, place.transform)?
        } else {
            let slot = plan.slots.get(place.slot).ok_or_else(|| EngineError::Other("import: component plan".into()))?;
            let label = if slot.name.is_empty() { None } else { Some(slot.name.as_str()) };
            let comp = s.doc_mut().add_component(label, parent_comp)?;
            comps.push(comp);
            let occ = s.doc.occurrence_of(comp).map(|o| o.id).ok_or_else(|| EngineError::Other("import: occurrence".into()))?;
            if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
                o.transform = place.transform;
            }
            first_occ.push(Some(occ));
            occ
        };
        if let Some(a) = parent_occ {
            joints.push((a, occ, place.transform));
        }
    }
    if let FeatureKind::Import { skip, .. } = &mut kind {
        skip.clone_from(&plan.skip);
    }
    // The bodies' names, as the feature will give them (checked once it is built).
    let names = match (&kind, imp) {
        (FeatureKind::Import { skip, .. }, Some(imp)) => solidcraft_doc::import_body_names(&s.model.state(), &body_names, imp, skip),
        // Mesh bodies keep their names unless taken (then they are put right once built).
        (FeatureKind::MeshImport { .. }, _) => body_names.iter().cloned().map(Some).collect(),
        _ => Vec::new(),
    };
    let id = s.doc_mut().add_feature(kind, Some(feature_name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = top;
        f.body_names = body_names;
        f.inserted = true;
    }
    let moves: Vec<(usize, u64, Mat)> = plan.bodies.iter().filter_map(|(b, slot, offset)| comps.get(*slot).map(|c| (*b, *c, *offset))).collect();
    put_bodies(s, &names, &moves);
    let joint_ids = add_rigid_joints(s, &joints)?;
    s.active_sketch = None;
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    // The names as built (they differ from the forecast only when the timeline was not up to
    // date): the bodies go by those.
    if !moves.is_empty() {
        let built: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == id).map(|b| b.name.clone()).collect();
        let forecast: Vec<String> = names.iter().flatten().cloned().collect();
        if built != forecast {
            let mut it = built.into_iter();
            let actual: Vec<Option<String>> = names.iter().map(|n| n.as_ref().and_then(|_| it.next())).collect();
            for n in forecast {
                s.doc_mut().body_components.remove(&n);
                s.doc_mut().body_offsets.remove(&n);
            }
            put_bodies(s, &actual, &moves);
            s.refresh();
        }
    }
    let name = s.doc.components.iter().find(|c| c.id == top).map(|c| c.name.clone()).unwrap_or_default();
    Ok((id, Placed { component: top, occurrence: top_occ, name, components: comps.len(), joints: joint_ids }))
}

/// Record that body `names[b]` lives in component `c`, placed by `offset`.
fn put_bodies(s: &mut Session, names: &[Option<String>], moves: &[(usize, u64, Mat)]) {
    if moves.is_empty() {
        return;
    }
    let d = s.doc_mut();
    for (b, comp, offset) in moves {
        let Some(Some(name)) = names.get(*b) else { continue };
        d.body_components.insert(name.clone(), *comp);
        if !solidcraft_doc::is_identity(offset) {
            d.body_offsets.insert(name.clone(), *offset);
        }
    }
}

/// Rigid joints holding each placement where it is in its parent (`(a, b, transform of b in
/// a)`), shown in the browser's joints but not drawn in the view. Returns their ids.
fn add_rigid_joints(s: &mut Session, pairs: &[(u64, u64, Mat)]) -> Result<Vec<u64>> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    if s.doc.assembly.joints.len() + pairs.len() > solidcraft_doc::joints::MAX_JOINTS {
        // Too many to hold together: the parts are placed, not locked.
        return Ok(Vec::new());
    }
    let mut ids = Vec::with_capacity(pairs.len());
    let point = |m: &Mat, p: Vec3| solidcraft_doc::apply_point(m, p);
    let vector = |m: &Mat, v: Vec3| solidcraft_doc::apply_vector(m, v);
    let made: Vec<Joint> = pairs
        .iter()
        .map(|(a, b, m)| {
            let id = s.doc_mut().allocate_id()?;
            ids.push(id);
            // As built: B's origin, seen from A (B's placement in A) and from B itself.
            Ok(Joint {
                id,
                name: format!("Rigid Group {a}:{b}"),
                kind: JointKind::Rigid,
                a: JointOrigin {
                    occurrence: *a,
                    snap: Snap::Frame { origin: point(m, Vec3::ZERO), z: vector(m, Vec3::Z), x: Some(vector(m, Vec3::X)) },
                },
                b: JointOrigin { occurrence: *b, snap: Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) } },
                values: Vec::new(),
                limits: Vec::new(),
                flip: true,
                offset: 0.0,
                angle: 0.0,
                suppressed: false,
                exprs: Default::default(),
                dynamics: Vec::new(),
            })
        })
        .collect::<Result<_>>()?;
    s.doc_mut().assembly.joints.extend(made);
    // Dozens of coinciding glyphs say nothing: the group shows in the browser instead.
    s.visibility.hidden_joints.retain(|h| !ids.contains(h));
    s.visibility.hidden_joints.extend(ids.iter().copied());
    Ok(ids)
}

impl Session {
    /// The component whose contents hold occurrence `occ` where it is: a rigid joint ties it to
    /// a placement of the component it sits in (an imported assembly's part). `None` when it is
    /// free to move on its own.
    pub fn held_by_rigid_group(&self, occ: u64) -> Option<u64> {
        let parent = self.doc.occurrences.iter().find(|o| o.id == occ)?.parent;
        if parent == 0 {
            return None;
        }
        let of_parent = |o: u64| self.doc.occurrences.iter().any(|x| x.id == o && x.component == parent);
        self.doc
            .assembly
            .joints
            .iter()
            .any(|j| {
                j.kind == JointKind::Rigid
                    && !j.suppressed
                    && ((j.b.occurrence == occ && of_parent(j.a.occurrence)) || (j.a.occurrence == occ && of_parent(j.b.occurrence)))
            })
            .then_some(parent)
    }

    /// The rigid joints that hold `component`'s contents together (an imported assembly's rigid
    /// group): both ends at its own occurrences or at occurrences inside it. `component.unlock`
    /// removes them.
    pub fn contents_joints(&self, component: u64) -> Vec<u64> {
        if component == 0 {
            return Vec::new();
        }
        let inside = |occ: u64| {
            self.doc
                .occurrences
                .iter()
                .find(|o| o.id == occ)
                .is_some_and(|o| o.component == component || self.doc.component_within(o.parent, component))
        };
        self.doc
            .assembly
            .joints
            .iter()
            .filter(|j| j.kind == JointKind::Rigid && inside(j.a.occurrence) && inside(j.b.occurrence))
            .map(|j| j.id)
            .collect()
    }
}
