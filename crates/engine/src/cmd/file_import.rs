//! Where an imported file's bodies go (#1386). Every import (STEP, IGES, 3MF, STL, OBJ) becomes a
//! new component named after the file, placed in the active component at the file's origin, and
//! its feature belongs to that component. A STEP assembly keeps its structure as components
//! nested under it, named after the file's products; a part (or sub-assembly) the file places
//! more than once becomes one component with an occurrence per placement, its later instances
//! not built again by the Import feature (`skip`). The contents of an imported assembly are held
//! together by rigid joints, so the import moves as one; `component.unlock` releases them.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use solidcore_record::Vec3;
use solidcraft_doc::joints::{Joint, JointKind, JointOrigin, Snap};
use solidcraft_doc::kernel::{StepImport, StepNode};
use solidcraft_doc::{FeatureKind, IDENTITY, Mat};

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

/// For each body `now` gives, the index `stored` gave the same body, or `None` for a body
/// `stored` doesn't have. A body is the same one when it has the same place in both trees: the
/// same product at the same position among its siblings, and the same position among that
/// product's bodies. `None` when the trees disagree about their products (a name or a number of
/// children), or a body index is out of range or used twice: then nothing can be matched.
fn matched_bodies(stored: &[StepNode], now: &[StepNode], count: usize) -> Option<Vec<Option<usize>>> {
    let mut matched = vec![None; count];
    let mut taken = std::collections::HashSet::new();
    let mut pending: Vec<(&[StepNode], &[StepNode])> = vec![(stored, now)];
    while let Some((was, is)) = pending.pop() {
        if was.len() != is.len() {
            return None;
        }
        for (a, b) in was.iter().zip(is) {
            if a.name != b.name {
                return None;
            }
            for (old, new) in a.bodies.iter().zip(&b.bodies) {
                let slot = matched.get_mut(*new)?;
                if slot.is_some() || !taken.insert(*old) {
                    return None;
                }
                *slot = Some(*old);
            }
            pending.push((&a.children, &b.children));
        }
    }
    Some(matched)
}

/// An Import feature written against `imp`'s numbering of the file's bodies (#13382). The
/// feature keeps its body names, skipped bodies and linked face names by body index, as the
/// STEP reader that inserted the file numbered them. A later reader can number the same file's
/// bodies differently, for example when it reads a body an earlier one left out. Each name
/// then went to the wrong body, which its name put in another component: bodies opened moved
/// and with another body's shape. The stored tree says which body each index was, so each
/// stored value moves to the index `imp` gives that body. A body only `imp` has gets the
/// reader's name, and is skipped when a new insert would skip it. Returns whether anything
/// changed; the feature is left as it is when the trees can't be matched.
pub(crate) fn realign_import(f: &mut solidcraft_doc::Feature, imp: &StepImport) -> bool {
    let FeatureKind::Import { components, skip, .. } = &f.kind else { return false };
    if components.is_empty() || *components == imp.tree {
        return false;
    }
    let count = imp.bodies.len();
    let Some(matched) = matched_bodies(components, &imp.tree, count) else { return false };
    let stored_count = {
        let mut all = Vec::new();
        for n in components {
            collect_bodies(n, &mut all, 0);
        }
        all.len()
    };
    if stored_count == count && matched.iter().enumerate().all(|(k, m)| *m == Some(k)) {
        // The same numbering: only placements or names that don't move bodies differ.
        return false;
    }
    let old_skip: std::collections::HashSet<u64> = skip.iter().copied().collect();
    let new_skip: std::collections::HashSet<u64> = Plan::of(imp).skip.into_iter().collect();
    let skip: Vec<u64> = (0..count as u64)
        .filter(|k| match matched.get(*k as usize).copied().flatten() {
            Some(old) => old_skip.contains(&(old as u64)),
            None => new_skip.contains(k),
        })
        .collect();
    f.body_names = by_new_index(&matched, &f.body_names);
    if !f.link_faces.is_empty() {
        f.link_faces = by_new_index(&matched, &f.link_faces);
    }
    if let FeatureKind::Import { components, skip: stored, .. } = &mut f.kind {
        components.clone_from(&imp.tree);
        *stored = skip;
    }
    true
}

/// `stored`, kept by the old body indices, as a list by the new ones (empty for a new body).
fn by_new_index<T: Clone + Default>(matched: &[Option<usize>], stored: &[T]) -> Vec<T> {
    matched.iter().map(|m| m.and_then(|old| stored.get(old)).cloned().unwrap_or_default()).collect()
}

/// [`realign_import`] for every Import feature of `doc` that is not suppressed, read with the
/// reader that will build it. The reads are returned so the caller can hold them while it
/// evaluates `doc`: the evaluation then finds each file read already instead of reading it again.
pub(crate) fn realign_imports(doc: &mut solidcraft_doc::Document) -> Vec<std::sync::Arc<StepImport>> {
    let mut reads = Vec::new();
    for f in doc.features.iter_mut().filter(|f| !f.suppressed) {
        let FeatureKind::Import { step, components, .. } = &f.kind else { continue };
        if step.is_empty() || components.is_empty() {
            continue;
        }
        // A file that doesn't read fails the feature when it is evaluated, which reports it.
        let Ok(imp) = solidcraft_doc::kernel::step_import_shared(step) else { continue };
        if realign_import(f, &imp) {
            log::info!("{}: the stored body names follow this version's numbering of the file's bodies", f.name);
        }
        reads.push(imp);
    }
    reads
}

/// The import's component, as the command reports it. The reply types in `schema/insert_mesh.rs`
/// and `schema/insert_step.rs` are what the commands answer with these fields.
pub(crate) struct Placed {
    pub component: u64,
    pub occurrence: Option<u64>,
    pub name: String,
    /// Components made below it (an assembly's parts and sub-assemblies).
    pub components: usize,
    /// Rigid joints holding its contents together.
    pub joints: Vec<u64>,
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

/// Move an import to the origin of the component it was inserted into (#12543): the box around
/// its bodies, where they are placed, comes to sit centred on X and Y with its lowest point on
/// Z = 0. The import's occurrence moves; the file's coordinates in the feature are kept, so the
/// move is its placement and Move Occurrence takes it on from there. Returns the shift, in that
/// component's frame; `None` when the import has no bodies to place.
pub(crate) fn move_to_origin(s: &mut Session, placed: &Placed) -> Result<Option<Vec3>> {
    let occ = placed.occurrence.ok_or_else(|| EngineError::Other("the import has no placement to move".into()))?;
    // A component activated without a path is shown at its first placement, as elsewhere.
    let active = s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(s.active_component));
    // The frame the import was placed in: the origin it goes to.
    let parent = s.doc.path_transform(s.active_component, &active).unwrap_or(IDENTITY);
    let to_parent =
        solidcraft_doc::mat_inverse(&parent).ok_or_else(|| EngineError::Other("the active component's placement can't be inverted".into()))?;
    // This placement of the import only: the bodies whose path runs through its occurrence
    // where it sits in the active component.
    let ours: Vec<String> = s
        .doc
        .body_frames(&s.model.state())
        .into_iter()
        .filter(|(_, _, path, _)| path.starts_with(&active) && path.get(active.len()) == Some(&occ))
        .map(|(name, ..)| name)
        .collect();
    let world = s.world_state()?;
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for body in world.bodies.iter().filter(|b| ours.contains(&b.name)) {
        // Each body's own box in the active component's frame: a turned component's frame would
        // only take a larger box around the world box's corners, leaving the mesh off Z = 0.
        let b = if solidcraft_doc::is_identity(&to_parent) {
            body.body.exact_bounds()?
        } else {
            solidcraft_doc::kernel::place_matrix(&body.body, to_parent)?.exact_bounds()?
        };
        if b.is_empty() {
            continue;
        }
        bounds = bounds.union(&b);
    }
    if bounds.is_empty() {
        return Ok(None);
    }
    let shift = Vec3::new(-(bounds.min.x + bounds.max.x) / 2.0, -(bounds.min.y + bounds.max.y) / 2.0, -bounds.min.z);
    if ![shift.x, shift.y, shift.z].iter().all(|v| v.is_finite()) {
        return Err(EngineError::Other("the import's bounds can't be placed at the origin".into()));
    }
    let mut by = IDENTITY;
    by[3] = [shift.x, shift.y, shift.z, 1.0];
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
        let moved = solidcraft_doc::mat_mul(&by, &o.transform);
        if !solidcraft_doc::is_placement(&moved) {
            return Err(EngineError::Other("moving the import to the origin takes it out of a placement within 1e9 mm".into()));
        }
        o.transform = moved;
    }
    s.refresh();
    Ok(Some(shift))
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
                a: JointOrigin::new(*a, Snap::Frame { origin: point(m, Vec3::ZERO), z: vector(m, Vec3::Z), x: Some(vector(m, Vec3::X)) }),
                b: JointOrigin::new(*b, Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) }),
                values: Vec::new(),
                limits: Vec::new(),
                flip: true,
                offset: 0.0,
                angle: 0.0,
                suppressed: false,
                exprs: Default::default(),
                captured: None,
                constraint: None,
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
