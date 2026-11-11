//! Linked subassemblies in the joint solver, and poses of their own for flexible ones (#5526,
//! #1516).
//!
//! A link that keeps its linked design's components ([`LinkTree`]) makes the linked component a
//! subassembly. The occurrences inside a component are shared by every occurrence of that
//! component, so an inner occurrence has one transform wherever its component is placed. Each
//! occurrence of a subassembly is rigid or flexible:
//!
//! - **Rigid:** the occurrences inside it stay where the linked design has them and it moves as
//!   one. A joint side on one of them moves the whole occurrence.
//! - **Flexible:** the linked design's joints are joints of this design too, once for each
//!   flexible occurrence (`LinkTree::flexible`). A side of such a joint is `{occurrence, inside}`:
//!   that occurrence's own copy of the inner occurrence. Where a copy stands is a [`PlacedPose`]
//!   (`Assembly::poses`), so two occurrences of one subassembly can stand in different poses.
//!   The inner occurrence's own transform stays where the linked design has it.
//!
//! [`expand`] gives the solver the design's joints with every side on an occurrence inside a
//! subassembly as a frame on what the solver moves: a copy that a flexible occurrence's joints
//! move, which has a place of its own in the solve, or the subassembly's occurrence itself when
//! its joints do not move that one. [`Expanded::fold`] takes the solver's answer for the copies
//! back as [`Solution::placed`], which [`Document::apply_joint_solution`] keeps as poses.
//!
//! A pose can also be the linked design's own: a link keeps the poses its linked design has for
//! the flexible subassemblies inside it, so those show here as that design shows them.
//!
//! Flexibility goes down through every level that is flexible. A flexible occurrence also has
//! the joints its linked design has for the flexible occurrences inside it (`LinkTree::nested`),
//! and so on down. A copy is then one inner occurrence under the flexible occurrences on the way
//! to it ([`PlacedPose::through`]): two hinges inside one unit, and the hinges of two units, each
//! stand in a pose of their own. A rigid occurrence anywhere on the way shows what is below it
//! as its linked design was saved.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

use super::{Assembly, Joint, JointOrigin, MAX_JOINTS, Snap, Solution, Start, mismatch, resolve_on_part};
use crate::{
    Document, IDENTITY, LinkTree, MAX_PLACEMENT_DEPTH, Mat, ModelState, Occurrence, apply_point, apply_vector, is_placement, mat_inverse, mat_mul,
};

/// Where one flexible occurrence of a linked subassembly has an occurrence inside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedPose {
    /// The occurrence this pose is for: a flexible occurrence of a linked component, or an
    /// occurrence a link keeps for one that is flexible in its linked design (the pose is then
    /// that design's, as it was saved).
    pub occurrence: u64,
    /// The flexible occurrences inside `occurrence` that the copy is under, from the outermost
    /// down: occurrences the link keeps for ones that are flexible in its linked design. Two
    /// such occurrences of one subassembly have a copy each of what is inside them. Empty for
    /// an inner occurrence that is under none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<u64>,
    /// The occurrence inside it that is posed: one the link keeps for the linked design's own.
    /// Like its own transform, the pose holds wherever that occurrence shows below `occurrence`
    /// and the occurrences of `through`.
    pub inner: u64,
    /// The inner occurrence's transform in its parent, for `occurrence`.
    pub transform: Mat,
}

/// Where the world has one copy of an inner occurrence: the occurrence it is a copy for, the
/// flexible occurrences inside that one which it is under ([`PlacedPose::through`]), the inner
/// occurrence, and the copy's world transform.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedCopy {
    pub occurrence: u64,
    pub through: Vec<u64>,
    pub inner: u64,
    pub world: Mat,
}

/// Whether the occurrences `through` come, in their order, among `path`.
fn follows(through: &[u64], path: &[u64]) -> bool {
    let mut rest = path.iter();
    through.iter().all(|id| rest.any(|on_path| on_path == id))
}

/// The occurrences of `above` that are flexible in the linked design of `tree`
/// ([`LinkTree::nested`]): what a copy below them is under.
fn context(tree: &LinkTree, above: &[u64]) -> Vec<u64> {
    above.iter().copied().filter(|id| tree.nested.contains(id)).collect()
}

/// One value for each copy: by the occurrence it is a copy for and the inner occurrence, then
/// by what the copy is under.
pub(crate) struct ByCopy<T> {
    by: HashMap<(u64, u64), Vec<(Vec<u64>, T)>>,
}

impl<T: Copy> ByCopy<T> {
    fn new() -> Self {
        ByCopy { by: HashMap::new() }
    }

    fn insert(&mut self, occurrence: u64, through: Vec<u64>, inner: u64, value: T) {
        self.by.entry((occurrence, inner)).or_default().push((through, value));
    }

    fn get(&self, occurrence: u64, through: &[u64], inner: u64) -> Option<T> {
        self.by.get(&(occurrence, inner))?.iter().find(|(under, _)| under.as_slice() == through).map(|(_, value)| *value)
    }

    /// Whether `occurrence` has a copy of `inner`, under anything.
    fn any(&self, occurrence: u64, inner: u64) -> bool {
        self.by.contains_key(&(occurrence, inner))
    }
}

/// What tells one pose from another, in the order the poses are kept and searched in: the
/// occurrence it is for, then the inner occurrence, then what the copy is under.
fn copy_of(pose: &PlacedPose) -> (u64, u64, &[u64]) {
    (pose.occurrence, pose.inner, pose.through.as_slice())
}

impl Assembly {
    /// Put the poses in order, by occurrence, then inner occurrence, then what the copy is
    /// under, one for each copy: lookups search them in that order.
    pub fn order_poses(&mut self) {
        self.poses.sort_by(|a, b| copy_of(a).cmp(&copy_of(b)));
        self.poses.dedup_by(|a, b| copy_of(a) == copy_of(b));
    }

    /// `poses` in that order: the list itself when it is in it already, as a design's own is,
    /// else a copy put in it. For poses shown in place of a design's own, whoever listed them
    /// ([`Document::posed_local_among`]).
    pub fn poses_in_order(poses: &[PlacedPose]) -> std::borrow::Cow<'_, [PlacedPose]> {
        if poses.is_sorted_by(|a, b| copy_of(a) < copy_of(b)) {
            return std::borrow::Cow::Borrowed(poses);
        }
        let mut sorted = Assembly { poses: poses.to_vec(), ..Assembly::default() };
        sorted.order_poses();
        std::borrow::Cow::Owned(sorted.poses)
    }
}

/// A design's [`PlacedPose`]s, which `Assembly::poses` keeps in order ([`Assembly::order_poses`]).
pub(crate) struct Poses<'a> {
    all: &'a [PlacedPose],
}

impl<'a> Poses<'a> {
    /// `None` for a design that has none, which is most designs: they pay nothing.
    pub(crate) fn of(doc: &'a Document) -> Option<Self> {
        (!doc.assembly.poses.is_empty()).then_some(Poses { all: &doc.assembly.poses })
    }

    /// The poses `occurrence` has for the occurrences inside it.
    fn of_occurrence(&self, occurrence: u64) -> &'a [PlacedPose] {
        let from = self.all.partition_point(|p| p.occurrence < occurrence);
        let to = self.all.partition_point(|p| p.occurrence <= occurrence);
        self.all.get(from..to).unwrap_or_default()
    }

    /// The transform of `o`, the last occurrence of `path`, in its parent: the one an
    /// occurrence above it on the path has for the copy the path shows, else its own. The
    /// outermost such occurrence says: a design's own pose for a copy comes before the one a
    /// linked design below was saved with. Of one occurrence's poses, the one whose copy is
    /// under the most occurrences of the path. `path` is occurrence ids from any ancestor down
    /// to `o`.
    pub(crate) fn local(&self, path: &[u64], o: &Occurrence) -> Mat {
        let above = path.len().saturating_sub(1);
        for i in 0..above {
            let mine = path.get(i).map(|id| self.of_occurrence(*id)).unwrap_or_default();
            if mine.is_empty() {
                continue;
            }
            let from = mine.partition_point(|p| p.inner < o.id);
            let between = path.get(i + 1..above).unwrap_or_default();
            let of_inner = mine.get(from..).unwrap_or_default().iter().take_while(|p| p.inner == o.id);
            if let Some(pose) = of_inner.filter(|p| follows(&p.through, between)).max_by_key(|p| p.through.len()) {
                return pose.transform;
            }
        }
        o.transform
    }

    /// The world transform at the end of `chain`: occurrences from the root down, each placed in
    /// the component of the one before.
    pub(crate) fn along(&self, chain: &[&Occurrence]) -> Mat {
        let ids: Vec<u64> = chain.iter().map(|o| o.id).collect();
        let mut m = IDENTITY;
        for (i, o) in chain.iter().enumerate() {
            m = mat_mul(&m, &self.local(ids.get(..=i).unwrap_or_default(), o));
        }
        m
    }
}

/// `occurrence` and the first occurrences of the components above it, from the root down: the
/// placement [`occurrence_world`] gives it.
pub(crate) fn first_chain(doc: &Document, occurrence: u64) -> Vec<&Occurrence> {
    let mut chain = Vec::new();
    let mut cur = doc.occurrences.iter().find(|o| o.id == occurrence);
    for _ in 0..=MAX_PLACEMENT_DEPTH {
        let Some(o) = cur else { break };
        chain.push(o);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    chain.reverse();
    chain
}

/// A design's occurrences by id, and each component's first: what a walk of many placements
/// needs looked up, where searching the design for each would cost the design every time.
struct Index<'a> {
    at: HashMap<u64, &'a Occurrence>,
    first: HashMap<u64, &'a Occurrence>,
}

impl<'a> Index<'a> {
    fn of(doc: &'a Document) -> Self {
        let (mut at, mut first) = (HashMap::new(), HashMap::new());
        for o in &doc.occurrences {
            at.entry(o.id).or_insert(o);
            first.entry(o.component).or_insert(o);
        }
        Index { at, first }
    }

    /// [`first_chain`], looked up.
    fn chain(&self, occurrence: u64) -> Vec<&'a Occurrence> {
        let mut chain = Vec::new();
        let mut cur = self.at.get(&occurrence).copied();
        for _ in 0..=MAX_PLACEMENT_DEPTH {
            let Some(o) = cur else { break };
            chain.push(o);
            if o.parent == 0 {
                break;
            }
            cur = self.first.get(&o.parent).copied();
        }
        chain.reverse();
        chain
    }

    /// The occurrences from the one placed in `top`'s component down to the copy of `inner`
    /// that is under the occurrences `through`: a component one of those shows is reached
    /// through that one, any other through its first occurrence, which is the way a joint of
    /// the linked design reaches it. `None` when `inner` is not inside `top`.
    fn way_through(&self, top: &Occurrence, through: &[u64], inner: u64) -> Option<Vec<u64>> {
        let mut way = Vec::new();
        let mut cur = self.at.get(&inner).copied()?;
        for _ in 0..=MAX_PLACEMENT_DEPTH {
            way.push(cur.id);
            if cur.parent == top.component {
                way.reverse();
                return Some(way);
            }
            if cur.parent == 0 {
                return None;
            }
            let named = through.iter().filter_map(|id| self.at.get(id).copied()).find(|o| o.component == cur.parent);
            cur = match named {
                Some(o) => o,
                None => self.first.get(&cur.parent).copied()?,
            };
        }
        None
    }

    /// The transform from the frame of `inside[from - 1]` (of `top` itself for 0) down to the
    /// last of `inside`, as `top` has those occurrences. `None` when they do not lead from one
    /// to the next.
    fn down(&self, poses: Option<&Poses>, top: &Occurrence, inside: &[u64], from: usize) -> Option<Mat> {
        let mut component = match from.checked_sub(1) {
            Some(i) => self.at.get(inside.get(i)?)?.component,
            None => top.component,
        };
        let mut path: Vec<u64> = std::iter::once(top.id).chain(inside.get(..from)?.iter().copied()).collect();
        let mut m = IDENTITY;
        for id in inside.get(from..)? {
            let x = self.at.get(id).filter(|x| x.parent == component)?;
            path.push(*id);
            m = mat_mul(&m, &poses.map_or(x.transform, |p| p.local(&path, x)));
            component = x.component;
        }
        Some(m)
    }

    /// World transform of occurrence `occurrence`, as the world shows it.
    fn world(&self, poses: Option<&Poses>, occurrence: u64) -> Mat {
        let chain = self.chain(occurrence);
        match poses {
            Some(poses) => poses.along(&chain),
            None => chain.iter().fold(IDENTITY, |m, o| mat_mul(&m, &o.transform)),
        }
    }
}

impl Document {
    /// A joint side that names an occurrence inside a linked subassembly by that occurrence
    /// alone, as a side on the subassembly's occurrence and the occurrences inside it down to
    /// that one. The occurrence inside is the same in every occurrence of the subassembly:
    /// named alone it stands for the first one's, and would come to mean another's when that
    /// one is deleted. Any other side comes back as it is.
    pub fn through_subassembly(&self, side: &JointOrigin) -> JointOrigin {
        let structured =
            |component: u64| self.links.get(&component).and_then(|link| link.tree.as_ref()).is_some_and(|tree| !tree.occurrences.is_empty());
        if !self.links.values().any(|link| link.tree.as_ref().is_some_and(|tree| !tree.occurrences.is_empty())) {
            return side.clone();
        }
        let chain = first_chain(self, side.occurrence);
        let top = chain.iter().position(|o| structured(o.component));
        let Some((top, below)) = top.and_then(|i| Some((chain.get(i)?, chain.get(i + 1..)?))) else { return side.clone() };
        let inside: Vec<u64> = below.iter().map(|o| o.id).chain(side.inside.iter().copied()).collect();
        if below.is_empty() || inside.len() > MAX_PLACEMENT_DEPTH {
            return side.clone();
        }
        JointOrigin { occurrence: top.id, inside, snap: side.snap.clone() }
    }

    /// The transform of `o`, the last occurrence of `path`, in its parent, as the world shows
    /// it along that path: the one an occurrence above it on the path has for its own copy of
    /// it (a pose), else its own. `path` is occurrence ids from any ancestor down to `o`. For
    /// walks of placements made outside this crate; [`Document::placements`] uses the same.
    pub fn posed_local(&self, path: &[u64], o: &Occurrence) -> Mat {
        Self::posed_local_among(&self.assembly.poses, path, o)
    }

    /// [`Self::posed_local`] among `poses` in place of the design's own, for a walk that shows
    /// poses the design does not hold: a previewed motion's. They are searched in the order
    /// [`Assembly::order_poses`] puts them, as a design's own are ([`Assembly::poses_in_order`]).
    pub fn posed_local_among(poses: &[PlacedPose], path: &[u64], o: &Occurrence) -> Mat {
        if poses.is_empty() { o.transform } else { Poses { all: poses }.local(path, o) }
    }

    /// Whether the world shows the placement `path` (occurrence ids from the root down) elsewhere
    /// than the design itself has it: an occurrence on it has a pose that is not where its
    /// linked design was saved. A feature reads the design, not the pose, so a pick made on such
    /// a placement is not where the feature would find it.
    pub fn path_is_posed(&self, path: &[u64]) -> bool {
        let Some(poses) = Poses::of(self) else { return false };
        (1..path.len().min(MAX_PLACEMENT_DEPTH + 1)).any(|at| {
            let Some(o) = path.get(at).and_then(|id| self.occurrences.iter().find(|o| o.id == *id)) else { return false };
            let (dt, da) = mismatch(&poses.local(path.get(..=at).unwrap_or_default(), o), &o.transform);
            dt > 1e-9 || da > 1e-10
        })
    }

    /// World transform of every copy that has a pose, where the world shows it: below the first
    /// occurrence of each component above the pose's occurrence, as `occurrence_world` places
    /// an occurrence.
    pub fn posed_worlds(&self) -> Vec<PlacedCopy> {
        let Some(poses) = Poses::of(self) else { return Vec::new() };
        let index = Index::of(self);
        let mut out = Vec::with_capacity(self.assembly.poses.len());
        for pose in &self.assembly.poses {
            let Some(top) = index.at.get(&pose.occurrence).copied() else { continue };
            let Some(way) = index.way_through(top, &pose.through, pose.inner) else { continue };
            let Some(below) = way.iter().map(|id| index.at.get(id).copied()).collect::<Option<Vec<_>>>() else { continue };
            // The whole placement, from the root down, and not from the pose's occurrence: a
            // flexible occurrence above that one can have a copy of its own of what a pose kept
            // from the linked design is for, and the world shows that copy there.
            let mut chain = index.chain(top.id);
            chain.extend(below);
            out.push(PlacedCopy { occurrence: pose.occurrence, through: pose.through.clone(), inner: pose.inner, world: poses.along(&chain) });
        }
        out
    }

    /// Keep where the solver placed flexible occurrences' own copies of the occurrences inside
    /// them ([`Solution::placed`]) as [`PlacedPose`]s. Returns true when one changed.
    pub(crate) fn apply_placed(&mut self, placed: &[PlacedCopy]) -> bool {
        if placed.is_empty() {
            return false;
        }
        // What each copy needs from the design, looked up once: its occurrence's world
        // transform, and the occurrences above it inside that occurrence, each with what it is
        // under and where it is shown now. A copy's pose is in the frame of the copy above it,
        // so the copies are taken from the top down, and one above is read as this loop leaves it.
        struct Entry<'p> {
            copy: &'p PlacedCopy,
            above: Vec<(u64, Vec<u64>, Mat)>,
            top: Mat,
        }
        let mut entries: Vec<Entry> = {
            let index = Index::of(self);
            let poses = Poses::of(self);
            let mut tops: HashMap<u64, Mat> = HashMap::new();
            placed
                .iter()
                .filter_map(|copy| {
                    let top = index.at.get(&copy.occurrence).copied()?;
                    let way = index.way_through(top, &copy.through, copy.inner)?;
                    let (_, above) = way.split_last()?;
                    let mut path = vec![top.id];
                    let above = above
                        .iter()
                        .map(|id| {
                            let x = index.at.get(id).copied()?;
                            let under: Vec<u64> = path.iter().skip(1).copied().filter(|on_way| copy.through.contains(on_way)).collect();
                            path.push(*id);
                            Some((*id, under, poses.as_ref().map_or(x.transform, |poses| poses.local(&path, x))))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let top = *tops.entry(top.id).or_insert_with(|| index.world(poses.as_ref(), top.id));
                    Some(Entry { copy, above, top })
                })
                .collect()
        };
        entries.sort_by_key(|entry| entry.above.len());
        let poses = &mut self.assembly.poses;
        let find = |poses: &[PlacedPose], occurrence: u64, inner: u64, through: &[u64]| {
            poses.binary_search_by(|p| (p.occurrence, p.inner, p.through.as_slice()).cmp(&(occurrence, inner, through)))
        };
        // New poses wait here until every copy is read: each put in its place in the list at
        // once would move the list once for each.
        let mut fresh: BTreeMap<(u64, u64, Vec<u64>), Mat> = BTreeMap::new();
        let mut changed = false;
        for entry in entries {
            let copy = entry.copy;
            let mut parent = entry.top;
            for (id, under, shown) in entry.above {
                // The copy above, as this loop leaves it: a pose that was there before this
                // solve is changed where it is in the list, and a new one waits in `fresh`. An
                // occurrence that is no copy of this occurrence's is where it was shown.
                let kept = find(poses, copy.occurrence, id, &under).ok().and_then(|at| poses.get(at)).map(|p| p.transform);
                let local = kept.or_else(|| fresh.get(&(copy.occurrence, id, under)).copied()).unwrap_or(shown);
                parent = mat_mul(&parent, &local);
            }
            let Some(local) = mat_inverse(&parent).map(|inverse| mat_mul(&inverse, &copy.world)) else { continue };
            // A pose is saved with the design: one that is no placement would stop it opening.
            if !is_placement(&local) {
                continue;
            }
            // A copy the joints move has a pose from the first solve on, also where that is
            // where the design has the occurrence: what lists the copies has each from the start.
            let at = find(poses, copy.occurrence, copy.inner, &copy.through).ok();
            let now = at.and_then(|at| poses.get(at)).map(|p| p.transform);
            if let Some(now) = now {
                let (dt, da) = mismatch(&now, &local);
                if dt <= 1e-9 && da <= 1e-10 {
                    continue;
                }
            }
            match at.and_then(|at| poses.get_mut(at)) {
                Some(pose) => pose.transform = local,
                None => {
                    fresh.insert((copy.occurrence, copy.inner, copy.through.clone()), local);
                }
            }
            changed = true;
        }
        if !fresh.is_empty() {
            poses.extend(fresh.into_iter().map(|((occurrence, inner, through), transform)| PlacedPose { occurrence, through, inner, transform }));
            self.assembly.order_poses();
        }
        changed
    }

    /// Where each of `sides` is in a linked subassembly, as the joint solver takes a joint side
    /// ([`Inside`]): what a command asks before it makes a joint.
    pub fn sides_inside(&self, sides: &[&JointOrigin]) -> Vec<Inside> {
        let Some(copies) = Copies::of(self, true) else { return sides.iter().map(|_| Inside::Outside).collect() };
        sides
            .iter()
            .map(|side| match copies.on(side) {
                On::Outside => Inside::Outside,
                On::Elsewhere(top) => Inside::Elsewhere { top: top.id },
                On::Inside { top, body, inside, from } if body != top.id => {
                    // The copy itself, or an occurrence below it that it carries.
                    match from.checked_sub(1).and_then(|q| inside.get(q)) {
                        Some(inner) => Inside::Moved { top: top.id, inner: *inner, copy: body },
                        None => Inside::With { top: top.id },
                    }
                }
                On::Inside { top, .. } => Inside::With { top: top.id },
            })
            .collect()
    }

    /// Each of `sides` as the joint solver takes it in this design, with its geometry read as a
    /// frame: what a design that links this one carries a joint's side as. A side on a copy
    /// that a flexible occurrence's joints move is a frame on that copy, named through the
    /// occurrence (`{occurrence, inside}`, the copy last); a side on anything else inside a
    /// linked subassembly is a frame on the subassembly's occurrence; any other is a frame on
    /// its own occurrence. `None` for a side whose geometry is gone, and for one on a second
    /// place of what a flexible occurrence's joints move, which no solve can follow.
    pub fn sides_as_solved(&self, st: &ModelState, sides: &[&JointOrigin]) -> Vec<Option<JointOrigin>> {
        let own = |side: &JointOrigin| Some(JointOrigin::new(side.occurrence, frame_snap(&super::resolve_origin(self, st, side).ok()?)));
        let Some(copies) = Copies::of(self, true) else { return sides.iter().map(|side| own(side)).collect() };
        sides
            .iter()
            .map(|side| match copies.on(side) {
                On::Outside => own(side),
                On::Elsewhere(_) => None,
                On::Inside { top, body, inside, from } => {
                    let on_part = resolve_on_part(self, st, side).ok()?;
                    let from = if body == top.id { 0 } else { from };
                    let carried = copies.index.down(copies.poses.as_ref(), top, &inside, from)?;
                    let snap = frame_snap(&mat_mul(&carried, &on_part));
                    Some(JointOrigin { occurrence: top.id, inside: inside.get(..from)?.to_vec(), snap })
                }
            })
            .collect()
    }

    /// The occurrences links keep for ones that are flexible in their linked designs, at any
    /// depth, and the flexible occurrences of this design's own linked subassemblies: what a
    /// design that links this one records as [`LinkTree::nested`].
    pub fn flexible_at_any_depth(&self) -> BTreeSet<u64> {
        let trees = self.links.values().filter_map(|link| link.tree.as_ref());
        trees.flat_map(|tree| tree.flexible.keys().chain(tree.nested.iter()).copied()).collect()
    }
}

/// Where a joint side is in a linked subassembly, as the joint solver takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inside {
    /// Not on an occurrence inside a linked subassembly.
    Outside,
    /// On an occurrence that moves as one with `top`, the subassembly's occurrence: every
    /// occurrence inside a rigid occurrence, and the ones a flexible occurrence's joints do not
    /// move.
    With { top: u64 },
    /// On the copy that flexible occurrence `top` has of `inner`, or on an occurrence that copy
    /// carries: the joints `top` has from its linked design move it. `copy` tells this copy
    /// from the others `top` has: two sides with the same one are moved together.
    Moved { top: u64, inner: u64, copy: u64 },
    /// On a place other than the first of an occurrence that flexible occurrence `top`'s joints
    /// move. The solver has one copy of such an occurrence, at its first place, so a joint to
    /// another place of it cannot follow those joints.
    Elsewhere { top: u64 },
}

/// One copy of an inner occurrence that the solver places on its own.
struct Body {
    id: u64,
    occurrence: u64,
    through: Vec<u64>,
    inner: u64,
}

/// Where [`Copies`] finds a joint side.
enum On<'a, 's> {
    /// Not on an occurrence inside a linked subassembly.
    Outside,
    /// Inside subassembly occurrence `top`, at `inside` from it down: on what the solver moves
    /// as `body` (a copy, or `top` itself), which is above `inside[from..]`.
    Inside { top: &'a Occurrence, body: u64, inside: std::borrow::Cow<'s, [u64]>, from: usize },
    /// On another place of an occurrence that `top`'s joints move ([`Inside::Elsewhere`]).
    Elsewhere(&'a Occurrence),
}

/// A design's linked subassemblies as the joint solver needs them: which occurrences are
/// subassemblies, what each link keeps, and the copies that flexible occurrences' joints move.
struct Copies<'a> {
    index: Index<'a>,
    poses: Option<Poses<'a>>,
    /// The occurrences of linked components that keep their design's components, each with
    /// what its link keeps.
    tops: HashMap<u64, &'a LinkTree>,
    /// Every occurrence a link keeps.
    kept: HashSet<u64>,
    /// The way down from a subassembly's component to a copy of an occurrence inside it, by
    /// the component, the occurrence and what the copy is under: found once, however many sides
    /// and copies ask.
    ways: RefCell<HashMap<(u64, u64, Vec<u64>), Option<Rc<[u64]>>>>,
    bodies: Vec<Body>,
    /// The solver's id for each copy.
    ids: ByCopy<u64>,
}

impl<'a> Copies<'a> {
    /// `None` for a design without linked subassemblies. Without `every`, also for one whose
    /// joints the solver can take as they are: no occurrence is flexible, and no joint side
    /// names an occurrence inside a subassembly by that occurrence alone. That is what a design
    /// with rigid occurrences only pays at each solve.
    fn of(doc: &'a Document, every: bool) -> Option<Self> {
        let trees: HashMap<u64, &LinkTree> = doc
            .links
            .iter()
            .filter_map(|(top, link)| link.tree.as_ref().filter(|tree| !tree.occurrences.is_empty()).map(|tree| (*top, tree)))
            .collect();
        if trees.is_empty() {
            return None;
        }
        let kept: HashSet<u64> = trees.values().flat_map(|tree| tree.occurrences.values().copied()).collect();
        let flexible = trees.values().any(|tree| !tree.flexible.is_empty());
        if !every && !flexible && !doc.assembly.joints.iter().any(|j| kept.contains(&j.a.occurrence) || kept.contains(&j.b.occurrence)) {
            return None;
        }
        let tops: HashMap<u64, &LinkTree> = doc.occurrences.iter().filter_map(|o| Some((o.id, *trees.get(&o.component)?))).collect();
        let mut copies =
            Copies { index: Index::of(doc), poses: Poses::of(doc), tops, kept, ways: RefCell::default(), bodies: Vec::new(), ids: ByCopy::new() };
        if flexible {
            copies.find_bodies(doc)?;
        }
        Some(copies)
    }

    /// [`Index::way_through`], found once for a subassembly's component and a copy.
    fn way(&self, top: &Occurrence, through: &[u64], inner: u64) -> Option<Rc<[u64]>> {
        let key = (top.component, inner, through.to_vec());
        self.ways.borrow_mut().entry(key).or_insert_with(|| self.index.way_through(top, through, inner).map(Rc::from)).clone()
    }

    /// The copy a way down from `top` ends at, when it is the way the solver has that copy by:
    /// what the copy is under, and the inner occurrence.
    fn copy_at(&self, top: &Occurrence, tree: &LinkTree, way: &[u64]) -> Option<(Vec<u64>, u64)> {
        let (last, above) = way.split_last()?;
        let through = context(tree, above);
        (self.way(top, &through, *last).as_deref() == Some(way)).then_some((through, *last))
    }

    /// The copies that get a place of their own in the solve: for each flexible occurrence, the
    /// inner occurrences its design's joints move ([`moving`]). `None` when the design has no
    /// ids left for them.
    fn find_bodies(&mut self, doc: &'a Document) -> Option<()> {
        // The joints a flexible occurrence has from its design are the recorded ones that are
        // on it: a record is not trusted.
        let recorded: HashMap<u64, HashSet<u64>> = self
            .tops
            .iter()
            .filter_map(|(top, tree)| Some((*top, tree.flexible.get(top)?.values().copied().collect::<HashSet<u64>>())))
            .filter(|(_, joints)| !joints.is_empty())
            .collect();
        if recorded.is_empty() {
            return Some(());
        }
        let mut own: HashMap<u64, Vec<&Joint>> = HashMap::new();
        for j in doc.assembly.joints.iter().filter(|j| !j.suppressed && j.a.occurrence == j.b.occurrence).take(MAX_JOINTS) {
            if recorded.get(&j.a.occurrence).is_some_and(|joints| joints.contains(&j.id)) {
                own.entry(j.a.occurrence).or_default().push(j);
            }
        }
        // The copies' ids are above any id the design has given an occurrence.
        let mut next = doc.occurrences.iter().map(|o| o.id).max()?.checked_add(1)?;
        for top in &doc.occurrences {
            let (Some(tree), Some(mine)) = (self.tops.get(&top.id).copied(), own.get(&top.id)) else { continue };
            // A side of one of its joints is a copy of an inner occurrence, by the way down to it.
            let end = |side: &JointOrigin| {
                let last = side.inside.last()?;
                if tree.fixed.contains(last) { None } else { self.copy_at(top, tree, &side.inside) }
            };
            let sides: Vec<[Option<(Vec<u64>, u64)>; 2]> = mine.iter().map(|j| [end(&j.a), end(&j.b)]).collect();
            for (through, inner) in moving(&sides) {
                self.ids.insert(top.id, through.clone(), inner, next);
                self.bodies.push(Body { id: next, occurrence: top.id, through, inner });
                next = next.checked_add(1)?;
            }
        }
        Some(())
    }

    /// Where a joint side is.
    fn on<'s>(&self, side: &'s JointOrigin) -> On<'a, 's> {
        use std::borrow::Cow;
        let direct = !side.inside.is_empty() && self.tops.contains_key(&side.occurrence);
        let (top, inside): (&'a Occurrence, Cow<'s, [u64]>) = if direct {
            let Some(top) = self.index.at.get(&side.occurrence).copied() else { return On::Outside };
            (top, Cow::Borrowed(side.inside.as_slice()))
        } else if self.kept.contains(&side.occurrence) {
            // Named by the inner occurrence alone: through the first occurrence of each
            // component above it.
            let chain = self.index.chain(side.occurrence);
            let found = chain.iter().position(|o| self.tops.contains_key(&o.id));
            let Some((top, below)) = found.and_then(|i| Some((chain.get(i).copied()?, chain.get(i + 1..)?))) else { return On::Outside };
            (top, Cow::Owned(below.iter().map(|o| o.id).chain(side.inside.iter().copied()).collect()))
        } else {
            // On an ordinary occurrence, through a flexible occurrence inside it: that is no
            // first place of what the flexible occurrence's joints move.
            let through = side.inside.iter().position(|id| self.tops.contains_key(id));
            let Some((top, below)) = through.and_then(|i| Some((side.inside.get(i)?, side.inside.get(i + 1..)?))) else { return On::Outside };
            if below.iter().any(|inner| self.ids.any(*top, *inner))
                && let Some(top) = self.index.at.get(top).copied()
            {
                return On::Elsewhere(top);
            }
            return On::Outside;
        };
        if inside.is_empty() || inside.len() > MAX_PLACEMENT_DEPTH {
            return On::Outside;
        }
        let Some(tree) = self.tops.get(&top.id).copied() else { return On::Outside };
        // The lowest occurrence on the way that is a copy of its own: by the way the solver has
        // that copy, or it is another place of it.
        let hit = (0..inside.len()).rev().find_map(|q| {
            let (inner, above) = (*inside.get(q)?, inside.get(..q)?);
            let body = self.ids.get(top.id, &context(tree, above), inner)?;
            Some((q, body))
        });
        match hit {
            Some((q, body)) => {
                let same = inside.get(..=q).is_some_and(|way| self.copy_at(top, tree, way).is_some());
                if same { On::Inside { top, body, inside, from: q + 1 } } else { On::Elsewhere(top) }
            }
            None => On::Inside { top, body: top.id, inside, from: 0 },
        }
    }
}

/// A design as the solver takes it when it has linked subassemblies ([`expand`]): the joints
/// it solves in place of the design's own, and the copies it places beside the design's
/// occurrences.
pub(crate) struct Expanded {
    /// The design's joints, each side on an occurrence inside a subassembly a frame on what the
    /// solver moves, and its motion links: all a solve reads of an assembly.
    pub(crate) assembly: Assembly,
    /// The copies, by their ids.
    bodies: Vec<Body>,
    ids: ByCopy<u64>,
    start: Start,
    /// Joints the solver cannot take: (joint id, name, why).
    refused: Vec<(u64, String, String)>,
}

/// What an error says of an occurrence a solve would misplace, up to the occurrence's id.
pub(crate) const WOULD_PLACE: &str = "it would place occurrence ";

impl Expanded {
    /// What the solve starts from, as the design's own joints say it.
    pub(crate) fn start(&self) -> Start {
        self.start
    }

    /// The solver's answer for the design itself: the copies' placements as
    /// [`Solution::placed`], and what the solve says of a copy said of the occurrences the
    /// design has.
    pub(crate) fn fold(&self, doc: &Document, mut sol: Solution) -> Solution {
        let mut dof: ByCopy<usize> = ByCopy::new();
        for body in &self.bodies {
            if let Some(world) = sol.world.remove(&body.id) {
                sol.placed.push(PlacedCopy { occurrence: body.occurrence, through: body.through.clone(), inner: body.inner, world });
            }
            if let Some(d) = sol.dof.remove(&body.id) {
                dof.insert(body.occurrence, body.through.clone(), body.inner, d);
            }
        }
        if let Some(first) = self.bodies.first() {
            sol.fixed.retain(|o| *o < first.id);
            // An error that names a copy by the id the solver has for it names what it is.
            for (_, _, why) in &mut sol.errors {
                let Some(rest) = why.strip_prefix(WOULD_PLACE) else { continue };
                let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                let Some(id) = rest.get(..digits).and_then(|id| id.parse::<u64>().ok()) else { continue };
                let Some(body) = self.bodies.binary_search_by(|b| b.id.cmp(&id)).ok().and_then(|at| self.bodies.get(at)) else { continue };
                *why = format!("{WOULD_PLACE}{}'s own copy of occurrence {}{}", body.occurrence, body.inner, rest.get(digits..).unwrap_or_default());
            }
        }
        sol.errors.extend(self.refused.iter().cloned());
        kept_dof(doc, sol, Some(&dof))
    }
}

/// The degrees of freedom of the occurrences links keep. An occurrence inside a subassembly is
/// not placed by a joint of its own: it has those of what carries it, which is the copy a
/// flexible occurrence's joints move (`copies`) or the subassembly's occurrence. An inner
/// occurrence's id stands for the one its component's first occurrence shows.
pub(crate) fn kept_dof(doc: &Document, mut sol: Solution, copies: Option<&ByCopy<usize>>) -> Solution {
    if sol.dof.is_empty() {
        return sol;
    }
    let trees: HashMap<u64, &LinkTree> =
        doc.links.iter().filter_map(|(top, link)| link.tree.as_ref().filter(|tree| !tree.occurrences.is_empty()).map(|tree| (*top, tree))).collect();
    if trees.is_empty() {
        return sol;
    }
    let index = Index::of(doc);
    for kept in trees.values().flat_map(|tree| tree.occurrences.values()) {
        let chain = index.chain(*kept);
        let Some((i, tree)) = chain.iter().enumerate().find_map(|(i, o)| Some((i, *trees.get(&o.component)?))) else { continue };
        let (Some(top), Some(below)) = (chain.get(i), chain.get(i + 1..)) else { continue };
        let below: Vec<u64> = below.iter().map(|o| o.id).collect();
        let copy = copies.and_then(|copies| (0..below.len()).rev().find_map(|q| copies.get(top.id, &context(tree, below.get(..q)?), *below.get(q)?)));
        if let Some(carried) = copy.or_else(|| sol.dof.get(&top.id).copied()) {
            sol.dof.insert(*kept, carried);
        }
    }
    sol
}

/// What the solver moves for the occurrence at the end of `path`, and the transform from that
/// occurrence's frame to it. `path` is occurrence ids from the root down to `occurrence`;
/// empty, it is the first occurrence of each component above it. An occurrence inside a linked
/// subassembly has no place of its own in a solve: the copy a flexible occurrence's joints move
/// carries it, or the subassembly's occurrence does. Any other occurrence stands for itself.
pub(crate) fn stand_in(doc: &Document, expanded: Option<&Expanded>, occurrence: u64, path: &[u64]) -> (u64, Mat) {
    let same = (occurrence, IDENTITY);
    let structured = |component: u64| doc.links.get(&component).and_then(|link| link.tree.as_ref()).is_some_and(|tree| !tree.occurrences.is_empty());
    if !doc.links.values().any(|link| link.tree.as_ref().is_some_and(|tree| !tree.occurrences.is_empty())) {
        return same;
    }
    let index = Index::of(doc);
    let chain: Vec<&Occurrence> = if path.last() == Some(&occurrence) && path.len() <= MAX_PLACEMENT_DEPTH + 1 {
        let Some(chain) = path.iter().map(|id| index.at.get(id).copied()).collect::<Option<Vec<_>>>() else { return same };
        chain
    } else {
        index.chain(occurrence)
    };
    let Some(i) = chain.iter().position(|o| structured(o.component)) else { return same };
    let (Some(top), Some(below)) = (chain.get(i).copied(), chain.get(i + 1..)) else { return same };
    // The solver has the subassembly's occurrence where the first occurrence of each component
    // above it shows it: another place of it is not what a solve moves.
    let first = index.chain(top.id);
    if below.is_empty() || first.len() != i + 1 || first.iter().zip(&chain).any(|(a, b)| a.id != b.id) {
        return same;
    }
    let inside: Vec<u64> = below.iter().map(|o| o.id).collect();
    let tree = doc.links.get(&top.component).and_then(|link| link.tree.as_ref());
    let hit = expanded.zip(tree).and_then(|(x, tree)| {
        (0..inside.len()).rev().find_map(|q| {
            let (inner, through) = (*inside.get(q)?, context(tree, inside.get(..q)?));
            let body = x.ids.get(top.id, &through, inner)?;
            Some((q, through, body))
        })
    });
    let (body, from) = match hit {
        Some((q, through, body)) => {
            // The way the solver has that copy by, or this is another place of it.
            let way = inside.get(q).and_then(|inner| index.way_through(top, &through, *inner));
            if way.as_deref() != inside.get(..=q) {
                return same;
            }
            (body, q + 1)
        }
        None => (top.id, 0),
    };
    match index.down(Poses::of(doc).as_ref(), top, &inside, from) {
        Some(carry) => (body, carry),
        None => same,
    }
}

/// An explicit frame as a joint side keeps it: its origin and its z and x axes.
pub fn frame_snap(m: &Mat) -> Snap {
    Snap::Frame { origin: apply_point(m, Vec3::ZERO), z: apply_vector(m, Vec3::Z), x: Some(apply_vector(m, Vec3::X)) }
}

/// Whether a joint side is a datum plane or axis.
fn datum(snap: &Snap) -> bool {
    matches!(snap, Snap::Plane { .. } | Snap::Axis { .. })
}

/// Whether both sides of a joint are datum planes or axes, which is what a joint's `flip`
/// counts from (`flip_of`): a joint whose sides become frames keeps what it does only when its
/// `flip` turns with that.
pub fn both_datums(a: &Snap, b: &Snap) -> bool {
    datum(a) && datum(b)
}

/// The copies of a flexible occurrence that its design's joints move: `sides` is what each of
/// those joints is between, a copy of an inner occurrence, or `None` for the occurrence itself
/// and for an inner occurrence its design holds still. The ones joined, through any
/// number of joints, to something that stays with the occurrence move. When nothing of it stays
/// (the design grounds none of them), the first side of the first joint does, and what is
/// joined to that moves, as the solver starts a design that grounds nothing from its first
/// joint. Inner occurrences joined only to each other otherwise stay where the design has
/// them, as its own solve leaves them.
fn moving<K: Ord + Clone>(sides: &[[Option<K>; 2]]) -> BTreeSet<K> {
    let mut next: BTreeMap<&K, Vec<&K>> = BTreeMap::new();
    let mut todo: Vec<&K> = Vec::new();
    for side in sides {
        match side {
            [Some(a), Some(b)] if a != b => {
                next.entry(a).or_default().push(b);
                next.entry(b).or_default().push(a);
            }
            [Some(inner), None] | [None, Some(inner)] => todo.push(inner),
            _ => {}
        }
    }
    let mut still = None;
    if todo.is_empty() {
        let first = sides.iter().find_map(|side| match side {
            [Some(a), Some(b)] if a != b => Some(a),
            _ => None,
        });
        let Some(first) = first else { return BTreeSet::new() };
        still = Some(first);
        todo.extend(next.get(first).into_iter().flatten().copied());
    }
    let mut reached: BTreeSet<&K> = BTreeSet::new();
    while let Some(inner) = todo.pop() {
        if Some(inner) != still && reached.insert(inner) {
            todo.extend(next.get(inner).into_iter().flatten().copied());
        }
    }
    reached.into_iter().cloned().collect()
}

/// The design as the solver takes it: every copy of an inner occurrence that a flexible
/// occurrence's joints move has a place of its own, and each joint side on an occurrence inside
/// a linked subassembly is a frame on such a copy, or on the subassembly's occurrence itself
/// when its joints do not move that one (always, for a rigid occurrence). `None` for a design
/// whose joints the solver can take as they are, which is every design without a flexible
/// occurrence unless a side names an inner occurrence alone: it is solved as it is, and only
/// the joints and motion links of any other are copied for the solve, never the design.
pub(crate) fn expand(doc: &Document, st: &ModelState) -> Option<Expanded> {
    let copies = Copies::of(doc, false)?;
    let joints = &doc.assembly.joints;
    let mut rewrites: Vec<(usize, bool, JointOrigin)> = Vec::new();
    let mut refused: Vec<(usize, &Occurrence)> = Vec::new();
    for (at, j) in joints.iter().enumerate().filter(|(_, j)| !j.suppressed) {
        for (second, side) in [(false, &j.a), (true, &j.b)] {
            match copies.on(side) {
                On::Outside => {}
                On::Elsewhere(top) => refused.push((at, top)),
                // On the subassembly's occurrence already, through the occurrences it names:
                // the solver reads that itself.
                On::Inside { top, body, .. } if body == top.id && side.occurrence == top.id => {}
                On::Inside { top, body, inside, from } => {
                    // The frame on the occurrence the side's geometry is on; one that can't be
                    // found is the solver's to report.
                    let Ok(on_part) = resolve_on_part(doc, st, side) else { continue };
                    let Some(to_body) = copies.index.down(copies.poses.as_ref(), top, &inside, from) else { continue };
                    rewrites.push((at, second, JointOrigin::new(body, frame_snap(&mat_mul(&to_body, &on_part)))));
                }
            }
        }
    }
    if rewrites.is_empty() && refused.is_empty() {
        return None;
    }
    let mut assembly = Assembly { joints: joints.clone(), links: doc.assembly.links.clone(), ..Assembly::default() };
    // A datum joint is taken up after the others (`build`): by what the design's joints are,
    // not by the frames they are solved as.
    let has_datum = joints.iter().filter(|j| !j.suppressed).take(MAX_JOINTS).any(|j| datum(&j.a.snap) || datum(&j.b.snap));
    for (at, second, side) in rewrites {
        let Some(j) = assembly.joints.get_mut(at) else { continue };
        let before = both_datums(&j.a.snap, &j.b.snap);
        *(if second { &mut j.b } else { &mut j.a }) = side;
        // A frame is no datum: the joint keeps what it does (`both_datums`).
        if before != both_datums(&j.a.snap, &j.b.snap) {
            j.flip = !j.flip;
        }
    }
    let mut errors = Vec::new();
    for (at, top) in refused {
        let Some(j) = assembly.joints.get_mut(at).filter(|j| !j.suppressed) else { continue };
        // Left out of the solve: taken as a frame it would stand where its occurrence was
        // before the solve moved it.
        j.suppressed = true;
        errors.push((
            j.id,
            j.name.clone(),
            format!("a side is on a second place of an occurrence that the joints of `{}` move: join to the first place of it", top.name),
        ));
    }
    // With nothing fixed among the joined occurrences, the first joint's A side is held: the
    // design's own occurrence there, which for a copy is the subassembly's occurrence.
    let anchor = assembly.joints.iter().find(|j| !j.suppressed).map(|j| {
        let body = copies.bodies.binary_search_by(|b| b.id.cmp(&j.a.occurrence)).ok().and_then(|at| copies.bodies.get(at));
        body.map_or(j.a.occurrence, |body| body.occurrence)
    });
    let Copies { bodies, ids, .. } = copies;
    Some(Expanded { assembly, bodies, ids, start: Start { anchor, datum: Some(has_datum) }, refused: errors })
}

#[cfg(test)]
#[path = "joints_placed_tests.rs"]
mod tests;
