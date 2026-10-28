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
    name: Option<String>,
}

/// How a STEP file's tree becomes components.
#[derive(Default)]
struct Plan {
    share_parts: bool,
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
    Step { import: &'a StepImport, share_parts: bool },
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
            places.push(Place { slot: p.slot, parent: p.parent, transform: p.transform, name: None });
        }
        if plan.bodies.iter().flatten().any(|s| *s >= made) {
            return None;
        }
        let slots = plan.slots.iter().take(made).map(|n| Slot { name: n.clone() }).collect();
        let bodies = plan.bodies.iter().enumerate().filter_map(|(b, slot)| slot.map(|s| (b, s, IDENTITY))).collect();
        Some(Plan { slots, places, bodies, skip: Vec::new(), share_parts: false })
    }

    fn of(imp: &StepImport, share_parts: bool) -> Plan {
        let mut plan = Plan { share_parts, ..Plan::default() };
        // One root product is the import's own component; several are components under it.
        let nodes: Vec<&StepNode> = match imp.tree.as_slice() {
            [root] => root.children.iter().collect(),
            roots => roots.iter().collect(),
        };
        let mut seen: HashMap<u64, Vec<(usize, &StepNode, Mat)>> = HashMap::new();
        for n in nodes {
            plan.walk(imp, n, None, &IDENTITY, &mut seen, 0);
        }
        plan.skip.sort_unstable();
        plan.skip.dedup();
        plan
    }

    fn walk<'a>(
        &mut self,
        imp: &StepImport,
        node: &'a StepNode,
        parent: Option<usize>,
        parent_world: &Mat,
        seen: &mut HashMap<u64, Vec<(usize, &'a StepNode, Mat)>>,
        depth: usize,
    ) {
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
        let key = node_key(imp, node, &world, self.share_parts, 0);
        if let Some(slot) = seen
            .get(&key)
            .and_then(|candidates| {
                candidates.iter().find(|(_, other, other_world)| same_node(imp, node, &world, other, other_world, self.share_parts, 0))
            })
            .map(|(slot, _, _)| *slot)
        {
            // Another placement of a part already made: an occurrence of its component, and the
            // file's copy of its bodies is not built.
            self.places.push(Place { slot, parent, transform: node.transform, name: node.instance.clone() });
            collect_bodies(node, &mut self.skip, 0);
            return;
        }
        let slot = self.slots.len();
        self.slots.push(Slot { name: node.name.trim().to_string() });
        seen.entry(key).or_default().push((slot, node, world));
        self.places.push(Place { slot, parent, transform: node.transform, name: node.instance.clone() });
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
fn body_placement(body: &solidcraft_doc::kernel::ImportedBody, world: &Mat, inverse: Option<&Mat>) -> Option<Mat> {
    // Ordinary B-reps have exactly the product's placement. Avoid inverse roundoff there;
    // mapped items can also place geometry within the product, which must stay part of its identity.
    if body.placement == *world {
        return Some(IDENTITY);
    }
    let local = solidcraft_doc::mat_mul(inverse?, &body.placement);
    local.iter().flatten().all(|v| v.is_finite()).then_some(local)
}

fn node_key(imp: &StepImport, node: &StepNode, world: &Mat, share_parts: bool, depth: usize) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let inverse = solidcraft_doc::mat_inverse(world);
    node.name.hash(&mut h);
    node.product.hash(&mut h);
    if node.has_properties {
        node.definition.hash(&mut h);
    }
    for b in &node.bodies {
        if let Some(b) = imp.bodies.get(*b) {
            let item = if share_parts { b.geometry_item } else { b.source_item };
            item.hash(&mut h);
            b.geometry_units.map(f64::to_bits).hash(&mut h);
            body_placement(b, world, inverse.as_ref()).map(|m| m.map(|column| column.map(f64::to_bits))).hash(&mut h);
            b.name.hash(&mut h);
            b.color.map(|c| c.map(f32::to_bits)).hash(&mut h);
            if let Some(paint) = b.body.paint() {
                paint.opacity.to_bits().hash(&mut h);
                for face in &paint.faces {
                    face.face.hash(&mut h);
                    face.color.map(f32::to_bits).hash(&mut h);
                    face.opacity.to_bits().hash(&mut h);
                }
            }
        }
    }
    if depth < 64 {
        for c in &node.children {
            node_key(imp, c, &solidcraft_doc::mat_mul(world, &c.transform), share_parts, depth + 1).hash(&mut h);
            for v in c.transform.iter().flatten() {
                v.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

fn same_node(imp: &StepImport, a: &StepNode, a_world: &Mat, b: &StepNode, b_world: &Mat, share_parts: bool, depth: usize) -> bool {
    let a_inverse = solidcraft_doc::mat_inverse(a_world);
    let b_inverse = solidcraft_doc::mat_inverse(b_world);
    depth < 64
        && a.name == b.name
        && a.product == b.product
        && (!(a.has_properties || b.has_properties) || a.definition == b.definition)
        && a.bodies.len() == b.bodies.len()
        && a.children.len() == b.children.len()
        && a.bodies.iter().zip(&b.bodies).all(|(a, b)| match (imp.bodies.get(*a), imp.bodies.get(*b)) {
            (Some(a), Some(b)) => {
                a.geometry_item == b.geometry_item
                    && (share_parts || a.source_item == b.source_item)
                    && a.geometry_units == b.geometry_units
                    && body_placement(a, a_world, a_inverse.as_ref())
                        .is_some_and(|local| Some(local) == body_placement(b, b_world, b_inverse.as_ref()))
                    && a.name == b.name
                    && a.closed == b.closed
                    && a.color == b.color
                    && a.body.paint() == b.body.paint()
            }
            _ => false,
        })
        && a.children.iter().zip(&b.children).all(|(a, b)| {
            a.transform == b.transform
                && same_node(
                    imp,
                    a,
                    &solidcraft_doc::mat_mul(a_world, &a.transform),
                    b,
                    &solidcraft_doc::mat_mul(b_world, &b.transform),
                    share_parts,
                    depth + 1,
                )
        })
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
    mut body_names: Vec<String>,
    name: &str,
    tree: Tree<'_>,
) -> Result<(u64, Placed)> {
    let step_file = matches!(&kind, FeatureKind::Import { file, .. }
        if std::path::Path::new(file).extension().and_then(|s| s.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp")));
    let (top, top_occ) = new_component(s, name)?;
    let (mut plan, imp) = match tree {
        Tree::Flat => (Plan::default(), None),
        Tree::Step { import, share_parts } => (Plan::of(import, share_parts), Some(import)),
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
        if let Some(name) = &place.name
            && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ)
        {
            o.name.clone_from(name);
        }
        if let Some(a) = parent_occ {
            joints.push((a, occ, place.transform));
        }
    }
    if let FeatureKind::Import { skip, .. } = &mut kind {
        if let Some(imp) = imp.filter(|_| step_file) {
            let capacity = imp.bodies.len().saturating_sub(plan.skip.len());
            let mut only = Vec::with_capacity(capacity);
            let mut kept_names = Vec::with_capacity(capacity);
            let mut stored = body_names.into_iter();
            for (i, body) in imp.bodies.iter().enumerate() {
                let name = stored.next();
                if plan.skip.binary_search(&(i as u64)).is_err() {
                    only.push(body.planned_index as u64);
                    kept_names.push(name.unwrap_or_else(|| body.name.clone()));
                }
            }
            body_names = kept_names;
            *skip = solidcraft_doc::ImportFilter::Only { only };
        } else {
            *skip = std::mem::take(&mut plan.skip).into();
        }
    }
    // The bodies' names, as the feature will give them (checked once it is built).
    let names = match (&kind, imp) {
        (FeatureKind::Import { skip, .. }, Some(imp)) => solidcraft_doc::import_body_names(&s.model.state(), &body_names, imp, skip),
        // Mesh bodies keep their names unless taken (then they are put right once built).
        (FeatureKind::MeshImport { .. }, _) => body_names.iter().cloned().map(Some).collect(),
        _ => Vec::new(),
    };
    let id = s.doc_mut().add_feature(kind, Some(feature_name))?;
    if step_file {
        let mut imported: std::collections::HashSet<_> = comps.iter().copied().collect();
        imported.insert(top);
        for component in &mut s.doc_mut().components {
            if imported.contains(&component.id) {
                component.imported_from = Some(id);
            }
        }
    }
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
