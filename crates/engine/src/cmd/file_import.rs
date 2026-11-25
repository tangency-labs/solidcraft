//! Where an imported file's bodies go (#1386). Every import (STEP, IGES, 3MF, STL, OBJ) becomes a
//! new component named after the file, placed in the active component at the file's origin, and
//! its feature belongs to that component. A STEP assembly keeps its structure as components
//! nested under it, named after the file's products; a part (or sub-assembly) the file places
//! more than once becomes one component with an occurrence per placement, its later instances
//! not built again by the Import feature (`skip`). The contents of an imported assembly are held
//! together by one rigid group (`RigidGroup`), so the import moves as one; `component.unlock`
//! releases them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use solidcore_record::Vec3;
use solidcraft_doc::joints::{JointKind, RigidGroup};
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
    /// The occurrence's own name, from the file (`None`: the component's name and a number).
    name: Option<String>,
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

/// The occurrence names in use while one import names its placements. A file can give thousands
/// of placements one name, so a name is looked up in a set, and each name remembers the next
/// ` (k)` to try instead of counting up from ` (2)` again.
struct PlacementNames {
    used: HashSet<String>,
    next: HashMap<String, usize>,
}

impl PlacementNames {
    fn new(occurrences: &[solidcraft_doc::Occurrence]) -> Self {
        Self { used: occurrences.iter().map(|o| o.name.clone()).collect(), next: HashMap::new() }
    }

    /// An occurrence made during the import: its name is taken too.
    fn note(&mut self, name: &str) {
        self.used.insert(name.to_string());
    }

    /// The name an occurrence the file names (a KiCad footprint's reference) is given: the file's
    /// name, cut to the 128 characters a rename allows, with ` (2)`, ` (3)`… when another
    /// occurrence has it. `None` keeps the usual `Component:N`: for an empty name, and for one of
    /// digits only, which a command would read as an occurrence id.
    fn take(&mut self, wanted: &str) -> Option<String> {
        let wanted: String = wanted.trim().chars().take(128).collect();
        if wanted.is_empty() || wanted.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let name = if self.used.contains(&wanted) {
            let start = self.next.get(&wanted).copied().unwrap_or(2);
            let (k, name) = (start..).map(|k| (k, format!("{wanted} ({k})"))).find(|(_, name)| !self.used.contains(name))?;
            self.next.insert(wanted, k.saturating_add(1));
            name
        } else {
            wanted
        };
        self.used.insert(name.clone());
        Some(name)
    }
}

/// What an import's file says about its structure.
pub(crate) enum Tree<'a> {
    /// Every body in the import's component, optionally already built by the reader.
    Flat(Option<solidcraft_doc::PreparedMeshImport>),
    /// A STEP file's products and placements.
    Step(&'a StepImport),
    /// A STEP file brought in flat (`file.insert_step`'s `flatten`): every solid a body of the
    /// import's component, each placement of a part a body of its own where the file places it,
    /// with no components below it and no joints.
    StepFlat(&'a StepImport),
    /// A mesh file's components (glTF nodes), bodies already in their component's frame
    /// (checked with [`mesh_plan_fits`] first).
    Mesh(&'a solidcraft_io::MeshPlan),
}

/// Add a STEP file's solids to an existing component: the exact part of an import whose other
/// bodies are meshes (a KiCad board's substrate next to its copper).
pub(crate) fn add_step_in_component(s: &mut Session, component: u64, file: &str, step: String, feature_name: &str) -> Result<u64> {
    let imp = solidcraft_kernel::step_import_shared(&step).map_err(|e| EngineError::Other(format!("import: {e}")))?;
    let kind = FeatureKind::Import { file: file.to_string(), step: step.into(), components: imp.tree.clone(), skip: Vec::new(), records: None };
    let id = s.doc_mut().add_feature(kind, Some(feature_name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = component;
    }
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let names: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == id).map(|b| b.name.clone()).collect();
    let d = s.doc_mut();
    for name in names {
        d.body_components.insert(name, component);
    }
    s.refresh();
    Ok(id)
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
            places.push(Place { slot: p.slot, parent: p.parent, transform: p.transform, name: p.name.clone() });
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
        // (A root whose solids all failed holds no component.)
        let roots: Vec<&StepNode> = imp.tree.iter().filter(|n| has_bodies(n, 0)).collect();
        let nodes: Vec<&StepNode> = match roots.as_slice() {
            [root] => root.children.iter().collect(),
            roots => roots.to_vec(),
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
            self.places.push(Place { slot, parent, transform: node.transform, name: None });
            collect_bodies(node, &mut self.skip, 0);
            return;
        }
        let slot = self.slots.len();
        self.slots.push(Slot { name: node.name.trim().to_string() });
        seen.insert(key, slot);
        self.places.push(Place { slot, parent, transform: node.transform, name: None });
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

/// How the bodies of a stored tree are the bodies of a read of the same file.
struct Matched {
    /// For each body the read gives, the stored index of the same body (`None`: a body the
    /// stored tree doesn't have).
    by_new: Vec<Option<usize>>,
    /// Stored bodies the read doesn't have (a solid this reader leaves out), each with the
    /// read's node it belongs to (child indices from the roots) and its STEP item: its slot,
    /// and with it its name, is kept for it.
    kept: Vec<(Vec<usize>, usize, u64)>,
}

/// The STEP item of each of `node`'s bodies, as the stored tree says (`None` for a tree saved
/// before items were kept, or one whose lists disagree).
fn stored_items(node: &StepNode) -> Option<&[u64]> {
    (!node.items.is_empty() && node.items.len() == node.bodies.len()).then_some(node.items.as_slice())
}

/// The product's solids in file order as the read has them: each one's body (`None`: left
/// out) and STEP item.
fn planned(node: &StepNode, imp: &StepImport) -> Vec<(Option<usize>, u64)> {
    let total = node.bodies.len() + node.left_out.len();
    let mut bodies = node.bodies.iter().enumerate();
    (0..total)
        .filter_map(|at| match node.left_out.iter().find(|l| l.at == at) {
            Some(l) => Some((None, l.item)),
            None => {
                let (j, b) = bodies.next()?;
                let item = node.items.get(j).copied().or_else(|| imp.bodies.get(*b).map(|b| b.source_item))?;
                Some((Some(*b), item))
            }
        })
        .collect()
}

/// Which body of `imp` each body of `stored` is (#13382, #16857). A body is the same one when it
/// is a solid of the same product (same name and place in both trees) read from the same STEP
/// item. A tree saved before items were kept says which solid each body was by its place among
/// the product's solids in file order, which is how its reader numbered them when it read every
/// one; when it read a different number of them, a body is the one with the same name, where
/// the name says. Never by a body's place among the bodies that happened to be read: one solid
/// that fails would give every later body the one before's name. `None` when the trees disagree
/// about their products (a name or a number of children), or a body index is out of range or
/// used twice: then nothing can be matched.
fn matched_bodies(stored: &[StepNode], imp: &StepImport, stored_names: &[String]) -> Option<Matched> {
    let count = imp.bodies.len();
    let mut m = Matched { by_new: vec![None; count], kept: Vec::new() };
    let mut taken = std::collections::HashSet::new();
    let mut pending: Vec<(&[StepNode], &[StepNode], Vec<usize>)> = vec![(stored, &imp.tree, Vec::new())];
    while let Some((was, is, path)) = pending.pop() {
        if was.len() != is.len() {
            return None;
        }
        for (k, (a, b)) in was.iter().zip(is).enumerate() {
            if a.name != b.name {
                return None;
            }
            let mut here = path.clone();
            here.push(k);
            for old in &a.bodies {
                if !taken.insert(*old) {
                    return None;
                }
            }
            let now = planned(b, imp);
            // Each stored body's solid, by its STEP item.
            let solids: Vec<(usize, Option<u64>)> = match stored_items(a) {
                Some(items) => a.bodies.iter().copied().zip(items.iter().copied().map(Some)).collect(),
                None if a.bodies.len() == now.len() => a.bodies.iter().copied().zip(now.iter().map(|(_, item)| Some(*item))).collect(),
                None => a.bodies.iter().map(|old| (*old, None)).collect(),
            };
            let mut used = std::collections::HashSet::new();
            let mut unnamed = Vec::new();
            for (old, item) in solids {
                match item {
                    Some(item) => match now.iter().find(|(_, i)| *i == item) {
                        Some((Some(new), _)) => {
                            let slot = m.by_new.get_mut(*new)?;
                            if slot.is_some() {
                                return None;
                            }
                            *slot = Some(old);
                            used.insert(*new);
                        }
                        // Not read now: its slot is kept for it.
                        _ => m.kept.push((here.clone(), old, item)),
                    },
                    None => unnamed.push(old),
                }
            }
            // By name, where one stored body and one body read have it.
            let name_of = |old: usize| stored_names.get(old).map(|n| n.trim()).filter(|n| !n.is_empty());
            for old in unnamed {
                let Some(name) = name_of(old) else { continue };
                let stored_with = a.bodies.iter().filter(|o| name_of(**o) == Some(name)).count();
                let mut read_with = b.bodies.iter().filter(|new| imp.bodies.get(**new).is_some_and(|x| x.name.trim() == name));
                let (Some(new), None) = (read_with.next(), read_with.next()) else { continue };
                if stored_with != 1 || used.contains(new) {
                    continue;
                }
                let slot = m.by_new.get_mut(*new)?;
                if slot.is_some() {
                    return None;
                }
                *slot = Some(old);
                used.insert(*new);
            }
            pending.push((&a.children, &b.children, here));
        }
    }
    Some(m)
}

/// An Import feature written against `imp`'s numbering of the file's bodies (#13382). The
/// feature keeps its body names, skipped bodies and linked face names by body index, as the
/// STEP reader that inserted the file numbered them. A later reader can number the same file's
/// bodies differently, for example when it reads a body an earlier one left out, or leaves out
/// one an earlier one read (#16857). Each name then went to the wrong body, which its name put
/// in another component: bodies opened moved and with another body's shape, and features
/// after the import worked on the wrong body. The stored tree says which body each index was,
/// so each stored value moves to the index `imp` gives that body. A body only `imp` has gets
/// the reader's name, and is skipped when a new insert would skip it. A stored body `imp`
/// doesn't have keeps its name in a slot after `imp`'s bodies, which nothing builds, so its
/// name goes to no other body and comes back with the solid when a reader reads it again.
/// Returns whether anything changed; the feature is left as it is when the trees can't be
/// matched.
pub(crate) fn realign_import(f: &mut solidcraft_doc::Feature, imp: &StepImport) -> bool {
    let FeatureKind::Import { components, skip, .. } = &f.kind else { return false };
    if components.is_empty() || *components == imp.tree {
        return false;
    }
    let count = imp.bodies.len();
    let Some(matched) = matched_bodies(components, imp, &f.body_names) else { return false };
    let stored_count = {
        let mut all = Vec::new();
        for n in components {
            collect_bodies(n, &mut all, 0);
        }
        all.len()
    };
    if stored_count == count && matched.kept.is_empty() && matched.by_new.iter().enumerate().all(|(k, m)| *m == Some(k)) {
        // The same numbering: only placements, names or items that don't move bodies differ.
        return false;
    }
    let old_skip: std::collections::HashSet<u64> = skip.iter().copied().collect();
    let mut new_skip: std::collections::HashSet<u64> = Plan::of(imp).skip.into_iter().collect();
    // A flat import (`flatten`) built the later instances a nested one skips: a body only `imp`
    // has is then built too.
    let flat = new_skip.iter().any(|k| matched.by_new.get(*k as usize).copied().flatten().is_some_and(|old| !old_skip.contains(&(old as u64))));
    if flat {
        new_skip.clear();
    }
    let skip: Vec<u64> = (0..count as u64)
        .filter(|k| match matched.by_new.get(*k as usize).copied().flatten() {
            Some(old) => old_skip.contains(&(old as u64)),
            None => new_skip.contains(k),
        })
        .collect();
    // The tree as read, with a slot after the read's bodies for each stored body it lacks.
    let mut tree = imp.tree.clone();
    let mut slots = matched.by_new.clone();
    for (path, old, item) in &matched.kept {
        let Some(n) = node_at(&mut tree, path) else { continue };
        if n.items.len() != n.bodies.len() {
            continue;
        }
        n.bodies.push(slots.len());
        n.items.push(*item);
        slots.push(Some(*old));
    }
    let names = by_new_index(&slots, &f.body_names);
    let links = (!f.link_faces.is_empty()).then(|| by_new_index(&slots, &f.link_faces));
    let threads = (!f.link_threads.is_empty()).then(|| by_new_index(&slots, &f.link_threads));
    let unchanged = names == f.body_names
        && links.as_ref().is_none_or(|l| *l == f.link_faces)
        && threads.as_ref().is_none_or(|t| *t == f.link_threads)
        && matches!(&f.kind, FeatureKind::Import { components, skip: stored, .. } if *components == tree && *stored == skip);
    if unchanged {
        return false;
    }
    f.body_names = names;
    if let Some(links) = links {
        f.link_faces = links;
    }
    if let Some(threads) = threads {
        f.link_threads = threads;
    }
    if let FeatureKind::Import { components, skip: stored, .. } = &mut f.kind {
        *components = tree;
        *stored = skip;
    }
    true
}

/// The node at `path` (child indices from the roots).
fn node_at<'a>(tree: &'a mut [StepNode], path: &[usize]) -> Option<&'a mut StepNode> {
    let (first, rest) = path.split_first()?;
    let n = tree.get_mut(*first)?;
    if rest.is_empty() { Some(n) } else { node_at(&mut n.children, rest) }
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
    /// The rigid group holding its contents together, with how many occurrences it holds.
    pub rigid_group: Option<(u64, usize)>,
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
    hidden: &[bool],
    name: &str,
    tree: Tree<'_>,
) -> Result<(u64, Placed)> {
    let (top, top_occ) = new_component(s, name)?;
    let (plan, imp, prepared) = match tree {
        Tree::Flat(prepared) => (Plan::default(), None, prepared),
        Tree::Step(imp) => (Plan::of(imp), Some(imp), None),
        // The Import feature builds every body where the file places it, in the import's frame:
        // with nothing to move and nothing skipped, that is the flat import.
        Tree::StepFlat(imp) => (Plan::default(), Some(imp), None),
        Tree::Mesh(m) => (Plan::of_mesh(m).ok_or_else(|| EngineError::Other("import: the file's node tree can't be placed".into()))?, None, None),
    };
    // The components, each made at its first placement; the others are further occurrences.
    let mut comps: Vec<u64> = Vec::with_capacity(plan.slots.len());
    // The occurrences made: what the import's rigid group holds.
    let mut made: Vec<u64> = Vec::with_capacity(plan.places.len());
    let mut names = PlacementNames::new(&s.doc.occurrences);
    for place in &plan.places {
        let parent_comp = place.parent.and_then(|p| comps.get(p).copied()).unwrap_or(top);
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
            occ
        };
        // The occurrence was just made, so it is at the end of the list.
        if let Some(o) = s.doc_mut().occurrences.iter_mut().rev().find(|o| o.id == occ) {
            names.note(&o.name);
            if let Some(name) = place.name.as_deref().and_then(|wanted| names.take(wanted)) {
                o.name = name;
            }
        }
        made.push(occ);
    }
    if let FeatureKind::Import { skip, .. } = &mut kind {
        // Bodies the reader already leaves out (a FreeCAD body's hidden intermediate shapes),
        // and later instances of a part placed more than once.
        skip.extend_from_slice(&plan.skip);
        skip.sort_unstable();
        skip.dedup();
    }
    // The bodies' names, as the feature will give them (checked once it is built).
    let names = match (&kind, imp) {
        (FeatureKind::Import { skip, .. }, Some(imp)) => solidcraft_doc::import_body_names(&s.model.state(), &body_names, imp, skip),
        // Mesh bodies keep their names unless taken (then they are put right once built).
        (FeatureKind::MeshImport { .. }, _) => body_names.iter().cloned().map(Some).collect(),
        _ => Vec::new(),
    };
    let names_to_hide = |names: &[Option<String>]| -> Vec<String> {
        names.iter().enumerate().filter(|(k, _)| hidden.get(*k).copied().unwrap_or(false)).filter_map(|(_, name)| name.clone()).collect()
    };
    let mut hidden_names = names_to_hide(&names);
    let id = s.doc_mut().add_feature(kind, Some(feature_name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = top;
        f.body_names = body_names;
        f.inserted = true;
    }
    let moves: Vec<(usize, u64, Mat)> = plan.bodies.iter().filter_map(|(b, slot, offset)| comps.get(*slot).map(|c| (*b, *c, *offset))).collect();
    put_bodies(s, &names, &moves);
    let rigid_group = if plan.places.is_empty() { None } else { Some(add_rigid_group(s, top, made)?) };
    s.active_sketch = None;
    if let Some(prepared) = prepared {
        s.model.prime_mesh_import(id, prepared);
    }
    s.sketching_view = None;
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
            hidden_names = names_to_hide(&actual);
        }
    }
    // The file's invisible bodies start hidden; they are still in the model, so Show brings
    // them back like any other body.
    hidden_names.retain(|name| s.model.state().bodies.iter().any(|b| &b.name == name));
    if !hidden_names.is_empty() {
        for name in hidden_names {
            if !s.visibility.hidden_bodies.contains(&name) {
                s.visibility.hidden_bodies.push(name);
            }
        }
        s.refresh_display();
    }
    let name = s.doc.components.iter().find(|c| c.id == top).map(|c| c.name.clone()).unwrap_or_default();
    Ok((id, Placed { component: top, occurrence: top_occ, name, components: comps.len(), rigid_group }))
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

/// Add a design rebuilt from a file's feature history (a FreeCAD file's sketches and the
/// features that use them) inside a new component named `name`. Each sketch dimension gets a
/// parameter of its own, and a body named like one the design already has is numbered.
/// Evaluates the design. Returns the features made and the component.
pub(crate) fn add_design(s: &mut Session, design: &solidcraft_io::NativeDesign, name: &str) -> Result<(Vec<u64>, Placed)> {
    let (top, top_occ) = new_component(s, name)?;
    let made = add_design_in(s, design, top)?;
    let name = s.doc.components.iter().find(|c| c.id == top).map(|c| c.name.clone()).unwrap_or_default();
    Ok((made, Placed { component: top, occurrence: top_occ, name, components: 0, rigid_group: None }))
}

/// Add a design rebuilt from a file (as [`add_design`] does) inside component `top`, which
/// already exists. Evaluates the design. Returns the features made.
pub(crate) fn add_design_in(s: &mut Session, design: &solidcraft_io::NativeDesign, top: u64) -> Result<Vec<u64>> {
    let source = &design.doc;
    let mut sketches: BTreeMap<u64, u64> = BTreeMap::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut bodies: BTreeMap<String, String> = BTreeMap::new();
    let mut used: BTreeSet<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let mut made = Vec::new();
    for f in &source.features {
        let mut kind = f.kind.clone();
        kind.remap_refs(&sketches, &names);
        kind.remap_bodies(&bodies);
        if let FeatureKind::Sketch { sketch, .. } = &mut kind {
            for c in &mut sketch.constraints {
                if let Some(pn) = c.param.clone() {
                    let (e, u) = source
                        .param(&pn)
                        .map(|x| (x.expr.clone(), x.unit.clone()))
                        .ok_or_else(|| EngineError::Other(format!("import: no parameter {pn}")))?;
                    c.param = Some(s.doc_mut().new_model_param(&e, &u));
                }
            }
        }
        let body_names: Vec<String> = f
            .body_names
            .iter()
            .map(|b| {
                let n = if used.contains(b) { solidcraft_doc::copy_name(b, |n| used.contains(n)) } else { b.clone() };
                used.insert(n.clone());
                bodies.insert(b.clone(), n.clone());
                n
            })
            .collect();
        // A fillet's or chamfer's edges are pinned by name, as the commands pin them, so they
        // are found again after an upstream edit moves them.
        let (edge_refs, edge_names) = match &kind {
            FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } => {
                s.refresh();
                let st = s.model.state();
                (
                    solidcraft_doc::edge_refs(&st, body, &solidcraft_doc::plain(edges)),
                    solidcraft_doc::edge_names_for(&st, body, &solidcraft_doc::plain(edges)),
                )
            }
            _ => (Vec::new(), Vec::new()),
        };
        // A sketch or plane named like one the design has is numbered, as a paste numbers it.
        // The features after it reach it by id or through `names`, so they follow the new name.
        let feature_name =
            if s.doc.name_clash(&kind, &f.name) { solidcraft_doc::copy_name(&f.name, |n| s.doc.name_clash(&kind, n)) } else { f.name.clone() };
        let id = s.doc_mut().add_feature(kind, Some(&feature_name))?;
        if let Some(nf) = s.doc_mut().feature_mut(id) {
            nf.component = top;
            nf.body_names = body_names;
            nf.suppressed = f.suppressed;
            nf.edge_refs = edge_refs;
            nf.edge_names = edge_names;
        }
        let new_name = s.doc.feature(id).map(|x| x.name.clone()).unwrap_or_default();
        sketches.insert(f.id, id);
        names.insert(f.name.clone(), new_name);
        made.push(id);
    }
    s.active_sketch = None;
    s.sketching_view = None;
    s.refresh();
    if let Some(e) = made.iter().find_map(|id| s.model.result(*id).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    Ok(made)
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

/// One rigid group holding the contents of `component` (an imported assembly's parts) where
/// they are. Returns its id and how many occurrences it holds.
fn add_rigid_group(s: &mut Session, component: u64, members: Vec<u64>) -> Result<(u64, usize)> {
    let id = s.doc_mut().allocate_id()?;
    let name = s.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
    let group = RigidGroup { id, name, component, members };
    let members = s.doc.rigid_group_members(&group).len();
    s.doc_mut().assembly.rigid_groups.push(group);
    Ok((id, members))
}

impl Session {
    /// The component whose contents hold occurrence `occ` where it is: a rigid group holds the
    /// contents of a component it sits in, a rigid joint ties it to a placement of the component
    /// it sits in (an imported assembly's part, in a design saved before rigid groups), or it is
    /// a part of a linked assembly, which sits where that assembly's design has it. `None` when
    /// it is free to move on its own.
    pub fn held_by_rigid_group(&self, occ: u64) -> Option<u64> {
        let parent = self.doc.occurrences.iter().find(|o| o.id == occ)?.parent;
        if parent == 0 {
            return None;
        }
        if self.doc.rigid_group_holding(occ).is_some() || self.doc.link_keeping_occurrence(occ).is_some() {
            return Some(parent);
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
        // A rigid joint a flexible occurrence of a linked subassembly has from its design is
        // that design's, not what holds an imported assembly together.
        let linked: std::collections::BTreeSet<u64> = self
            .doc
            .links
            .values()
            .filter_map(|link| link.tree.as_ref())
            .flat_map(|tree| tree.flexible.values())
            .flat_map(|joints| joints.values().copied())
            .collect();
        self.doc
            .assembly
            .joints
            .iter()
            .filter(|j| j.kind == JointKind::Rigid && !linked.contains(&j.id) && inside(j.a.occurrence) && inside(j.b.occurrence))
            .map(|j| j.id)
            .collect()
    }

    /// Is something holding `component`'s contents together: a rigid group, or the rigid joints
    /// an import made before rigid groups ([`Self::contents_joints`])? `component.unlock` releases it.
    pub fn contents_held(&self, component: u64) -> bool {
        self.doc.rigid_group_of(component).is_some() || !self.contents_joints(component).is_empty()
    }
}

/// Repeated STEP assembly placements omitted by the Import feature.
pub(crate) fn step_skip(import: &StepImport) -> Vec<u64> {
    Plan::of(import).skip
}
