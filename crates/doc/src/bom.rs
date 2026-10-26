//! The bill of materials of an assembly (the design, or one component's definition): the rows a
//! bill of materials view lists, typed. `assembly.bom`'s JSON and CSV and the Bill of Materials
//! window present these rows; parts lists, item numbers and balloons are to read them too.
//!
//! [`list`] walks the placements below the assembly once. One rule (`classify`) decides for each
//! placement whether it is counted in a row, how what it holds is walked and what its row stands
//! for when weighed, from the bill of materials type it counts as (its occurrence's, else its
//! component's), the view and the component's shape. One budget of [`MAX_PLACEMENTS`] placements
//! and [`MAX_PLACEMENT_DEPTH`] levels covers the whole list, folded parts' contents included: a
//! larger assembly is refused, never listed in part.
//!
//! A row is named by its `RowKey`: the rows it is listed under and its lowest member (a
//! component and the type it counts as there), however its part number or contents read. Its
//! mass is what one of its parts weighs as the row stands for it (`Holds`), so the rows of any
//! list add up to the assembly once. The keys, what rows stand for, their members and `row_of`
//! stay inside this crate until balloons read them; item numbers are saved by key ([`numbers`]). Placements are rigid, so
//! each definition is weighed once however often it is placed, and kept in the [`Tree`] for every list made from it. The
//! assembly's own mass is weighed the same way, never added up from rows: a row whose definitions
//! weigh differently has none.
//!
//! Materials are bodies' (`material.assign`): a row names the materials of the bodies it stands for,
//! and a row that stands for an assembly names one only when everything in it shares it. A static
//! material typed in for a component ([`metadata::MATERIAL`](crate::metadata::MATERIAL)) is what
//! its rows say instead, in every view and at every level ([`Row::statics`]), and what it stands
//! for in the one material an assembly around it shares: the bill of materials says one thing
//! about the part everywhere. It only names: mass still comes from the bodies.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{BomType, Component, DocError, Document, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, ModelBody, ModelState, Result};

mod numbers;

pub use numbers::{ItemNumber, ItemNumbers, MAX_ITEM, Numbering, Order, number, record};

/// A bill of materials view, saved by the word commands use for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum View {
    /// Parts only: every part at any depth, counted through its subassemblies.
    #[default]
    #[serde(rename = "flat")]
    PartsOnly,
    /// Structured, first level: the assembly's direct contents, a subassembly as one row.
    #[serde(rename = "top")]
    FirstLevel,
    /// Structured, all levels: the assembly's direct contents, each subassembly a row with its own
    /// contents listed under it, to any depth (1, 1.1, 1.1.1).
    #[serde(rename = "all")]
    AllLevels,
}

impl View {
    /// Every view, in the order commands and documentation list them.
    pub const ALL: [View; 3] = [View::PartsOnly, View::FirstLevel, View::AllLevels];

    /// The word commands use (`assembly.bom`'s `levels`).
    pub fn as_str(self) -> &'static str {
        match self {
            View::PartsOnly => "flat",
            View::FirstLevel => "top",
            View::AllLevels => "all",
        }
    }

    /// The view a word names, if it names one.
    pub fn parse(word: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.as_str() == word)
    }
}

/// What one row gathers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// The definitions that share a part number; a definition without one is a row of its own.
    #[default]
    PartNumber,
    /// One definition.
    Definition,
}

impl Group {
    /// Every grouping, in the order commands and documentation list them.
    pub const ALL: [Group; 2] = [Group::PartNumber, Group::Definition];

    /// The word commands use (`assembly.bom`'s `group_by`).
    pub fn as_str(self) -> &'static str {
        match self {
            Group::PartNumber => "part_number",
            Group::Definition => "definition",
        }
    }

    /// The grouping a word names, if it names one.
    pub fn parse(word: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.as_str() == word)
    }
}

/// What a bill of materials lists. The default is the design's parts only, by part number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// The assembly listed: a component, or 0 for the design.
    pub source: u64,
    pub view: View,
    pub group: Group,
    /// A normal part with a part number is one row, and what it holds is listed under it as its
    /// children (as parts only lists them) instead of as rows of their own.
    pub fold: bool,
    /// Weigh each row and the assembly: every part is measured, so it is asked for.
    pub mass: bool,
    /// In all levels, what a purchased part holds is listed under it too (it is still one row,
    /// weighed whole).
    pub purchased_contents: bool,
}

/// A component as one placement counts: its id and the bill of materials type it counts as there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Member {
    pub component: u64,
    pub bom_type: BomType,
}

/// A row's identity: the key members of the rows it is listed under (outermost first; none at the
/// top), and its own lowest member. It doesn't change with the row's part number or contents, so
/// item numbers, overrides and balloons can follow the row through those edits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct RowKey {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) parents: Vec<Member>,
    pub(crate) member: Member,
}

/// What one of a row's parts stands for when it is weighed, so that the rows of any list add up
/// to the assembly once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Holds {
    /// Its own bodies and everything placed inside it.
    All,
    /// Its own bodies alone: what it holds is listed beside it.
    Own,
    /// Everything but its purchased parts, which are listed beside it (an inseparable part in
    /// parts only).
    AllButPurchased,
}

/// What one of a row's parts weighs, or the assembly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Mass {
    /// Not asked for ([`Request::mass`]).
    #[default]
    NotAsked,
    Grams(f64),
    Unknown(Unknown),
}

/// Why a mass isn't known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unknown {
    /// Nothing in it has a body.
    NothingToWeigh,
    /// A body in it can't be measured, or measures inside out (a negative volume).
    Unmeasurable,
    /// The definitions the row gathers (one part number) weigh differently.
    Differs,
}

/// A value of a row typed in by hand for its component, in place of the one worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Static {
    /// [`Row::materials`], from [`metadata::MATERIAL`](crate::metadata::MATERIAL).
    Material,
}

impl Static {
    /// The part property it is kept under, and its name in `assembly.bom`'s rows.
    pub fn key(self) -> &'static str {
        match self {
            Static::Material => crate::metadata::MATERIAL,
        }
    }
}

/// One row of a bill of materials view.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub(crate) key: RowKey,
    /// Its item number among the rows it is listed with: the one saved for it, else its position
    /// from 1 (the view not numbered yet) or the next number after the saved ones ([`numbers`]).
    pub item: u32,
    /// Its item number isn't saved yet.
    pub provisional: bool,
    /// Its item number is locked: a renumber keeps it.
    pub locked: bool,
    /// Another row listed with it has the same item number.
    pub duplicate: bool,
    pub bom_type: BomType,
    /// What one of its parts stands for when weighed.
    pub(crate) stands_for: Holds,
    /// The definitions it gathers, by id (ascending), and their names, each once.
    pub components: Vec<u64>,
    pub names: Vec<String>,
    /// Its part properties: its first definition's part number (the one it gathers by), and the
    /// first description and revision its definitions give.
    pub part_number: String,
    pub description: String,
    pub revision: String,
    /// The definitions' free properties; where several give one name, the first's.
    pub properties: BTreeMap<String, String>,
    /// The physical materials of what one of its parts stands for, each once, in body order: a
    /// part's own bodies'. A row that stands for an assembly (something placed inside it is weighed
    /// with it) shows the one material all of that shares, else none. A static material replaces
    /// them.
    pub materials: Vec<String>,
    /// Its values typed in by hand ([`Static`]). Where its definitions give different ones, the
    /// first's that gives one wins, as with its description.
    pub statics: Vec<Static>,
    /// How many are listed: in all, and in one of the row it is listed under (the assembly at the
    /// top).
    pub count: u64,
    pub each: u64,
    /// The placements it counts, by occurrence path from the assembly listed, in path order;
    /// empty unless the list was made with them (`list_with`).
    pub(crate) members: Vec<Vec<u64>>,
    pub mass: Mass,
    /// What one of it holds, listed under it: a folded part's contents as parts only lists them,
    /// or in all levels a subassembly's own rows.
    pub children: Vec<Row>,
}

/// The assembly as a whole.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Total {
    /// Every placement listed, children included.
    pub occurrences: u64,
    /// Its own bodies and all it holds but references, weighed from the design (with the rows'
    /// measurements), not added up from rows.
    pub mass: Mass,
}

/// The rows of one bill of materials view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bom {
    /// The assembly listed: a component, or 0 for the design.
    pub source: u64,
    /// By part number, rows without one after them by name, one part's rows by type (`list_order`);
    /// each row's children likewise. In a numbered view, by item number.
    pub rows: Vec<Row>,
    pub total: Total,
    /// Its item numbers are saved: it has been numbered ([`ItemNumbers`]).
    pub numbered: bool,
    /// Each member path, and where its row is (its position among the rows, then among each one's
    /// children): `row_of` reads it. Empty unless the list was made with its members.
    index: HashMap<Vec<u64>, Vec<usize>>,
}

/// Why a placement is counted in no row of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "item numbers and balloons follow rows by them (#3874)"))]
pub(crate) enum Unlisted {
    /// No placement below the assembly listed has that path.
    NotFound,
    /// It counts as a reference, or is inside one: no bill of materials counts it.
    Reference,
    /// It has no row of its own and isn't inside one: a phantom without bodies, or in parts only
    /// a subassembly listed through its parts (the assembly listed, an empty path, is none either).
    NoRow,
}

impl Bom {
    /// Every row, each followed by the rows listed under it (1, 1.1, 1.2, 2…), with its item
    /// numbers from the top (`[1, 2]` reads 1.2).
    pub fn rows_deep(&self) -> Vec<(Vec<u32>, &Row)> {
        let mut out = Vec::new();
        let mut stack: Vec<(Vec<u32>, &Row)> = self.rows.iter().rev().map(|r| (vec![r.item], r)).collect();
        while let Some((item, row)) = stack.pop() {
            for child in row.children.iter().rev() {
                let mut under = item.clone();
                under.push(child.item);
                stack.push((under, child));
            }
            out.push((item, row));
        }
        out
    }

    /// The row a placement is counted in, by its occurrence path from the assembly listed: the row
    /// that lists it, else the row of the part it is inside (a nut inside a purchased bolt is
    /// counted in the bolt's). `tree` is the one the list was made from, with its members
    /// (`list_with`).
    #[cfg_attr(not(test), expect(dead_code, reason = "item numbers and balloons follow rows by them (#3874)"))]
    pub(crate) fn row_of(&self, tree: &Tree, path: &[u64]) -> std::result::Result<&Row, Unlisted> {
        if path.len() > MAX_PLACEMENT_DEPTH {
            return Err(Unlisted::NotFound);
        }
        let mut component = self.source;
        for id in path {
            let placed = tree.contents(component).iter().find(|p| p.occurrence == *id).ok_or(Unlisted::NotFound)?;
            if placed.member.bom_type == BomType::Reference {
                return Err(Unlisted::Reference);
            }
            component = placed.member.component;
        }
        // Each placement is in one row or none, so the longest member path it starts with is its row's.
        let at = (1..=path.len()).rev().find_map(|n| path.get(..n).and_then(|start| self.index.get(start)));
        at.and_then(|at| self.row_at(at)).ok_or(Unlisted::NoRow)
    }

    /// The row at `at`: its position among the rows, then among each one's children.
    fn row_at(&self, at: &[usize]) -> Option<&Row> {
        let (first, rest) = at.split_first()?;
        let mut row = self.rows.get(*first)?;
        for &i in rest {
            row = row.children.get(i)?;
        }
        Some(row)
    }

    /// Where each member's row is, for `row_of`.
    fn index_members(&mut self) {
        let mut index = HashMap::new();
        let mut stack: Vec<(Vec<usize>, &Row)> = self.rows.iter().enumerate().map(|(i, r)| (vec![i], r)).collect();
        while let Some((at, row)) = stack.pop() {
            for member in &row.members {
                index.insert(member.clone(), at.clone());
            }
            for (i, child) in row.children.iter().enumerate() {
                let mut under = at.clone();
                under.push(i);
                stack.push((under, child));
            }
        }
        self.index = index;
    }
}

/// Item numbers as one reads them: `[1, 2]` is "1.2".
pub fn item_text(item: &[u32]) -> String {
    item.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

/// The design's structure as bills of materials read it, looked up once, and what each
/// definition weighs, measured once for every list made from it. It reads one state of the
/// design: make another after a change.
pub struct Tree<'a> {
    doc: &'a Document,
    /// What each component places (by its id; 0 is the root), in occurrence order.
    children: HashMap<u64, Vec<Placed>>,
    components: HashMap<u64, &'a Component>,
    /// Each component's own bodies, in body order; a component that owns none isn't here.
    bodies: HashMap<u64, Vec<&'a ModelBody>>,
    /// What one of each definition weighs, by what it stands for.
    weighed: BTreeMap<(u64, Holds), Weighed>,
    /// The materials of what one of each definition stands for, each once, in body order.
    made_of: BTreeMap<(u64, Holds), Vec<String>>,
}

/// One occurrence: its id, and the component it places with the type it counts as there.
#[derive(Clone, Copy, Debug)]
struct Placed {
    occurrence: u64,
    member: Member,
}

/// What one of a component weighs: grams, and how many bodies that is (none: nothing to weigh).
/// `None` when a body in it can't be measured, or measures inside out.
type Weighed = Option<(f64, usize)>;

impl<'a> Tree<'a> {
    /// The structure of `doc` with the bodies of `state`, its evaluated model.
    pub fn new(doc: &'a Document, state: &'a ModelState) -> Tree<'a> {
        let mut children: HashMap<u64, Vec<Placed>> = HashMap::new();
        for (o, bom_type) in doc.occurrences.iter().zip(doc.occurrence_bom_types()) {
            children.entry(o.parent).or_default().push(Placed { occurrence: o.id, member: Member { component: o.component, bom_type } });
        }
        let mut components: HashMap<u64, &Component> = HashMap::new();
        for c in &doc.components {
            components.entry(c.id).or_insert(c);
        }
        let mut bodies: HashMap<u64, Vec<&ModelBody>> = HashMap::new();
        for (body, owner) in state.bodies.iter().zip(doc.body_owners(state)) {
            bodies.entry(owner).or_default().push(body);
        }
        Tree { doc, children, components, bodies, weighed: BTreeMap::new(), made_of: BTreeMap::new() }
    }

    /// What a component places.
    fn contents(&self, id: u64) -> &[Placed] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
    }

    /// The components placed in `id` whose parts are part of what `id` stands for as `holds` says
    /// (see `adds`), one for each placement, with the type each counts as there.
    fn adding(&self, id: u64, holds: Holds) -> Vec<Member> {
        self.contents(id).iter().filter(|p| adds(holds, p.member.bom_type)).map(|p| p.member).collect()
    }

    /// The static material typed in for a component, if any.
    fn static_material(&self, id: u64) -> Option<&'a str> {
        self.components.get(&id).and_then(|c| c.metadata.material())
    }

    fn own_bodies(&self, id: u64) -> &[&'a ModelBody] {
        self.bodies.get(&id).map_or(&[], Vec::as_slice)
    }

    /// What a row of `member` gathers by: its part number when `group` asks for that and it has
    /// one, else its definition. `None` for a damaged file's occurrence of a component that isn't
    /// there, which has no row.
    fn part(&self, member: Member, group: Group) -> Option<Part<'a>> {
        let c: &'a Component = self.components.get(&member.component)?;
        Some(match c.metadata.part_number() {
            Some(pn) if group == Group::PartNumber => Part::Number(pn),
            _ => Part::Definition(member.component),
        })
    }

    /// The rows of one level: the placements `at` (positions in `entries`), gathered as `group`
    /// says, listed under the rows `parents` names, `per` of them in all (1 for the assembly).
    /// The rows under these are gathered as `under` says.
    #[allow(clippy::too_many_arguments)]
    fn level(&self, listing: &Listing, paths: &mut [Vec<u64>], at: &[usize], group: Group, under: Group, parents: &[Member], per: u64) -> Vec<Row> {
        let mut grouped: BTreeMap<GroupKey<'a>, Vec<usize>> = BTreeMap::new();
        for &i in at {
            let Some(entry) = listing.entries.get(i) else { continue };
            let Some(part) = self.part(entry.member, group) else { continue };
            let kit = listing.kits.get(i).copied().unwrap_or_default();
            grouped.entry(GroupKey { part, bom_type: entry.member.bom_type, kit }).or_default().push(i);
        }
        let mut rows: Vec<Row> = grouped.values().filter_map(|members| self.row(listing, paths, members, under, parents, per)).collect();
        rows.sort_by(list_order);
        for (item, row) in (1..).zip(&mut rows) {
            row.item = item;
        }
        rows
    }

    /// Each entry's kit, as a number: what it holds as its row's children read it, to any depth.
    /// Two entries have the same kit when what they hold is the same parts (by part number, else
    /// definition), counted as the same types, each holding the same again all the way down. One
    /// pass from the last entry back: an entry's children come after it, so theirs are known first.
    fn kits(&self, entries: &[Entry], kids: &[Vec<usize>]) -> Vec<u32> {
        let mut kit_ids: HashMap<Vec<u32>, u32> = HashMap::new();
        let mut placed_ids: HashMap<(Option<Part<'a>>, BomType, u32), u32> = HashMap::new();
        // Each entry as a child reads: its part, its type and its own kit.
        let mut placed = vec![0u32; entries.len()];
        let mut kits = vec![0u32; entries.len()];
        for (i, entry) in entries.iter().enumerate().rev() {
            let mut held: Vec<u32> = kids.get(i).into_iter().flatten().filter_map(|&k| placed.get(k).copied()).collect();
            held.sort_unstable();
            let next = kit_ids.len() as u32;
            let kit = *kit_ids.entry(held).or_insert(next);
            let next = placed_ids.len() as u32;
            let key = (self.part(entry.member, Group::PartNumber), entry.member.bom_type, kit);
            if let (Some(k), Some(p)) = (kits.get_mut(i), placed.get_mut(i)) {
                *k = kit;
                *p = *placed_ids.entry(key).or_insert(next);
            }
        }
        kits
    }

    /// One row: the placements `members` (positions in `entries`) of one part, type and kit, with
    /// the rows under it gathered as `under` says. Their `paths` (when the list keeps them) move into
    /// the row: each placement is in one row.
    #[allow(clippy::too_many_arguments)]
    fn row(&self, listing: &Listing, paths: &mut [Vec<u64>], members: &[usize], under: Group, parents: &[Member], per: u64) -> Option<Row> {
        let listed: Vec<&Entry> = members.iter().filter_map(|&i| listing.entries.get(i)).collect();
        let first = listed.first()?;
        let mut components: Vec<u64> = listed.iter().map(|e| e.member.component).collect();
        components.sort_unstable();
        components.dedup();
        let member = Member { component: *components.first()?, bom_type: first.member.bom_type };
        let count = listed.len() as u64;
        let mut taken: Vec<Vec<u64>> = members.iter().filter_map(|&i| paths.get_mut(i).map(std::mem::take)).filter(|p| !p.is_empty()).collect();
        taken.sort_unstable();
        // What one of it holds is listed under it.
        let inside: Vec<usize> = members.iter().flat_map(|&i| listing.kids.get(i).into_iter().flatten().copied()).collect();
        let mut chain = parents.to_vec();
        chain.push(member);
        let mut row = Row {
            key: RowKey { parents: parents.to_vec(), member },
            item: 0,
            provisional: true,
            locked: false,
            duplicate: false,
            bom_type: member.bom_type,
            stands_for: first.holds,
            // Filled in by `name_materials`, which reads each definition once.
            materials: Vec::new(),
            statics: Vec::new(),
            components,
            names: Vec::new(),
            part_number: String::new(),
            description: String::new(),
            revision: String::new(),
            properties: BTreeMap::new(),
            count,
            // Rows with different kits are different rows, to any depth, so every part of the parent
            // row holds as many.
            each: count / per.max(1),
            members: taken,
            mass: Mass::NotAsked,
            children: self.level(listing, paths, &inside, under, under, &chain, count),
        };
        for (i, c) in row.components.iter().filter_map(|id| self.components.get(id)).enumerate() {
            let m = &c.metadata;
            if i == 0 {
                row.part_number = m.part_number().unwrap_or_default().to_string();
            }
            if !row.names.contains(&c.name) {
                row.names.push(c.name.clone());
            }
            if row.description.is_empty() {
                row.description = m.description().unwrap_or_default().to_string();
            }
            if row.revision.is_empty() {
                row.revision = m.revision().unwrap_or_default().to_string();
            }
            for (k, v) in &c.properties {
                row.properties.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        Some(row)
    }

    /// Each row's materials, and its children's (Cameron, 2026-10-23): a part lists its own bodies';
    /// a row that stands for an assembly shows a material only when everything it stands for
    /// shares it, so a frame of steel and aluminium parts reads blank and a bought bearing of steel
    /// parts reads steel. A static material, the first its definitions give, replaces that.
    fn name_materials(&mut self, rows: &mut [Row]) -> Result<()> {
        for row in rows {
            self.name_materials(&mut row.children)?;
            let typed_in = row.components.iter().find_map(|&id| self.static_material(id));
            if let Some(material) = typed_in {
                row.materials = vec![material.to_string()];
                row.statics.push(Static::Material);
                continue;
            }
            let mut names: Vec<String> = Vec::new();
            let mut assembly = false;
            for &c in &row.components {
                assembly |= row.stands_for != Holds::Own && !self.adding(c, row.stands_for).is_empty();
                for name in self.made_of(c, row.stands_for, 0)? {
                    push_once(&mut names, name);
                }
            }
            row.materials = if assembly && names.len() > 1 { Vec::new() } else { names };
        }
        Ok(())
    }

    /// The materials of what one of `id` stands for as `holds` says, each once, in body order: the
    /// bodies `weigh` weighs. A static material stands for all of a part placed inside it, and for
    /// a phantom's own bodies only (a phantom's row is its own bodies, so what it holds keeps its
    /// own). Each definition is read once for each kind of row; `depth` bounds the walk below a
    /// definition not read yet.
    fn made_of(&mut self, id: u64, holds: Holds, depth: usize) -> Result<Vec<String>> {
        if let Some(names) = self.made_of.get(&(id, holds)) {
            return Ok(names.clone());
        }
        if depth > MAX_PLACEMENT_DEPTH {
            return Err(too_big());
        }
        let mut names: Vec<String> = Vec::new();
        match self.static_material(id) {
            Some(material) if !self.own_bodies(id).is_empty() => push_once(&mut names, material.to_string()),
            Some(_) => {}
            None => {
                for body in self.own_bodies(id) {
                    push_once(&mut names, self.doc.body_material(&body.name));
                }
            }
        }
        if holds != Holds::Own {
            for child in self.adding(id, holds) {
                match self.static_material(child.component) {
                    Some(material) if child.bom_type != BomType::Phantom => push_once(&mut names, material.to_string()),
                    _ => {
                        for name in self.made_of(child.component, holds, depth + 1)? {
                            push_once(&mut names, name);
                        }
                    }
                }
            }
        }
        self.made_of.insert((id, holds), names.clone());
        Ok(names)
    }

    /// Each row's mass, and its children's.
    fn weigh_rows(&mut self, rows: &mut [Row]) -> Result<()> {
        for row in rows {
            let weighed = row.components.iter().map(|&c| self.weigh(c, row.stands_for, 0)).collect::<Result<Vec<Weighed>>>()?;
            row.mass = agree(&weighed);
            self.weigh_rows(&mut row.children)?;
        }
        Ok(())
    }

    /// One of `id` as `holds` says: its own bodies, and of what is placed inside it everything that
    /// adds to it (see `adds`). Where a part is placed doesn't change what it weighs, so each
    /// definition is weighed once for each kind of row and each body measured once. `depth` bounds
    /// the walk below a definition not weighed yet.
    fn weigh(&mut self, id: u64, holds: Holds, depth: usize) -> Result<Weighed> {
        if let Some(&weighed) = self.weighed.get(&(id, holds)) {
            return Ok(weighed);
        }
        if depth > MAX_PLACEMENT_DEPTH {
            return Err(too_big());
        }
        let mut total: Weighed = Some((0.0, 0));
        if holds == Holds::Own {
            for body in self.own_bodies(id) {
                add(&mut total, self.measure(body));
            }
        } else {
            add(&mut total, self.weigh(id, Holds::Own, depth)?);
            for child in self.adding(id, holds) {
                add(&mut total, self.weigh(child.component, holds, depth + 1)?);
            }
        }
        let weighed = total.filter(|(grams, _)| grams.is_finite());
        self.weighed.insert((id, holds), weighed);
        Ok(weighed)
    }

    /// What one body weighs, in grams: its volume, measured once for the model
    /// ([`ModelBody::volume`]), scaled by a body offset as the offset scales what is drawn (a
    /// damaged file's scaled offset weighs what is drawn).
    fn measure(&self, body: &ModelBody) -> Weighed {
        let scale = self.doc.model_body_offset(body).map_or(1.0, crate::mat_volume_scale);
        // A negative volume (a body inside out) is unknown, never a negative mass.
        body.volume().map(|v| v * scale).filter(|v| *v >= 0.0).map(|v| (v * self.doc.density(&body.name) / 1000.0, 1))
    }
}

/// The order a level lists its rows in until it is numbered: by part number; rows without one
/// after them, by name; one part's rows by type.
fn list_order(a: &Row, b: &Row) -> Ordering {
    (a.part_number.is_empty(), &a.part_number, &a.names, &a.components, a.bom_type).cmp(&(
        b.part_number.is_empty(),
        &b.part_number,
        &b.names,
        &b.components,
        b.bom_type,
    ))
}

/// What a row gathers by: a part number, borrowed from its definition, or the definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Part<'a> {
    Number(&'a str),
    Definition(u64),
}

/// How one row is told from another of its level: what it gathers by, the type its parts count
/// as, and its kit: what one of them holds as the row's children read it, to any depth (`Tree::kits`;
/// the same for all that hold nothing listed).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey<'a> {
    part: Part<'a>,
    bom_type: BomType,
    kit: u32,
}

/// The placements a list counts, what each folded or all-levels row holds, and each one's kit.
struct Listing {
    entries: Vec<Entry>,
    /// What each entry holds: the entries listed as its children.
    kids: Vec<Vec<usize>>,
    kits: Vec<u32>,
}

/// How a walk lists what it passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walk {
    /// What the view lists.
    Listed,
    /// Inside an inseparable part in parts only: just its purchased parts, which are listed
    /// beside it, at any depth.
    PurchasedOnly,
}

/// What a placed component is like, as far as listing it goes.
#[derive(Clone, Copy, Debug, Default)]
struct Shape {
    /// It places components of its own.
    contents: bool,
    /// It has bodies of its own.
    bodies: bool,
    /// It is a part with a part number in a list that folds.
    folded: bool,
    /// What it holds is listed under it if it is purchased ([`Request::purchased_contents`], all
    /// levels).
    open_purchased: bool,
}

/// How one placement counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    /// It is counted in a row here.
    row: bool,
    /// How what it holds is walked (`None`: not at all).
    inside: Option<Walk>,
    /// What it holds is listed as its row's children.
    kit: bool,
    /// What its row stands for when weighed.
    holds: Holds,
}

/// How a placement counts in `view`: the one rule every list and every mass read. A reference
/// counts nowhere and nothing inside it does; a phantom's own bodies, when it has any, are a row of
/// their own (as a subassembly's are in parts only) and what it holds counts where it is placed; a purchased part is one row with nothing inside it listed (in all levels, its
/// contents under it when asked); an inseparable part is one row, and in parts only its purchased
/// parts are listed beside it; a normal part is a row in the structured views, in all levels with
/// what it holds under it, and in parts only a subassembly is listed through its parts unless it
/// has bodies of its own. A folded part is one row holding all it holds.
fn classify(view: View, walk: Walk, bom_type: BomType, shape: Shape) -> Step {
    let nothing = Step { row: false, inside: None, kit: false, holds: Holds::All };
    let row = |holds| Step { row: true, holds, ..nothing };
    let contents = |walk| shape.contents.then_some(walk);
    match (bom_type, walk) {
        (BomType::Reference, _) => nothing,
        (BomType::Phantom, Walk::Listed) => Step { row: shape.bodies, inside: Some(Walk::Listed), kit: false, holds: Holds::Own },
        (BomType::Phantom, Walk::PurchasedOnly) => Step { inside: Some(walk), ..nothing },
        (BomType::Purchased, Walk::Listed) if view == View::AllLevels && shape.open_purchased => {
            Step { inside: contents(Walk::Listed), kit: true, ..row(Holds::All) }
        }
        (BomType::Purchased, _) => row(Holds::All),
        (_, Walk::PurchasedOnly) => Step { inside: contents(Walk::PurchasedOnly), ..nothing },
        (BomType::Inseparable, Walk::Listed) => match view {
            View::PartsOnly => Step { inside: contents(Walk::PurchasedOnly), ..row(Holds::AllButPurchased) },
            View::FirstLevel | View::AllLevels => row(Holds::All),
        },
        (BomType::Normal, Walk::Listed) if shape.folded => Step { inside: contents(Walk::Listed), kit: true, ..row(Holds::All) },
        (BomType::Normal, Walk::Listed) => match view {
            View::PartsOnly => Step { row: !shape.contents || shape.bodies, inside: contents(Walk::Listed), kit: false, holds: Holds::Own },
            View::FirstLevel => row(Holds::All),
            View::AllLevels => Step { inside: contents(Walk::Listed), kit: true, ..row(Holds::All) },
        },
    }
}

/// Whether a placement of `bom_type` inside a part adds to what the part's row stands for: a
/// reference adds nothing, and what the list shows beside the row (what `classify` lists inside
/// an inseparable part) isn't weighed with it.
fn adds(holds: Holds, bom_type: BomType) -> bool {
    match holds {
        Holds::Own => false,
        Holds::All => bom_type != BomType::Reference,
        Holds::AllButPurchased => bom_type != BomType::Reference && !classify(View::PartsOnly, Walk::PurchasedOnly, bom_type, Shape::default()).row,
    }
}

/// One placement a list counts in a row.
struct Entry {
    member: Member,
    holds: Holds,
    /// The folded placement (a position in the entries) whose children it is among.
    kit: Option<usize>,
}

/// Every placement below `request.source` that `request` counts in a row, in one walk under one
/// budget, and with `members` each one's occurrence path (else an empty path each).
fn walk(tree: &Tree, request: &Request, members: bool) -> Result<(Vec<Entry>, Vec<Vec<u64>>)> {
    struct Frame {
        component: u64,
        depth: usize,
        /// Kept only with `members`.
        path: Vec<u64>,
        walk: Walk,
        kit: Option<usize>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut paths: Vec<Vec<u64>> = Vec::new();
    let mut visited = 0usize;
    let mut stack = vec![Frame { component: request.source, depth: 0, path: Vec::new(), walk: Walk::Listed, kit: None }];
    while let Some(frame) = stack.pop() {
        for placed in tree.contents(frame.component) {
            visited += 1;
            if frame.depth >= MAX_PLACEMENT_DEPTH || visited > MAX_PLACEMENTS {
                return Err(too_big());
            }
            let component = placed.member.component;
            // What a folded part holds is listed as parts only lists it, and isn't folded again; in
            // all levels what any row holds is listed in all levels again.
            let (view, folds) = match frame.kit {
                Some(_) if request.view == View::AllLevels => (View::AllLevels, false),
                Some(_) => (View::PartsOnly, false),
                None => (request.view, request.fold),
            };
            let numbered = tree.components.get(&component).is_some_and(|c| c.metadata.part_number().is_some());
            let shape = Shape {
                contents: tree.children.contains_key(&component),
                bodies: tree.bodies.contains_key(&component),
                folded: folds && numbered,
                open_purchased: request.purchased_contents,
            };
            let step = classify(view, frame.walk, placed.member.bom_type, shape);
            let path = if members { [frame.path.as_slice(), &[placed.occurrence]].concat() } else { Vec::new() };
            let here = entries.len();
            if step.row {
                entries.push(Entry { member: placed.member, holds: step.holds, kit: frame.kit });
                paths.push(path.clone());
            }
            if let Some(walk) = step.inside {
                let kit = if step.kit { Some(here) } else { frame.kit };
                stack.push(Frame { component, depth: frame.depth + 1, path, walk, kit });
            }
        }
    }
    Ok((entries, paths))
}

/// The rows `request` lists, with the assembly's total; with `request.mass`, each row and the
/// assembly weighed.
pub fn list(tree: &mut Tree, request: &Request) -> Result<Bom> {
    list_with(tree, request, false)
}

/// [`list`], with each row's members when asked (what item numbers and balloons will follow rows
/// by, and what `row_of` reads), so a list made for its rows alone copies no paths.
pub(crate) fn list_with(tree: &mut Tree, request: &Request, members: bool) -> Result<Bom> {
    if request.source != 0 && !tree.components.contains_key(&request.source) {
        return Err(DocError::Unknown(format!("component {}", request.source)));
    }
    let (entries, mut paths) = walk(tree, request, members)?;
    // What each folded placement holds: the placements listed as its children.
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    for (i, entry) in entries.iter().enumerate() {
        if let Some(of) = entry.kit.and_then(|k| kids.get_mut(k)) {
            of.push(i);
        }
    }
    let kits = tree.kits(&entries, &kids);
    let listing = Listing { entries, kids, kits };
    let top: Vec<usize> = listing.entries.iter().enumerate().filter(|(_, e)| e.kit.is_none()).map(|(i, _)| i).collect();
    // What a folded part holds is gathered by part number in any list; all levels gathers every
    // level as it gathers the first.
    let under = if request.view == View::AllLevels { request.group } else { Group::PartNumber };
    let mut rows = tree.level(&listing, &mut paths, &top, request.group, under, &[], 1);
    let numbered = numbers::apply(&mut rows, numbers::saved(tree.doc, request.source, request.view));
    let mut bom = Bom { source: request.source, rows, total: Total::default(), numbered, index: HashMap::new() };
    if members {
        bom.index_members();
    }
    tree.name_materials(&mut bom.rows)?;
    if request.mass {
        tree.weigh_rows(&mut bom.rows)?;
        // After the rows, so what their definitions weigh is kept: only what no row holds (the
        // assembly's own bodies) is measured again.
        bom.total.mass = agree(&[tree.weigh(request.source, Holds::All, 0)?]);
    }
    let occurrences: u64 = bom.rows_deep().iter().map(|(_, row)| row.count).sum();
    bom.total.occurrences = occurrences;
    Ok(bom)
}

/// One of a row's parts, from what each of its definitions weighs: known when every one can be
/// measured, has something to weigh, and they agree. Some with something to weigh and some with
/// nothing differ.
fn agree(weighed: &[Weighed]) -> Mass {
    let Some(known) = weighed.iter().copied().collect::<Option<Vec<(f64, usize)>>>() else {
        return Mass::Unknown(Unknown::Unmeasurable);
    };
    if known.iter().all(|&(_, bodies)| bodies == 0) {
        return Mass::Unknown(Unknown::NothingToWeigh);
    }
    match known.first() {
        Some(&(first, _)) if known.iter().any(|&(grams, bodies)| bodies == 0 || !same_mass(grams, first)) => Mass::Unknown(Unknown::Differs),
        Some(&(first, _)) => Mass::Grams(first),
        None => Mass::Unknown(Unknown::NothingToWeigh),
    }
}

/// How closely the kernel's `measure` gives one solid's volume however it was modelled, relative to
/// it (about 1e-4): a pin made as a cylinder and as a revolved rectangle measured 1.05e-5 apart
/// (2026-10-26), and this is ten times that.
pub const MEASURE_ACCURACY: f64 = 1e-4;

/// Two masses of one part, measured from definitions modelled apart: one when they are within the
/// kernel's measurement accuracy of each other.
fn same_mass(a: f64, b: f64) -> bool {
    (a - b).abs() <= MEASURE_ACCURACY * a.abs().max(b.abs())
}

/// `more` added to `total`; unknown when either is. The body count only says whether there is
/// any, so a compact assembly with more placed bodies than a count holds saturates it.
fn add(total: &mut Weighed, more: Weighed) {
    *total = total.zip(more).map(|((grams, bodies), (g, n))| (grams + g, bodies.saturating_add(n)));
}

/// `name` added to `names` unless it is there already, so each is listed once, in first order.
fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

fn too_big() -> DocError {
    crate::assembly::too_many_placements()
}

#[cfg(test)]
#[path = "bom_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
