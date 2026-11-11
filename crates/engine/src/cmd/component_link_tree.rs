//! A linked design's own components, kept as components of the design that links it (#5526).
//!
//! Insert Design used to put every body of the linked design into the one linked component, so
//! a linked assembly came in as one part: its parts could not be told apart, counted in a bill
//! of materials or joined to on their own. A link now keeps the linked design's components and
//! their placements below the linked component ([`LinkTree`]), as an imported STEP assembly
//! keeps the file's (`file_import`). The link's one Import feature still builds every body. Each
//! body belongs to the component that stands for its own in the linked design, and a part placed
//! several times is one component with a placement for each. The placements are the linked
//! design's as it was saved, with the poses it has for the flexible subassemblies inside it.
//!
//! An occurrence of the subassembly is rigid unless made flexible: it moves as one. Its
//! components follow the linked component as any occurrence no joint places does, and no
//! command moves one on its own, so they need no joint and cost the solver nothing, however
//! many they are. A joint of this design made to one of them is on the subassembly's occurrence
//! for the solver (`solidcraft_doc::joints::placed`), so the whole of it moves.
//!
//! Each occurrence of the linked component can be made flexible on its own ([`Keep`]). The
//! linked design's own joints are then joints here too, once for each flexible occurrence, on
//! that occurrence's own copy of the occurrences inside it (#1516), so two occurrences of one
//! subassembly can stand in different poses. The joints stay the linked design's (Update Links
//! writes them again); where they stand is this design's. The joints the linked design has for
//! the flexible occurrences inside it come with its own, level after level: a mechanism any
//! number of levels down moves from here when every occurrence on the way to it is flexible. The occurrences kept for the design
//! stay where it was saved with them, which is what a rigid occurrence shows.
//!
//! [`sync`] makes that tree, and makes it follow the linked design again on Update Links and
//! Relink Source. What the linked design still has keeps its component and placements here, so
//! what refers to them (a joint to a part of the subassembly, a hidden placement) stays.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use solidcraft_doc::joints::{Joint, JointOrigin, MotionLink};
use solidcraft_doc::{Document, LinkTree, Mat, Metadata, ModelState};

use crate::{EngineError, Result, Session};

/// One of the linked design's components, as that design has it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceComponent {
    pub id: u64,
    pub name: String,
    /// The component it was made in (0: the design itself).
    pub parent: u64,
    pub metadata: Metadata,
    pub properties: BTreeMap<String, String>,
}

/// One of the linked design's placements.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceOccurrence {
    pub id: u64,
    pub component: u64,
    /// The component it sits in (0: the design itself).
    pub parent: u64,
    pub transform: Mat,
}

/// The structure of a linked design: the components it places, where, and which of them each
/// linked body belongs to.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SourceTree {
    /// The components the design places, at any depth.
    pub components: Vec<SourceComponent>,
    /// Their placements.
    pub occurrences: Vec<SourceOccurrence>,
    /// For each body the link's Import feature reads, in the file's order: the component of the
    /// linked design it belongs to (0: the design itself), and the inverse of that component's
    /// first placement in the design, which takes the body from where the feature builds it (in
    /// the design's frame) into the component's (`Document::body_offsets`).
    pub bodies: Vec<(u64, Mat)>,
    /// The design's joints as a flexible link carries them ([`Self::with_joints`]); their ids
    /// and their sides' occurrences are the design's (0: the design itself).
    pub joints: Vec<Joint>,
    /// The motion links between those joints, by the design's joint ids.
    pub links: Vec<MotionLink>,
    /// The placements the design's own solve holds still (its grounded ones).
    pub fixed: Vec<u64>,
    /// The poses the design has for the flexible subassemblies inside it, by its own occurrence
    /// ids: the occurrence a pose is for, the flexible occurrences inside that one which the
    /// copy is under, the occurrence inside, and that one's transform.
    pub poses: Vec<(u64, Vec<u64>, u64, Mat)>,
    /// The design's occurrences that are flexible in it, at any depth ([`Self::with_joints`]):
    /// a link's flexible occurrence has a copy of what is inside each of them.
    pub nested: Vec<u64>,
    /// What of the design's joints a flexible link cannot carry, in words.
    pub notes: Vec<String>,
}

/// How a link is to keep its linked design, as a command asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Keep {
    /// The design's components are components inside the linked one. Without, every body is in
    /// the linked component itself: nothing of the design's structure.
    pub structure: bool,
    /// Flexible (the design's joints move its parts here) or rigid (they stay where the design
    /// was saved with them), for [`Self::occurrence`]. `None`: each occurrence as it is.
    pub flexible: Option<bool>,
    /// The occurrence of the linked component `flexible` is for. `None`: every one.
    pub occurrence: Option<u64>,
}

impl Keep {
    /// The design's components kept, each occurrence rigid or flexible as it is.
    pub(super) const STRUCTURE: Keep = Keep { structure: true, flexible: None, occurrence: None };

    /// As a link with this record keeps its design now.
    pub(super) fn of(tree: Option<&LinkTree>) -> Keep {
        Keep { structure: tree.is_some(), ..Keep::STRUCTURE }
    }

    /// Whether the design's components are kept.
    pub(super) fn structure(self) -> bool {
        self.structure
    }

    /// The occurrences of linked component `top` that are flexible once this is done.
    pub(super) fn flexible_in(self, d: &Document, top: u64) -> BTreeSet<u64> {
        if !self.structure {
            return BTreeSet::new();
        }
        let all: BTreeSet<u64> = d.occurrences.iter().filter(|o| o.component == top).map(|o| o.id).collect();
        let tree = d.links.get(&top).and_then(|link| link.tree.as_ref());
        let mut set: BTreeSet<u64> = tree.map(|tree| tree.flexible.keys().copied().filter(|o| all.contains(o)).collect()).unwrap_or_default();
        match (self.flexible, self.occurrence) {
            (None, _) => {}
            (Some(true), None) => set = all,
            (Some(false), None) => set.clear(),
            (Some(true), Some(o)) if all.contains(&o) => {
                set.insert(o);
            }
            (Some(_), Some(o)) => {
                set.remove(&o);
            }
        }
        set
    }
}

impl SourceTree {
    /// The components `doc` places and their placements, found from the design down: a
    /// component nothing places is not part of what the design shows. `bodies` is left empty.
    pub(super) fn of(doc: &Document) -> SourceTree {
        let mut inside: BTreeMap<u64, Vec<&solidcraft_doc::Occurrence>> = BTreeMap::new();
        for o in &doc.occurrences {
            inside.entry(o.parent).or_default().push(o);
        }
        let by_id: BTreeMap<u64, &solidcraft_doc::Component> = doc.components.iter().map(|c| (c.id, c)).collect();
        let mut tree = SourceTree::default();
        let (mut components, mut occurrences) = (BTreeSet::new(), BTreeSet::new());
        let mut queue = VecDeque::from([0u64]);
        while let Some(parent) = queue.pop_front() {
            for o in inside.get(&parent).into_iter().flatten() {
                let Some(c) = by_id.get(&o.component).filter(|c| c.id != 0) else { continue };
                // A design file is not trusted: an id used twice stands for one placement.
                if !occurrences.insert(o.id) {
                    continue;
                }
                tree.occurrences.push(SourceOccurrence { id: o.id, component: c.id, parent, transform: o.transform });
                if components.insert(c.id) {
                    tree.components.push(SourceComponent {
                        id: c.id,
                        name: c.name.clone(),
                        parent: c.parent,
                        metadata: c.metadata.clone(),
                        properties: c.properties.clone(),
                    });
                    queue.push_back(c.id);
                }
            }
        }
        // In the design's own order: an occurrence inside a component is reached through that
        // component's first occurrence, which is to be the same one here as there.
        let mut order: HashMap<u64, usize> = HashMap::new();
        for (at, o) in doc.occurrences.iter().enumerate() {
            order.entry(o.id).or_insert(at);
        }
        tree.occurrences.sort_by_key(|o| order.get(&o.id).copied().unwrap_or(usize::MAX));
        tree.poses = doc
            .assembly
            .poses
            .iter()
            .filter(|p| {
                occurrences.contains(&p.occurrence) && occurrences.contains(&p.inner) && p.through.iter().all(|under| occurrences.contains(under))
            })
            .map(|p| (p.occurrence, p.through.clone(), p.inner, p.transform))
            .collect();
        tree
    }

    /// Whether the design has components to keep: without any, its bodies are the linked
    /// component's own.
    pub(super) fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// With the design's joints, for a link that has a flexible occurrence. Each side is a
    /// frame on what the design's own solve moves for it ([`Document::sides_as_solved`]), and
    /// each number what its expression comes to, so the joint needs none of the design's faces,
    /// sketches or parameters where it is carried to. The joints the design has for the
    /// flexible occurrences inside it are carried with its own: their sides are on those
    /// occurrences' copies (`{occurrence, inside}`). A motion link between two of them keeps
    /// the ratio last worked out; a cam, whose table follows a sketch of the design, is not
    /// carried.
    pub(super) fn with_joints(mut self, doc: &Document, st: &ModelState) -> SourceTree {
        let placed: BTreeSet<u64> = self.occurrences.iter().map(|o| o.id).collect();
        let known = |o: &JointOrigin| (o.occurrence == 0 || placed.contains(&o.occurrence)) && o.inside.iter().all(|id| placed.contains(id));
        // A design file is not trusted: an id used twice stands for one joint.
        let mut seen = BTreeSet::new();
        let joints: Vec<&Joint> = doc.assembly.joints.iter().take(solidcraft_doc::joints::MAX_JOINTS).filter(|j| seen.insert(j.id)).collect();
        let sides: Vec<&JointOrigin> = joints.iter().flat_map(|j| [&j.a, &j.b]).collect();
        let solved = doc.sides_as_solved(st, &sides);
        let mut left = 0usize;
        for (k, j) in joints.iter().enumerate() {
            let side = |at: usize| solved.get(at).cloned().flatten().filter(|o| known(o));
            match (side(2 * k), side(2 * k + 1)) {
                // Both sides on what moves as one: nothing for the joint to move here.
                (Some(a), Some(b)) if a.occurrence == b.occurrence && a.inside == b.inside => left += 1,
                (Some(a), Some(b)) => {
                    // A side read as a frame is no datum plane or axis any more, which is what
                    // `flip` counts from: it turns with that, so the joint does what it did.
                    let flip = j.flip != solidcraft_doc::joints::both_datums(&j.a.snap, &j.b.snap);
                    // What Capture Position saved in the linked design is that design's: here
                    // a joint has none until this design captures its own.
                    self.joints.push(Joint { a, b, flip, exprs: Default::default(), captured: None, ..(*j).clone() });
                }
                _ => left += 1,
            }
        }
        if left > 0 {
            self.notes.push(format!(
                "{left} of the linked design's joints are not carried: a side can't be found, both sides are on what moves as one, or a side is on a second place of what a flexible occurrence's joints move"
            ));
        }
        self.nested = doc.flexible_at_any_depth().into_iter().filter(|o| placed.contains(o)).collect();
        let carried: BTreeSet<u64> = self.joints.iter().map(|j| j.id).collect();
        for l in &doc.assembly.links {
            if !carried.contains(&l.a) || !carried.contains(&l.b) {
                continue;
            }
            // What a ratio or a cam's table is worked out from is the linked design's geometry,
            // so both are carried as that design last worked them out, and Update Links reads
            // them again. A cam keeps its table and says that it is one.
            let cam = l.is_cam();
            self.links.push(MotionLink { coupling: None, lift: if cam { l.lift.clone() } else { Vec::new() }, carried_cam: cam, ..l.clone() });
        }
        self.fixed = solidcraft_doc::joints::solve(doc, st).fixed.iter().copied().filter(|o| placed.contains(o)).collect();
        self
    }
}

/// What [`sync`] found and did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Synced {
    /// Something in this design changed.
    pub changed: bool,
    /// The components the link now keeps below the linked one.
    pub components: usize,
    /// The joints of the linked design that each of its flexible occurrences has.
    pub joints: usize,
}

/// Rename component `id` as `component.rename` does: its placements still named after it
/// ("Old:2") follow.
fn rename(d: &mut Document, id: u64, name: &str) {
    let Some(c) = d.components.iter_mut().find(|c| c.id == id) else { return };
    let old = std::mem::replace(&mut c.name, name.to_string());
    for o in d.occurrences.iter_mut().filter(|o| o.component == id) {
        let k = o.name.strip_prefix(old.as_str()).and_then(|rest| rest.strip_prefix(':'));
        if let Some(k) = k.filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())) {
            o.name = format!("{name}:{k}");
        }
    }
}

/// The most components and occurrences of a linked design that a link keeps as a subassembly.
/// A linked design is not trusted, and each of them is work here at every Update Links.
const MAX_KEPT_COMPONENTS: usize = 5_000;
const MAX_KEPT_OCCURRENCES: usize = 20_000;
/// The longest name of a component or joint of a linked design that a link takes, in bytes: a
/// name is written again here for every occurrence it is made for.
const MAX_KEPT_NAME: usize = 512;

/// Names for the occurrences a link makes, as `Document::add_occurrence` gives them
/// (`<component>:<number>`), with the names in use looked up in a set. That function searches
/// the design for every number it tries, which a linked design with thousands of components of
/// one name would turn into their cube.
struct OccurrenceNames {
    taken: HashSet<String>,
    /// The number to try next, by the component's name.
    next: HashMap<String, usize>,
}

impl OccurrenceNames {
    fn of(d: &Document) -> Self {
        OccurrenceNames { taken: d.occurrences.iter().map(|o| o.name.clone()).collect(), next: HashMap::new() }
    }

    /// A name for one more occurrence of the component named `component`, of which the design
    /// has `placed` already.
    fn next(&mut self, component: &str, placed: usize) -> String {
        let mut k = self.next.get(component).copied().unwrap_or(1).max(placed.saturating_add(1));
        for _ in 0..=self.taken.len() {
            let name = format!("{component}:{k}");
            k = k.saturating_add(1);
            if self.taken.insert(name.clone()) {
                self.next.insert(component.to_string(), k);
                return name;
            }
        }
        format!("{component}:{k}")
    }
}

/// Whether the linked design's components place each other in a ring: such a design shows
/// nothing sound, and a link must not make its ring here.
fn rings(source: &SourceTree) -> bool {
    let mut inside: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for o in &source.occurrences {
        inside.entry(o.parent).or_default().push(o.component);
    }
    // Every component is reached from the design; one that comes back to itself rings.
    let (mut done, mut open) = (BTreeSet::new(), BTreeSet::new());
    let mut stack: Vec<(u64, usize)> = vec![(0, 0)];
    open.insert(0);
    while let Some((component, at)) = stack.pop() {
        let Some(next) = inside.get(&component).and_then(|children| children.get(at)).copied() else {
            open.remove(&component);
            done.insert(component);
            continue;
        };
        stack.push((component, at + 1));
        if open.contains(&next) {
            return true;
        }
        if !done.contains(&next) && open.insert(next) {
            stack.push((next, 0));
        }
    }
    false
}

/// Whether `source` has more components or occurrences than a link keeps.
pub(super) fn too_large(source: &SourceTree) -> bool {
    source.components.len() > MAX_KEPT_COMPONENTS || source.occurrences.len() > MAX_KEPT_OCCURRENCES
}

/// Whether this design can take `source` for its linked component `top`, kept as `keep` says:
/// an error says why not. Asked before a command changes anything of the link, and again by
/// [`sync`], so a link is changed as a whole or not at all.
pub(super) fn fits(s: &Session, top: u64, source: &SourceTree, keep: Keep) -> Result<()> {
    let top_name = s.doc.components.iter().find(|c| c.id == top).map(|c| c.name.clone()).unwrap_or_default();
    admit(&top_name, source)?;
    let kept = s.doc.links.get(&top).and_then(|link| link.tree.as_ref());
    // Each flexible occurrence gets every joint the design has to carry.
    let flexible = if source.joints.is_empty() { 0 } else { keep.flexible_in(&s.doc, top).len() };
    let recorded: BTreeSet<u64> = kept.iter().flat_map(|tree| tree.flexible.values()).flat_map(|joints| joints.values().copied()).collect();
    let others = s.doc.assembly.joints.iter().filter(|j| !recorded.contains(&j.id)).count();
    if others.saturating_add(flexible.saturating_mul(source.joints.len())) > solidcraft_doc::joints::MAX_JOINTS {
        return Err(EngineError::Other(format!(
            "{top_name}: {flexible} flexible occurrences with the {} joints of the linked design each are more joints than a design holds ({}): make fewer occurrences flexible",
            source.joints.len(),
            solidcraft_doc::joints::MAX_JOINTS
        )));
    }
    // A component the design no longer has goes, with every feature in it. A feature made
    // here in one is this design's work: it is not removed with the component.
    let still: BTreeSet<u64> = source.components.iter().map(|c| c.id).collect();
    let gone: BTreeSet<u64> =
        kept.iter().flat_map(|tree| tree.components.iter()).filter(|(design, _)| !still.contains(design)).map(|(_, here)| *here).collect();
    if !gone.is_empty() {
        let lost = s.doc.components_within(&gone);
        let made_here: Vec<&str> = s.doc.features.iter().filter(|f| lost.contains(&f.component)).map(|f| f.name.as_str()).take(4).collect();
        if !made_here.is_empty() {
            let more = if made_here.len() > 3 { " and more" } else { "" };
            let named = made_here.iter().take(3).map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ");
            return Err(EngineError::Other(format!(
                "{top_name}: the linked design no longer has a component that holds features made in this design ({named}{more}): move or delete them, then update again"
            )));
        }
        // A body moved out of such a component, or into it, is recorded in the timeline: the
        // component can't go while that record names it.
        let moved = |f: &solidcraft_doc::Feature| matches!(&f.kind, solidcraft_doc::FeatureKind::BodyTransfer { from, to, .. } if lost.contains(from) || lost.contains(to));
        if let Some(transfer) = s.doc.features.iter().find(|f| !lost.contains(&f.component) && moved(f)) {
            return Err(EngineError::Other(format!(
                "{top_name}: the linked design no longer has a component that `{}` moved a body out of or into: delete that step from the timeline, then update again",
                transfer.name
            )));
        }
    }
    // Room for what the design has gained.
    let (has_components, has_occurrences) = (kept.map_or(0, |tree| tree.components.len()), kept.map_or(0, |tree| tree.occurrences.len()));
    if s.doc.components.len().saturating_add(source.components.len().saturating_sub(has_components)) > 10_000
        || s.doc.occurrences.len().saturating_add(source.occurrences.len().saturating_sub(has_occurrences)) > 100_000
    {
        return Err(EngineError::Other(format!(
            "{top_name}: this design has no room for the linked design's components (10000 components, 100000 occurrences)"
        )));
    }
    Ok(())
}

/// Whether a link can keep `source` as a subassembly, whatever design links it: an error says
/// why not ([`fits`] asks this first).
pub(super) fn admit(top_name: &str, source: &SourceTree) -> Result<()> {
    if too_large(source) {
        return Err(EngineError::Other(format!(
            "{top_name}: the linked design has more components than a link keeps ({MAX_KEPT_COMPONENTS} components, {MAX_KEPT_OCCURRENCES} occurrences): link it with `structure: false`"
        )));
    }
    if source.components.iter().any(|c| c.name.len() > MAX_KEPT_NAME) || source.joints.iter().any(|j| j.name.len() > MAX_KEPT_NAME) {
        return Err(EngineError::Other(format!(
            "{top_name}: a component or a joint of the linked design has a name longer than {MAX_KEPT_NAME} bytes"
        )));
    }
    if rings(source) {
        return Err(EngineError::Other(format!("{top_name}: a component of the linked design contains itself")));
    }
    if source.occurrences.iter().any(|o| !solidcraft_doc::is_placement(&o.transform)) {
        return Err(EngineError::Other(format!("{top_name}: an occurrence in the linked design is not placed by a rigid motion within 1e9 mm")));
    }
    Ok(())
}

/// Make the components and occurrences linked component `top` keeps those of `source`, its
/// linked design, whose bodies the link's Import feature `feature` has just built (the design
/// is evaluated). With an empty `source` whatever the link kept goes, and the bodies are the
/// linked component's own again.
///
/// A component or occurrence the link already keeps for one the design still has stays the
/// same one here, and takes the design's name, part properties and transform. One the design
/// has gained is made, one it has lost is removed, with what referred to it; `warnings` names
/// the joints that go that way. The record of what stands for what ([`LinkTree`]) is written to
/// the link unless `keep` asks for no structure, which drops it. For each occurrence of the
/// linked component that is flexible once `keep` is done, the design's joints are made, written
/// again or removed in the same way, each keeping the values it has here; an occurrence made
/// rigid loses its joints and its pose.
pub(super) fn sync(s: &mut Session, top: u64, feature: u64, source: &SourceTree, keep: Keep, warnings: &mut Vec<String>) -> Result<Synced> {
    let top_name = s.doc.components.iter().find(|c| c.id == top).map(|c| c.name.clone()).unwrap_or_default();
    let fail = |e: solidcraft_doc::DocError| EngineError::Other(format!("{top_name}: {e}"));
    fits(s, top, source, keep)?;
    let kept = s.doc.links.get(&top).and_then(|link| link.tree.clone()).unwrap_or_default();
    // An occurrence is flexible by the joints it has from the linked design: a design without
    // any that can be carried leaves every occurrence rigid.
    let flexible = if source.joints.is_empty() { BTreeSet::new() } else { keep.flexible_in(&s.doc, top) };
    let recorded: BTreeSet<u64> = kept.flexible.values().flat_map(|joints| joints.values().copied()).collect();
    // A design without components, linked by a link that keeps none (a part): nothing to make
    // or to remove, and only whether the link keeps structure can change.
    let nothing_kept = kept.components.is_empty() && kept.occurrences.is_empty() && kept.flexible.is_empty();
    if source.is_empty() && source.joints.is_empty() && nothing_kept && flexible.is_empty() {
        let tree = keep.structure().then(LinkTree::default);
        let changed = s.doc.links.get(&top).is_some_and(|link| link.tree != tree);
        if changed && let Some(link) = s.doc_mut().links.get_mut(&top) {
            link.tree = tree;
        }
        return Ok(Synced { changed, ..Synced::default() });
    }
    let state = s.model.state();
    let mut d: Document = (*s.doc).clone();
    // A link that kept its design in one component and now keeps its components: the bodies
    // leave the linked component for the ones kept inside it. A joint side or a joint origin
    // found on a face or an edge of those bodies stays where it is, as a frame on the linked
    // component's occurrence, and is named so it can be picked again.
    if kept.components.is_empty() && !source.is_empty() {
        let on_top = |o: &JointOrigin| {
            o.inside.is_empty()
                && matches!(
                    o.snap,
                    solidcraft_doc::joints::Snap::FaceCenter { .. }
                        | solidcraft_doc::joints::Snap::CircleCenter { .. }
                        | solidcraft_doc::joints::Snap::Edge { .. }
                )
                && s.doc.occurrences.iter().any(|x| x.id == o.occurrence && x.component == top)
        };
        let frozen = |o: &JointOrigin| {
            let frame = solidcraft_doc::joints::resolve_origin(&s.doc, &state, o).ok()?;
            Some(JointOrigin::new(o.occurrence, solidcraft_doc::joints::frame_snap(&frame)))
        };
        let mut kept_in_place: Vec<String> = Vec::new();
        for j in &mut d.assembly.joints {
            let mut changed = false;
            for side in [&mut j.a, &mut j.b] {
                if on_top(side)
                    && let Some(frame) = frozen(side)
                {
                    *side = frame;
                    changed = true;
                }
            }
            if changed {
                kept_in_place.push(format!("joint {}", j.name));
            }
        }
        for origin in &mut d.assembly.origins {
            if on_top(&origin.origin)
                && let Some(frame) = frozen(&origin.origin)
            {
                origin.origin = frame;
                kept_in_place.push(format!("joint origin {}", origin.name));
            }
        }
        if !kept_in_place.is_empty() {
            warnings.push(format!(
                "{top_name}: {} stay where they are, as frames on its occurrence, and no longer follow the faces they were picked on: pick those again on the component that now has them",
                kept_in_place.join(", ")
            ));
        }
    }

    // What the record names is trusted only where it still is what the link made: a component
    // inside the linked one, an occurrence of such a component in it or in another such.
    let ours =
        |d: &Document, id: u64| id != top && !d.links.contains_key(&id) && d.components.iter().any(|c| c.id == id) && d.component_within(id, top);
    let was: BTreeSet<u64> = kept.components.values().copied().filter(|id| ours(&d, *id)).collect();
    let was_placed: BTreeSet<u64> = {
        let at: BTreeMap<u64, (u64, u64)> = d.occurrences.iter().map(|o| (o.id, (o.component, o.parent))).collect();
        kept.occurrences
            .values()
            .copied()
            .filter(|id| at.get(id).is_some_and(|(component, parent)| was.contains(component) && (*parent == top || was.contains(parent))))
            .collect()
    };

    // The components: the ones kept, and new ones for those the design has gained.
    let mut components: BTreeMap<u64, u64> = BTreeMap::new();
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    for c in &source.components {
        let here = kept.components.get(&c.id).copied().filter(|id| was.contains(id) && taken.insert(*id));
        let id = match here {
            Some(id) => id,
            None => {
                // Its occurrences are made below, where the design has them.
                let id = d.add_component_unplaced(Some(&c.name), top).map_err(fail)?;
                taken.insert(id);
                id
            }
        };
        components.insert(c.id, id);
    }
    let mut renamed = false;
    for c in &source.components {
        let Some(&id) = components.get(&c.id) else { continue };
        let parent = components.get(&c.parent).copied().filter(|p| *p != id).unwrap_or(top);
        let name = c.name.trim();
        if !name.is_empty() && d.components.iter().any(|x| x.id == id && x.name != name) {
            rename(&mut d, id, name);
            renamed = true;
        }
        if let Some(x) = d.components.iter_mut().find(|x| x.id == id) {
            x.metadata.clone_from(&c.metadata);
            x.properties.clone_from(&c.properties);
            x.parent = parent;
        }
    }
    // Every one of them is inside the linked component, so it goes when that one is deleted: a
    // design whose own components do not lead back to it hangs them from the linked one.
    for id in components.values() {
        if !d.component_within(*id, top)
            && let Some(x) = d.components.iter_mut().find(|x| x.id == *id)
        {
            x.parent = top;
        }
    }

    // The occurrences, each where the design has it.
    let mut occurrences: BTreeMap<u64, u64> = BTreeMap::new();
    let mut placed: BTreeSet<u64> = BTreeSet::new();
    let mut names = OccurrenceNames::of(&d);
    let named: HashMap<u64, String> = d.components.iter().map(|c| (c.id, c.name.clone())).collect();
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for o in &d.occurrences {
        *counts.entry(o.component).or_default() += 1;
    }
    for o in &source.occurrences {
        let parent = if o.parent == 0 { Some(top) } else { components.get(&o.parent).copied() };
        let (Some(&component), Some(parent)) = (components.get(&o.component), parent) else { continue };
        if !solidcraft_doc::is_placement(&o.transform) {
            return Err(EngineError::Other(format!("{top_name}: an occurrence in the linked design is not placed by a rigid motion within 1e9 mm")));
        }
        let here = kept
            .occurrences
            .get(&o.id)
            .copied()
            .filter(|id| was_placed.contains(id) && d.occurrences.iter().any(|x| x.id == *id && x.component == component && x.parent == parent))
            .filter(|id| placed.insert(*id));
        let id = match here {
            Some(id) => {
                if let Some(x) = d.occurrences.iter_mut().find(|x| x.id == id) {
                    x.transform = o.transform;
                    // Grounded, it would hold the whole subassembly to the world.
                    x.grounded = false;
                }
                id
            }
            None => {
                if d.occurrences.len() >= 100_000 {
                    return Err(EngineError::Other(format!("{top_name}: too many occurrences")));
                }
                let id = d.allocate_id().map_err(fail)?;
                let count = counts.entry(component).or_default();
                let name = names.next(named.get(&component).map_or("", String::as_str), *count);
                *count += 1;
                d.occurrences.push(solidcraft_doc::Occurrence { id, component, parent, name, transform: o.transform, grounded: false });
                placed.insert(id);
                id
            }
        };
        occurrences.insert(o.id, id);
    }

    // What the design no longer has goes, with what referred to it. The joints its flexible
    // occurrences have from it are its own: they are written again below, not lost. No feature
    // made here goes this way: a component that holds one is not let go ([`fits`]).
    let lost: BTreeSet<u64> = was.difference(&taken).copied().collect();
    let lost = d.components_within(&lost);
    let gone_placed: BTreeSet<u64> = was_placed.difference(&placed).copied().collect();
    let mut gone_joints = d.remove_occurrences(&gone_placed);
    if !lost.is_empty() {
        gone_joints.extend(d.remove_component_trees(&lost).map_err(fail)?.joints);
    }
    for j in s.doc.assembly.joints.iter().filter(|j| gone_joints.contains(&j.id) && !recorded.contains(&j.id)) {
        warnings.push(format!("joint {} went with the component it was on, which {top_name}'s linked design no longer has", j.name));
    }

    // Each body belongs to the component that stands for its own in the design. The feature
    // keeps its bodies' names by the file's body index, which is the order of `source.bodies`:
    // a body that a later feature here removes or takes up shifts no other body's component,
    // and a feature that is suppressed or rolled back keeps its bodies' components.
    let built: Vec<&str> = state.bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.as_str()).collect();
    let stored: Vec<String> = d.feature(feature).map(|f| f.body_names.clone()).unwrap_or_default();
    let named: BTreeSet<&str> = stored.iter().map(String::as_str).filter(|name| !name.is_empty()).collect();
    let by_slot: Vec<(String, usize)> = if stored.len() >= source.bodies.len() && !named.is_empty() && built.iter().all(|name| named.contains(name)) {
        stored.iter().enumerate().filter(|(_, name)| !name.is_empty()).map(|(slot, name)| (name.clone(), slot)).collect()
    } else {
        // Names the feature has not kept: the bodies it builds now, in the order it builds them.
        let slots = d.feature(feature).map(|f| f.body_name_slots(built.len())).unwrap_or_default();
        built.iter().map(|name| (*name).to_string()).zip(slots).collect()
    };
    let linked: BTreeSet<&str> = built.iter().copied().chain(by_slot.iter().map(|(name, _)| name.as_str())).collect();
    let all: BTreeSet<u64> = was.union(&taken).copied().collect();
    // A body the feature builds no more leaves no stale assignment behind.
    let stale = |name: &str, component: u64| linked.contains(name) || (all.contains(&component) && state.body(name).is_none());
    let dropped: Vec<String> = d.body_components.iter().filter(|(name, c)| stale(name.as_str(), **c)).map(|(name, _)| name.clone()).collect();
    for name in dropped.iter().map(String::as_str).chain(linked.iter().copied()) {
        d.body_components.remove(name);
        d.body_offsets.remove(name);
    }
    for (name, slot) in &by_slot {
        let Some((component, offset)) = source.bodies.get(*slot) else { continue };
        let Some(&component) = components.get(component) else { continue };
        d.body_components.insert(name.clone(), component);
        if !solidcraft_doc::is_identity(offset) {
            d.body_offsets.insert(name.clone(), *offset);
        }
    }

    // Each flexible occurrence of the linked component has the design's joints for itself. A
    // side is that occurrence and, inside it, the occurrences kept for the design's down to the
    // one the side is on, through the first occurrence of each component on the way
    // (`solidcraft_doc::joints::placed`). A side on the design's own bodies is on the
    // occurrence itself.
    let at: HashMap<u64, (u64, u64)> = d.occurrences.iter().map(|o| (o.id, (o.component, o.parent))).collect();
    let mut first: HashMap<u64, u64> = HashMap::new();
    for o in &d.occurrences {
        first.entry(o.component).or_insert(o.id);
    }
    let way_down = |inner: u64| -> Option<Vec<u64>> {
        let mut way = Vec::new();
        let mut cur = inner;
        for _ in 0..=solidcraft_doc::MAX_PLACEMENT_DEPTH {
            let (_, parent) = *at.get(&cur)?;
            way.push(cur);
            if parent == top {
                way.reverse();
                return Some(way);
            }
            cur = *first.get(&parent).filter(|_| parent != 0)?;
        }
        None
    };
    // A side on a copy inside a flexible occurrence of the design (`inside` there) goes on down
    // through the occurrences kept for those.
    let side = |occurrence: u64, o: &JointOrigin| -> Option<JointOrigin> {
        let mut inside = if o.occurrence == 0 { Vec::new() } else { way_down(*occurrences.get(&o.occurrence)?)? };
        for id in &o.inside {
            inside.push(*occurrences.get(id)?);
        }
        (inside.len() <= solidcraft_doc::MAX_PLACEMENT_DEPTH).then_some(JointOrigin { occurrence, inside, snap: o.snap.clone() })
    };
    let fixed: BTreeSet<u64> =
        if flexible.is_empty() { BTreeSet::new() } else { source.fixed.iter().filter_map(|o| occurrences.get(o).copied()).collect() };
    // The occurrences kept for ones that are flexible in the design: what a copy can be under.
    let nested: BTreeSet<u64> =
        if flexible.is_empty() { BTreeSet::new() } else { source.nested.iter().filter_map(|o| occurrences.get(o).copied()).collect() };
    let tops: BTreeMap<u64, String> = d.occurrences.iter().filter(|o| o.component == top).map(|o| (o.id, o.name.clone())).collect();
    let mut joint_names = JointNames::of(&d);
    let mut joints: BTreeMap<u64, BTreeMap<u64, u64>> = BTreeMap::new();
    let mut carried: BTreeSet<u64> = BTreeSet::new();
    // Where each joint is in the list, which only grows from here on.
    let mut joint_at: HashMap<u64, usize> = d.assembly.joints.iter().enumerate().map(|(at, j)| (j.id, at)).collect();
    // The copies each flexible occurrence's joints move: the ones that keep a pose.
    let mut posed: HashSet<(u64, Vec<u64>, u64)> = HashSet::new();
    let mut full = false;
    for occurrence in &flexible {
        if full {
            break;
        }
        let before = kept.flexible.get(occurrence);
        let mut mine: BTreeMap<u64, u64> = BTreeMap::new();
        for j in &source.joints {
            let (Some(a), Some(b)) = (side(*occurrence, &j.a), side(*occurrence, &j.b)) else { continue };
            let moved: Vec<(u64, Vec<u64>, u64)> = [&a, &b]
                .into_iter()
                .filter_map(|x| x.inside.split_last())
                .filter(|(inner, _)| !fixed.contains(inner))
                .map(|(inner, above)| (*occurrence, above.iter().copied().filter(|id| nested.contains(id)).collect(), *inner))
                .collect();
            // A recorded joint is this occurrence's only when its sides are on it: a record
            // that names another joint does not get that one rewritten.
            let known = before.and_then(|joints| joints.get(&j.id)).copied().filter(|id| !carried.contains(id));
            let here = known
                .and_then(|id| joint_at.get(&id).copied())
                .filter(|at| d.assembly.joints.get(*at).is_some_and(|x| x.a.occurrence == *occurrence && x.b.occurrence == *occurrence));
            let id = match here {
                Some(at) => {
                    let Some(here) = d.assembly.joints.get_mut(at) else { continue };
                    // The joint is the design's; where it stands, its name and whether it is
                    // suppressed are this design's.
                    let mut values = if here.kind == j.kind && here.values.len() == j.values.len() { here.values.clone() } else { j.values.clone() };
                    solidcraft_doc::joints::clamp(j, &mut values);
                    let captured = here.captured.clone();
                    *here = Joint { id: here.id, name: here.name.clone(), suppressed: here.suppressed, captured, a, b, values, ..j.clone() };
                    here.id
                }
                None if d.assembly.joints.len() < solidcraft_doc::joints::MAX_JOINTS => {
                    let id = d.allocate_id().map_err(fail)?;
                    let name = joint_names.next(tops.get(occurrence).map_or(top_name.as_str(), String::as_str), &j.name);
                    joint_at.insert(id, d.assembly.joints.len());
                    d.assembly.joints.push(Joint { id, name, a, b, ..j.clone() });
                    id
                }
                None => {
                    full = true;
                    break;
                }
            };
            mine.insert(j.id, id);
            carried.insert(id);
            posed.extend(moved);
        }
        joints.insert(*occurrence, mine);
    }
    if full {
        warnings.push(format!("{top_name}: this design has no room for all of the linked design's joints"));
    }
    if !flexible.is_empty() {
        warnings.extend(source.notes.iter().cloned());
    }
    // A joint the design no longer has, or of an occurrence that is rigid again, goes: one the
    // record names that is on an occurrence of the linked component, and no other.
    let dropped: BTreeSet<u64> = d
        .assembly
        .joints
        .iter()
        .filter(|j| recorded.contains(&j.id) && !carried.contains(&j.id) && tops.contains_key(&j.a.occurrence) && j.a.occurrence == j.b.occurrence)
        .map(|j| j.id)
        .collect();
    gone_joints.extend(dropped.iter().copied());
    remove_joints(&mut d, &dropped);
    // The motion links between an occurrence's joints are the design's. One made here between
    // the joints of two occurrences is this design's, and stays.
    for mine in joints.values() {
        let own: BTreeSet<u64> = mine.values().copied().collect();
        d.assembly.links.retain(|l| !(own.contains(&l.a) && own.contains(&l.b)));
        for l in &source.links {
            if let (Some(a), Some(b)) = (mine.get(&l.a), mine.get(&l.b)) {
                d.assembly.links.push(MotionLink { a: *a, b: *b, ..l.clone() });
            }
        }
    }
    // A rigid occurrence shows the design as it was saved, and a flexible one keeps a pose only
    // for what its joints still move: an occurrence whose joint the design has dropped, or which
    // the design now holds still, is where the design has it again. The poses the design has
    // for the flexible subassemblies inside it are kept as it has them now, for the occurrences
    // that stand for its own, so those show here as it shows them.
    let ours: HashSet<u64> = occurrences.values().copied().collect();
    d.assembly.poses.retain(|p| {
        !ours.contains(&p.occurrence) && (!tops.contains_key(&p.occurrence) || posed.contains(&(p.occurrence, p.through.clone(), p.inner)))
    });
    for (occurrence, through, inner, transform) in &source.poses {
        let through: Option<Vec<u64>> = through.iter().map(|under| occurrences.get(under).copied()).collect();
        if let (Some(&occurrence), Some(through), Some(&inner)) = (occurrences.get(occurrence), through, occurrences.get(inner))
            && solidcraft_doc::is_placement(transform)
        {
            d.assembly.poses.push(solidcraft_doc::joints::PlacedPose { occurrence, through, inner, transform: *transform });
        }
    }
    d.assembly.order_poses();
    let carried_joints = joints.values().map(BTreeMap::len).max().unwrap_or(0);
    let tree = keep.structure().then(|| LinkTree { components: components.clone(), occurrences, flexible: joints, fixed, nested });
    if let Some(link) = d.links.get_mut(&top) {
        link.tree = tree;
    }
    if renamed {
        // A sketch that projects a body of one of the renamed occurrences keeps it.
        let placed_before = s.doc.body_frames_with_source(&state);
        super::super::sketch_project::follow_placed_renames(&mut d, &state, &placed_before, None);
    }
    let changed = d.components != s.doc.components
        || d.occurrences != s.doc.occurrences
        || d.assembly != s.doc.assembly
        || d.body_components != s.doc.body_components
        || d.body_offsets != s.doc.body_offsets
        || d.links != s.doc.links
        || (renamed && d.features != s.doc.features);
    if changed {
        let failing: BTreeSet<u64> = s.model.results.iter().filter(|r| r.error.is_some()).map(|r| r.id).collect();
        s.set_doc(d);
        s.visibility.hidden_joints.retain(|h| !gone_joints.contains(h));
        s.refresh();
        // A feature of this design that read the linked bodies and now fails says so here.
        for r in s.model.results.iter().filter(|r| !failing.contains(&r.id)) {
            if let Some(e) = &r.error {
                warnings.push(format!("{top_name}: feature {} now fails: {e}", r.name));
            }
        }
    }
    Ok(Synced { changed, components: components.len(), joints: carried_joints })
}

/// Names for the joints carried from a linked design: a joint's own where no joint here has
/// it, else with the name of the occurrence it is for in front (`hinge:2: Hinge`), and a number
/// behind when that is in use too.
struct JointNames {
    taken: HashSet<String>,
    /// The number to try next behind a name.
    next: HashMap<String, usize>,
}

impl JointNames {
    fn of(d: &Document) -> Self {
        JointNames { taken: d.assembly.joints.iter().map(|j| j.name.clone()).collect(), next: HashMap::new() }
    }

    /// A name for the joint named `name` in the linked design, carried for occurrence `top`.
    fn next(&mut self, top: &str, name: &str) -> String {
        if self.taken.insert(name.to_string()) {
            return name.to_string();
        }
        let base = format!("{top}: {name}");
        if self.taken.insert(base.clone()) {
            return base;
        }
        let mut k = self.next.get(&base).copied().unwrap_or(2);
        for _ in 0..=self.taken.len() {
            let candidate = format!("{base} ({k})");
            k = k.saturating_add(1);
            if self.taken.insert(candidate.clone()) {
                self.next.insert(base, k);
                return candidate;
            }
        }
        base
    }
}

/// Remove joints, with the motion links and motion study keys that name them and their places
/// in history folders.
fn remove_joints(d: &mut Document, gone: &BTreeSet<u64>) {
    if gone.is_empty() {
        return;
    }
    d.assembly.joints.retain(|j| !gone.contains(&j.id));
    d.assembly.links.retain(|l| !gone.contains(&l.a) && !gone.contains(&l.b));
    for study in &mut d.assembly.studies {
        study.keys.retain(|k| !gone.contains(&k.joint));
    }
    for group in &mut d.timeline_groups {
        group.joints.retain(|id| !gone.contains(id));
    }
}

/// Occurrence `to` of a linked component was made as a copy of its occurrence `from`: when
/// `from` is flexible, `to` is too, with joints of its own at the same values and its parts in
/// the same pose. Nothing for a rigid `from`, or one that is no occurrence of a linked subassembly.
pub(super) fn copy_placement(d: &mut Document, from: u64, to: u64) -> Result<()> {
    let at = |d: &Document, id: u64| d.occurrences.iter().find(|o| o.id == id).map(|o| (o.component, o.name.clone()));
    let (Some((top, from_name)), Some((to_top, to_name))) = (at(d, from), at(d, to)) else { return Ok(()) };
    let Some(mine) = d.links.get(&top).and_then(|link| link.tree.as_ref()).and_then(|tree| tree.flexible.get(&from)).cloned() else { return Ok(()) };
    if top != to_top || from == to {
        return Ok(());
    }
    let fail = |e: solidcraft_doc::DocError| EngineError::Other(format!("{to_name}: {e}"));
    let moved = |side: &JointOrigin| JointOrigin { occurrence: if side.occurrence == from { to } else { side.occurrence }, ..side.clone() };
    let mut made: BTreeMap<u64, u64> = BTreeMap::new();
    let mut theirs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut names = JointNames::of(d);
    // The copy is flexible with every joint or not at all.
    if d.assembly.joints.len().saturating_add(mine.len()) > solidcraft_doc::joints::MAX_JOINTS {
        return Err(EngineError::Other(format!(
            "{to_name}: the design has no room for the joints of a flexible copy (at most {} joints): make `{from_name}` rigid first",
            solidcraft_doc::joints::MAX_JOINTS
        )));
    }
    let joint_at: HashMap<u64, usize> = d.assembly.joints.iter().enumerate().map(|(at, j)| (j.id, at)).collect();
    for (design, joint) in &mine {
        let Some(j) = joint_at.get(joint).and_then(|at| d.assembly.joints.get(*at)).cloned() else { continue };
        // The design's own name for it: `from`'s joint has it bare or behind `from`'s name.
        let own = j.name.strip_prefix(&format!("{from_name}: ")).unwrap_or(&j.name).to_string();
        let id = d.allocate_id().map_err(fail)?;
        let name = names.next(&to_name, &own);
        d.assembly.joints.push(Joint { id, name, a: moved(&j.a), b: moved(&j.b), ..j });
        made.insert(*design, id);
        theirs.insert(*joint, id);
    }
    let links: Vec<MotionLink> =
        d.assembly.links.iter().filter_map(|l| Some(MotionLink { a: *theirs.get(&l.a)?, b: *theirs.get(&l.b)?, ..l.clone() })).collect();
    d.assembly.links.extend(links);
    let poses: Vec<solidcraft_doc::joints::PlacedPose> = d
        .assembly
        .poses
        .iter()
        .filter(|p| p.occurrence == from)
        .map(|p| solidcraft_doc::joints::PlacedPose { occurrence: to, ..p.clone() })
        .collect();
    d.assembly.poses.retain(|p| p.occurrence != to);
    d.assembly.poses.extend(poses);
    d.assembly.order_poses();
    if let Some(tree) = d.links.get_mut(&top).and_then(|link| link.tree.as_mut()) {
        tree.flexible.insert(to, made);
    }
    Ok(())
}

/// Occurrence `occurrence` no longer shows linked component `top`: the joints and the pose it
/// had there for itself go. Returns the joints removed.
pub(super) fn forget_placement(d: &mut Document, top: u64, occurrence: u64) -> BTreeSet<u64> {
    let tree = d.links.get_mut(&top).and_then(|link| link.tree.as_mut());
    let mine = tree.and_then(|tree| {
        let mine = tree.flexible.remove(&occurrence);
        // What the linked design holds still is kept for the flexible occurrences only.
        if tree.flexible.is_empty() {
            tree.fixed.clear();
        }
        mine
    });
    let gone: BTreeSet<u64> = mine.into_iter().flat_map(BTreeMap::into_values).collect();
    remove_joints(d, &gone);
    d.assembly.poses.retain(|p| p.occurrence != occurrence);
    gone
}
