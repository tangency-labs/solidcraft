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
//! A row is named by its [`RowKey`]: the rows it is listed under and its lowest member (a
//! component and the type it counts as there), however its part number or contents read. Its
//! mass is what one of its parts weighs as the row stands for it ([`Holds`]), so the rows of any
//! list add up to the assembly once. Placements are rigid, so each definition is weighed once
//! however often it is placed, and kept in the [`Tree`] for every list made from it. The
//! assembly's own mass is weighed the same way, never added up from rows: a row whose definitions
//! weigh differently has none.

use std::collections::{BTreeMap, HashMap};

use crate::{BomType, Component, DocError, Document, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, ModelBody, ModelState, Result};

/// A bill of materials view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    /// Parts only: every part at any depth, counted through its subassemblies.
    #[default]
    PartsOnly,
    /// Structured, first level: the assembly's direct contents, a subassembly as one row.
    FirstLevel,
}

impl View {
    /// Every view, in the order commands and documentation list them.
    pub const ALL: [View; 2] = [View::PartsOnly, View::FirstLevel];

    /// The word commands use (`assembly.bom`'s `levels`).
    pub fn as_str(self) -> &'static str {
        match self {
            View::PartsOnly => "flat",
            View::FirstLevel => "top",
        }
    }

    /// The view a word names, if it names one.
    pub fn parse(word: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.as_str() == word)
    }
}

/// What one row gathers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
}

/// A component as one placement counts: its id and the bill of materials type it counts as there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Member {
    pub component: u64,
    pub bom_type: BomType,
}

/// A row's identity: the key members of the rows it is listed under (outermost first; none at the
/// top), and its own lowest member. It doesn't change with the row's part number or contents, so
/// item numbers, overrides and balloons can follow the row through those edits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey {
    pub parents: Vec<Member>,
    pub member: Member,
}

/// What one of a row's parts stands for when it is weighed, so that the rows of any list add up
/// to the assembly once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Holds {
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

/// One row of a bill of materials view.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: RowKey,
    /// Its position among the rows it is listed with, from 1: its item number until numbers are
    /// saved.
    pub item: u32,
    pub bom_type: BomType,
    /// What one of its parts stands for when weighed.
    pub stands_for: Holds,
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
    /// with it) shows the one material all of that shares, else none.
    pub materials: Vec<String>,
    /// How many are listed: in all, and in one of the row it is listed under (the assembly at the
    /// top).
    pub count: u64,
    pub each: u64,
    /// The placements it counts, by occurrence path from the assembly listed, in path order.
    pub members: Vec<Vec<u64>>,
    pub mass: Mass,
    /// What one of a folded part holds, as parts only lists it.
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
    /// By part number, rows without one after them by name, one part's rows by type; each row's
    /// children likewise.
    pub rows: Vec<Row>,
    pub total: Total,
}

/// Why a placement is counted in no row of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlisted {
    /// No placement below the assembly listed has that path.
    NotFound,
    /// It counts as a reference, or is inside one: no bill of materials counts it.
    Reference,
    /// It has no row of its own and isn't inside one: a phantom, or in parts only a subassembly
    /// listed through its parts (the assembly listed, an empty path, is none either).
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
    /// counted in the bolt's). `tree` is the one the list was made from.
    pub fn row_of(&self, tree: &Tree, path: &[u64]) -> std::result::Result<&Row, Unlisted> {
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
        let mut found: Option<(usize, &Row)> = None;
        for (_, row) in self.rows_deep() {
            for member in &row.members {
                if path.starts_with(member) && found.is_none_or(|(len, _)| member.len() > len) {
                    found = Some((member.len(), row));
                }
            }
        }
        found.map(|(_, row)| row).ok_or(Unlisted::NoRow)
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
    /// (see `adds`), one for each placement.
    fn adding(&self, id: u64, holds: Holds) -> Vec<u64> {
        self.contents(id).iter().filter(|p| adds(holds, p.member.bom_type)).map(|p| p.member.component).collect()
    }

    fn own_bodies(&self, id: u64) -> &[&'a ModelBody] {
        self.bodies.get(&id).map_or(&[], Vec::as_slice)
    }

    /// What a row of `member` gathers by: its part number when `group` asks for that and it has
    /// one, else its definition. `None` for a damaged file's occurrence of a component that isn't
    /// there, which has no row.
    fn part(&self, member: Member, group: Group) -> Option<Part> {
        let c = self.components.get(&member.component)?;
        Some(match c.metadata.part_number() {
            Some(pn) if group == Group::PartNumber => Part::Number(pn.to_string()),
            _ => Part::Definition(member.component),
        })
    }

    /// The rows of one level: the placements `at` (positions in `entries`), gathered as `group`
    /// says, listed under the rows `parents` names, `per` of them in all (1 for the assembly).
    fn level(&self, entries: &[Entry], kids: &[Vec<usize>], at: &[usize], group: Group, parents: &[Member], per: u64) -> Vec<Row> {
        let mut grouped: BTreeMap<GroupKey, Vec<usize>> = BTreeMap::new();
        for &i in at {
            let Some(entry) = entries.get(i) else { continue };
            let Some(part) = self.part(entry.member, group) else { continue };
            let kit = self.kit(entries, kids.get(i).map_or(&[], Vec::as_slice));
            grouped.entry(GroupKey { part, bom_type: entry.member.bom_type, kit }).or_default().push(i);
        }
        let mut rows: Vec<Row> = grouped.values().filter_map(|members| self.row(entries, kids, members, parents, per)).collect();
        // By part number; rows without one after them, by name; one part's rows by type.
        rows.sort_by(|a, b| {
            (a.part_number.is_empty(), &a.part_number, &a.names, &a.components, a.bom_type).cmp(&(
                b.part_number.is_empty(),
                &b.part_number,
                &b.names,
                &b.components,
                b.bom_type,
            ))
        });
        for (item, row) in (1..).zip(&mut rows) {
            row.item = item;
        }
        rows
    }

    /// What one folded placement holds, as its row's children read it: each part, the type it
    /// counts as and how many.
    fn kit(&self, entries: &[Entry], kids: &[usize]) -> BTreeMap<(Part, BomType), u64> {
        let mut kit: BTreeMap<(Part, BomType), u64> = BTreeMap::new();
        for entry in kids.iter().filter_map(|&i| entries.get(i)) {
            if let Some(part) = self.part(entry.member, Group::PartNumber) {
                *kit.entry((part, entry.member.bom_type)).or_default() += 1;
            }
        }
        kit
    }

    /// One row: the placements `members` (positions in `entries`) of one part, type and kit.
    fn row(&self, entries: &[Entry], kids: &[Vec<usize>], members: &[usize], parents: &[Member], per: u64) -> Option<Row> {
        let listed: Vec<&Entry> = members.iter().filter_map(|&i| entries.get(i)).collect();
        let first = listed.first()?;
        let mut components: Vec<u64> = listed.iter().map(|e| e.member.component).collect();
        components.sort_unstable();
        components.dedup();
        let member = Member { component: *components.first()?, bom_type: first.member.bom_type };
        let count = listed.len() as u64;
        let mut paths: Vec<Vec<u64>> = listed.iter().map(|e| e.path.clone()).collect();
        paths.sort_unstable();
        // What a folded part holds is listed under it, gathered by part number in any list.
        let inside: Vec<usize> = members.iter().flat_map(|&i| kids.get(i).into_iter().flatten().copied()).collect();
        let mut chain = parents.to_vec();
        chain.push(member);
        let mut row = Row {
            key: RowKey { parents: parents.to_vec(), member },
            item: 0,
            bom_type: member.bom_type,
            stands_for: first.holds,
            // Filled in by `name_materials`, which reads each definition once.
            materials: Vec::new(),
            components,
            names: Vec::new(),
            part_number: String::new(),
            description: String::new(),
            revision: String::new(),
            properties: BTreeMap::new(),
            count,
            // Rows with different kits are different rows, so every part of the parent row holds as many.
            each: count / per.max(1),
            members: paths,
            mass: Mass::NotAsked,
            children: self.level(entries, kids, &inside, Group::PartNumber, &chain, count),
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

    /// Each row's materials, and its children's (Cameron, 2026-10-23): a part lists its own; a row
    /// that stands for an assembly shows a material only when everything it stands for shares it,
    /// so a frame of steel and aluminium parts reads blank and a bought bearing of steel parts
    /// reads steel. A material set on a component is set on all its bodies, so it shows that way.
    fn name_materials(&mut self, rows: &mut [Row]) -> Result<()> {
        for row in rows {
            let mut names: Vec<String> = Vec::new();
            let mut assembly = false;
            for &c in &row.components {
                assembly |= row.stands_for != Holds::Own && !self.adding(c, row.stands_for).is_empty();
                for name in self.made_of(c, row.stands_for, 0)? {
                    push_once(&mut names, name);
                }
            }
            row.materials = if assembly && names.len() > 1 { Vec::new() } else { names };
            self.name_materials(&mut row.children)?;
        }
        Ok(())
    }

    /// The materials of what one of `id` stands for as `holds` says, each once, in body order: the
    /// bodies `weigh` weighs. Each definition is read once for each kind of row; `depth` bounds the
    /// walk below a definition not read yet.
    fn made_of(&mut self, id: u64, holds: Holds, depth: usize) -> Result<Vec<String>> {
        if let Some(names) = self.made_of.get(&(id, holds)) {
            return Ok(names.clone());
        }
        if depth > MAX_PLACEMENT_DEPTH {
            return Err(too_big());
        }
        let mut names: Vec<String> = Vec::new();
        for body in self.own_bodies(id) {
            push_once(&mut names, self.doc.body_material(&body.name));
        }
        if holds != Holds::Own {
            for child in self.adding(id, holds) {
                for name in self.made_of(child, holds, depth + 1)? {
                    push_once(&mut names, name);
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
                add(&mut total, self.weigh(child, holds, depth + 1)?);
            }
        }
        let weighed = total.filter(|(grams, _)| grams.is_finite());
        self.weighed.insert((id, holds), weighed);
        Ok(weighed)
    }

    /// What one body weighs, in grams.
    fn measure(&self, body: &ModelBody) -> Weighed {
        // Only the volume is needed: `measure_mass` skips the topology counts' tessellation. A
        // body offset is applied first, so a damaged file's scaled offset weighs what is drawn.
        let measured = match self.doc.model_body_offset(body) {
            Some(offset) => solidcraft_kernel::transform_matrix(&body.body, *offset).and_then(|b| solidcraft_kernel::measure_mass(&b)),
            None => solidcraft_kernel::measure_mass(&body.body),
        };
        // A negative volume (a body inside out) is unknown, never a negative mass.
        measured.ok().filter(|m| m.volume >= 0.0).map(|m| (m.volume * self.doc.density(&body.name) / 1000.0, 1))
    }
}

/// What a row gathers by.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Part {
    Number(String),
    Definition(u64),
}

/// How one row is told from another of its level: what it gathers by, the type its parts count
/// as, and what one of them holds as the row's children read it (empty unless it is folded).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    part: Part,
    bom_type: BomType,
    kit: BTreeMap<(Part, BomType), u64>,
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
/// counts nowhere and nothing inside it does; a phantom has no row and what it holds counts where
/// it is placed; a purchased part is one row with nothing inside it listed; an inseparable part
/// is one row, and in parts only its purchased parts are listed beside it; a normal part is a row
/// at the first level, and in parts only a subassembly is listed through its parts unless it has
/// bodies of its own. A folded part is one row holding all it holds.
fn classify(view: View, walk: Walk, bom_type: BomType, shape: Shape) -> Step {
    let nothing = Step { row: false, inside: None, kit: false, holds: Holds::All };
    let row = |holds| Step { row: true, holds, ..nothing };
    let contents = |walk| shape.contents.then_some(walk);
    match (bom_type, walk) {
        (BomType::Reference, _) => nothing,
        (BomType::Phantom, _) => Step { inside: Some(walk), ..nothing },
        (BomType::Purchased, _) => row(Holds::All),
        (_, Walk::PurchasedOnly) => Step { inside: contents(Walk::PurchasedOnly), ..nothing },
        (BomType::Inseparable, Walk::Listed) => match view {
            View::PartsOnly => Step { inside: contents(Walk::PurchasedOnly), ..row(Holds::AllButPurchased) },
            View::FirstLevel => row(Holds::All),
        },
        (BomType::Normal, Walk::Listed) if shape.folded => Step { inside: contents(Walk::Listed), kit: true, ..row(Holds::All) },
        (BomType::Normal, Walk::Listed) => match view {
            View::PartsOnly => Step { row: !shape.contents || shape.bodies, inside: contents(Walk::Listed), kit: false, holds: Holds::Own },
            View::FirstLevel => row(Holds::All),
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
    /// Its occurrence path from the assembly listed.
    path: Vec<u64>,
    member: Member,
    holds: Holds,
    /// The folded placement (a position in the entries) whose children it is among.
    kit: Option<usize>,
}

/// Every placement below `request.source` that `request` counts in a row, in one walk under one
/// budget.
fn walk(tree: &Tree, request: &Request) -> Result<Vec<Entry>> {
    struct Frame {
        component: u64,
        path: Vec<u64>,
        walk: Walk,
        kit: Option<usize>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut visited = 0usize;
    let mut stack = vec![Frame { component: request.source, path: Vec::new(), walk: Walk::Listed, kit: None }];
    while let Some(frame) = stack.pop() {
        for placed in tree.contents(frame.component) {
            visited += 1;
            if frame.path.len() >= MAX_PLACEMENT_DEPTH || visited > MAX_PLACEMENTS {
                return Err(too_big());
            }
            let component = placed.member.component;
            // What a folded part holds is listed as parts only lists it, and isn't folded again.
            let (view, folds) = match frame.kit {
                Some(_) => (View::PartsOnly, false),
                None => (request.view, request.fold),
            };
            let numbered = tree.components.get(&component).is_some_and(|c| c.metadata.part_number().is_some());
            let shape =
                Shape { contents: tree.children.contains_key(&component), bodies: tree.bodies.contains_key(&component), folded: folds && numbered };
            let step = classify(view, frame.walk, placed.member.bom_type, shape);
            let mut path = frame.path.clone();
            path.push(placed.occurrence);
            let here = entries.len();
            if step.row {
                entries.push(Entry { path: path.clone(), member: placed.member, holds: step.holds, kit: frame.kit });
            }
            if let Some(walk) = step.inside {
                let kit = if step.kit { Some(here) } else { frame.kit };
                stack.push(Frame { component, path, walk, kit });
            }
        }
    }
    Ok(entries)
}

/// The rows `request` lists, with the assembly's total; with `request.mass`, each row and the
/// assembly weighed.
pub fn list(tree: &mut Tree, request: &Request) -> Result<Bom> {
    if request.source != 0 && !tree.components.contains_key(&request.source) {
        return Err(DocError::Unknown(format!("component {}", request.source)));
    }
    let entries = walk(tree, request)?;
    // What each folded placement holds: the placements listed as its children.
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    for (i, entry) in entries.iter().enumerate() {
        if let Some(of) = entry.kit.and_then(|k| kids.get_mut(k)) {
            of.push(i);
        }
    }
    let top: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.kit.is_none()).map(|(i, _)| i).collect();
    let mut bom = Bom { source: request.source, rows: tree.level(&entries, &kids, &top, request.group, &[], 1), total: Total::default() };
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
/// measured, has something to weigh, and they agree.
fn agree(weighed: &[Weighed]) -> Mass {
    let Some(known) = weighed.iter().copied().collect::<Option<Vec<(f64, usize)>>>() else {
        return Mass::Unknown(Unknown::Unmeasurable);
    };
    match known.first() {
        Some(_) if known.iter().any(|&(_, bodies)| bodies == 0) => Mass::Unknown(Unknown::NothingToWeigh),
        Some(&(first, _)) if known.iter().any(|&(grams, _)| (grams - first).abs() > 1e-9 * first.abs()) => Mass::Unknown(Unknown::Differs),
        Some(&(first, _)) => Mass::Grams(first),
        None => Mass::Unknown(Unknown::NothingToWeigh),
    }
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
    DocError::Invalid(format!("the assembly is deeper than {MAX_PLACEMENT_DEPTH} levels or has more than {MAX_PLACEMENTS} placements"))
}

#[cfg(test)]
#[path = "bom_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
