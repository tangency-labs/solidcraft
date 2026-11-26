//! Restore deleted material through its original leaf and rim identities.
use super::*;

const MAX_PICKS: usize = 256;
const MAX_HOLES: usize = 32;
const MAX_QUAD_CAPS: usize = 8;
const MAX_FILL_CELLS: usize = 4096;

/// A cap rim side in the new face winding, opposite the retained skin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuadCapSide {
    pub edge: [usize; 2],
    pub owner: usize,
}

/// One complete free opening, certified against the current evaluator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuadCapRim {
    pub sides: [QuadCapSide; 4],
    pub closes_skin: bool,
}

/// One complete logical edge in the retained skin's winding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeRimSide {
    pub edge: [usize; 2],
    pub owner: usize,
    /// Ordered evaluator vertices, including both logical endpoints.
    pub path: Vec<usize>,
}

// The reserved row is never an evaluator chart. Keeping it in the append-only
// leaf array gives the derived face the same stable ID space as ordinary faces.
const CAP_SLOT: Leaf = Leaf { root: 0, domain: [0; 4], active: false };

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QuadCap {
    pub(super) face: usize,
    sides: [QuadCapSide; 4],
}

pub(super) fn read_additional_quad_caps<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<QuadCap>, D::Error> {
    struct AdditionalCaps;
    impl<'de> serde::de::Visitor<'de> for AdditionalCaps {
        type Value = Vec<QuadCap>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("ordered additional quad cap records")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            use serde::de::Error;
            let mut caps: Vec<QuadCap> = Vec::new();
            while let Some(cap) = seq.next_element::<QuadCap>()? {
                if caps.len() >= MAX_QUAD_CAPS - 1 || cap.face >= MAX_LEAVES || caps.last().is_some_and(|old| old.face >= cap.face) {
                    return Err(A::Error::custom("invalid additional quad cap records"));
                }
                caps.push(cap);
            }
            Ok(caps)
        }
    }
    d.deserialize_seq(AdditionalCaps)
}

pub(super) fn read_retired_quad_caps<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<usize>, D::Error> {
    struct RetiredCaps;
    impl<'de> serde::de::Visitor<'de> for RetiredCaps {
        type Value = Vec<usize>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("ordered retired quad cap face IDs")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            use serde::de::Error;
            let mut ids = Vec::new();
            while let Some(id) = seq.next_element::<usize>()? {
                if id >= MAX_LEAVES || ids.last().is_some_and(|&last| id <= last) {
                    return Err(A::Error::custom("invalid retired quad cap face IDs"));
                }
                ids.push(id);
            }
            Ok(ids)
        }
    }
    d.deserialize_seq(RetiredCaps)
}

impl Hierarchy {
    pub fn has_quad_cap(&self) -> bool {
        self.quad_cap.is_some() || !self.additional_quad_caps.is_empty()
    }

    fn quad_caps(&self) -> impl Iterator<Item = &QuadCap> {
        self.quad_cap.iter().chain(&self.additional_quad_caps)
    }

    pub fn quad_cap_faces(&self) -> impl Iterator<Item = usize> + '_ {
        self.quad_caps().map(|cap| cap.face)
    }

    pub fn is_quad_cap_face(&self, face: usize) -> bool {
        self.quad_cap_faces().any(|id| id == face)
    }

    /// The sole cap face when a caller specifically requires one.
    pub fn quad_cap_face(&self) -> Option<usize> {
        self.quad_cap.as_ref().filter(|_| self.additional_quad_caps.is_empty()).map(|cap| cap.face)
    }

    pub fn can_reserve_quad_cap(&self) -> bool {
        self.quad_cap_faces().count() < MAX_QUAD_CAPS && self.leaves.len() < MAX_LEAVES
    }

    /// Materializing the skin into a new base cage would lose these slots.
    pub fn has_quad_cap_history(&self) -> bool {
        self.has_quad_cap() || !self.retired_quad_caps.is_empty()
    }

    pub(super) fn validate_quad_cap_slots(&self) -> Result<()> {
        if !self.has_quad_cap_history() {
            return Ok(());
        }
        let active: Vec<_> = self.quad_cap_faces().collect();
        if (!self.additional_quad_caps.is_empty() && self.quad_cap.is_none())
            || active.len() > MAX_QUAD_CAPS
            || active.windows(2).any(|pair| pair[0] >= pair[1])
            || self.retired_quad_caps.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(bad("invalid active or retired quad cap face slots"));
        }
        let reserved = self.leaves.iter().filter(|&leaf| leaf == &CAP_SLOT).count();
        let valid_slot = |&id| id >= self.base.faces.len() && self.leaves.get(id) == Some(&CAP_SLOT);
        if reserved != active.len() + self.retired_quad_caps.len()
            || !active.iter().chain(&self.retired_quad_caps).all(valid_slot)
            || active.iter().any(|id| self.retired_quad_caps.binary_search(id).is_ok())
        {
            return Err(bad("invalid active or retired quad cap face slots"));
        }
        Ok(())
    }

    pub(super) fn retire_quad_cap(&self, face: usize) -> Result<Self> {
        self.validated_quad_caps()?;
        let mut out = self.clone();
        if out.quad_cap.as_ref().is_some_and(|cap| cap.face == face) {
            out.quad_cap = (!out.additional_quad_caps.is_empty()).then(|| out.additional_quad_caps.remove(0));
        } else {
            let index = out.additional_quad_caps.binary_search_by_key(&face, |cap| cap.face).map_err(|_| bad("select an active quad cap face"))?;
            out.additional_quad_caps.remove(index);
        }
        let index = out.retired_quad_caps.partition_point(|&id| id < face);
        out.retired_quad_caps.insert(index, face);
        out.validate_quad_cap_slots()?;
        Ok(out)
    }

    /// Reserve one stable face and retain its exact directed attachment. The
    /// document layer qualifies the rebuilt CAD result before adoption.
    pub fn reserve_quad_cap(&mut self, edges: &[[usize; 2]], vertices: &[usize]) -> Result<usize> {
        self.check_quad_cap_slot()?;
        let rim = self.quad_cap_rim(edges, vertices)?;
        Ok(self.install_quad_cap(rim))
    }

    fn check_quad_cap_slot(&self) -> Result<()> {
        self.validate_quad_cap_slots()?;
        if !self.can_reserve_quad_cap() || (!self.has_quad_cap_history() && self.leaves.contains(&CAP_SLOT)) {
            return Err(bad("this Form cannot allocate another quad cap face"));
        }
        Ok(())
    }

    fn install_quad_cap(&mut self, rim: QuadCapRim) -> usize {
        let face = self.leaves.len();
        let cap = QuadCap { face, sides: rim.sides };
        if self.quad_cap.is_none() {
            self.quad_cap = Some(cap);
        } else {
            self.additional_quad_caps.push(cap);
        }
        self.leaves.push(CAP_SLOT);
        face
    }

    /// Replace one interior material disk with a derived cap. The immutable
    /// evaluator stays in place. Only the selected visible leaf owners retire.
    pub fn erase_and_cap(&self, faces: &[usize]) -> Result<Self> {
        self.check_quad_cap_slot()?;
        if !self.supports_hole_restoration() {
            return Err(bad("refined Erase and Fill needs original leaf charts without extrusion or trim walls"));
        }
        if faces.is_empty() || faces.len() > MAX_PICKS {
            return Err(bad("select 1..256 faces of one four-sided interior region"));
        }
        let source = self.skin_layout()?;
        let selected: BTreeSet<_> = faces.iter().copied().collect();
        if selected.len() != faces.len() || selected.iter().any(|&id| source.faces.get(id).is_none_or(Vec::is_empty)) {
            return Err(bad("select distinct active faces to erase and fill"));
        }
        let cells: BTreeSet<_> = source
            .owners
            .iter()
            .enumerate()
            .filter_map(|(cell, owner)| owner.filter(|id| selected.contains(id)).map(|_| cell))
            .take(MAX_FILL_CELLS + 1)
            .collect();
        if cells.is_empty() || cells.len() > MAX_FILL_CELLS {
            return Err(bad("the selected Erase and Fill region exceeds its evaluator cell budget"));
        }
        let (cage, _, _) = self.tags()?;
        let topology = cage.topology()?;
        let mut interface = BTreeSet::new();
        for &cell in &cells {
            let face = cage.faces.get(cell).ok_or_else(|| bad("missing Erase and Fill evaluator cell"))?;
            for (a, b) in pairs(face) {
                let edge = topology
                    .lookup
                    .get(&key(a, b))
                    .and_then(|&id| topology.edges.get(id))
                    .ok_or_else(|| bad("missing Erase and Fill interface edge"))?;
                let [first, second] = edge.faces.as_slice() else {
                    return Err(bad("the erased region must stay clear of existing openings"));
                };
                let other = if *first == cell { *second } else { *first };
                if !source.owners.get(other).is_some_and(Option::is_some) {
                    return Err(bad("the erased region must stay clear of existing openings"));
                }
                if !cells.contains(&other) && !interface.insert((a, b)) {
                    return Err(bad("ambiguous Erase and Fill interface"));
                }
            }
        }
        if interface.is_empty() {
            return Err(bad("the erased region must border retained faces"));
        }
        let mut out = self.delete_faces(faces)?;
        let skin = out.skin_layout()?;
        if skin.owners.len() != source.owners.len()
            || skin
                .owners
                .iter()
                .zip(&source.owners)
                .enumerate()
                .any(|(cell, (next, previous))| *next != if cells.contains(&cell) { None } else { *previous })
        {
            return Err(bad("Erase and Fill changed unrelated evaluator ownership"));
        }
        let rims = cage.boundary_loops_where(|cell| skin.owners.get(cell).is_some_and(Option::is_some))?;
        let mut matching = rims.iter().filter(|rim| rim.len() == interface.len() && pairs(rim).all(|(a, b)| interface.contains(&(b, a))));
        let rim =
            matching.next().filter(|_| matching.next().is_none()).ok_or_else(|| bad("the erased region must expose exactly one new complete rim"))?;
        let mut work = MAX_FILL_CELLS;
        if missing_disk(&cage, &topology, &skin.owners, rim, &mut work)? != cells {
            return Err(bad("the erased faces must form one complete material disk"));
        }
        let cap = Self::quad_cap_sides(&topology, &skin, rim, rims.len() == out.quad_cap_faces().count() + 1)?;
        out.install_quad_cap(cap);
        out.layout()?;
        Ok(out)
    }

    /// Re-resolve a saved cap against the current active material and its
    /// original owner IDs. Unrelated leaf allocation cannot retarget a side.
    pub fn validated_quad_cap(&self) -> Result<Option<(usize, QuadCapRim)>> {
        let mut caps = self.validated_quad_caps()?.into_iter();
        let first = caps.next();
        if caps.next().is_some() {
            return Err(bad("select one quad cap face"));
        }
        Ok(first)
    }

    pub fn validated_quad_caps(&self) -> Result<Vec<(usize, QuadCapRim)>> {
        if !self.has_quad_cap() {
            return Ok(Vec::new());
        }
        self.replay_quad_caps(&self.skin_layout()?)
    }

    pub(super) fn replay_quad_caps(&self, skin: &Layout) -> Result<Vec<(usize, QuadCapRim)>> {
        if !self.has_quad_cap() {
            return Ok(Vec::new());
        }
        if !self.supports_hole_restoration() {
            return Err(bad("invalid quad cap source charts"));
        }
        let (cage, _, _) = self.tags()?;
        let topology = cage.topology()?;
        let rims = cage.boundary_loops_where(|cell| skin.owners.get(cell).is_some_and(Option::is_some))?;
        // The boundary routine refuses shared or branching rim vertices.
        // A saved first corner therefore identifies one whole opening.
        let openings: BTreeMap<_, _> = rims.iter().enumerate().flat_map(|(index, rim)| rim.iter().map(move |&vertex| (vertex, index))).collect();
        let closes_skin = rims.len() == self.quad_cap_faces().count();
        let mut occupied = BTreeSet::new();
        self.quad_caps()
            .map(|cap| {
                let &index = openings.get(&cap.sides[0].edge[0]).ok_or_else(|| bad("saved quad cap rim is no longer free"))?;
                if !occupied.insert(index) {
                    return Err(bad("quad caps must occupy distinct openings"));
                }
                let rim = Self::quad_cap_sides(&topology, skin, &rims[index], closes_skin)
                    .map_err(|error| bad(&format!("saved quad cap rim cannot be replayed ({error})")))?;
                if rim.sides != cap.sides {
                    return Err(bad("topology change alters the saved quad cap side ownership"));
                }
                Ok((cap.face, rim))
            })
            .collect()
    }

    /// Whether the original rectangular chart representation is available.
    /// The selected rim is checked separately.
    pub fn supports_hole_restoration(&self) -> bool {
        self.trim_wall.is_none() && self.extrusions.is_empty() && !self.has_boundary_collapse()
    }

    /// Resolve a four-sided cap by exact directed active topology. Each
    /// logical side may cover multiple evaluator edges with the same owner.
    pub fn quad_cap_rim(&self, edges: &[[usize; 2]], vertices: &[usize]) -> Result<QuadCapRim> {
        if !self.supports_hole_restoration() {
            return Err(bad("a quad cap needs original leaf charts without an extrusion or trim wall"));
        }
        check_picks(edges, vertices)?;
        self.quad_cap_rim_in_skin(&self.skin_layout()?, edges, vertices)
    }

    fn quad_cap_rim_in_skin(&self, layout: &Layout, edges: &[[usize; 2]], vertices: &[usize]) -> Result<QuadCapRim> {
        let (cage, _, _) = self.tags()?;
        let PickedRims { selected, unselected } = picked_rims(&cage, layout, edges, vertices)?;
        let [rim] = selected.as_slice() else { return Err(bad("select exactly one opening for a quad cap")) };
        let caps = self.replay_quad_caps(layout)?;
        if caps.iter().any(|(_, cap)| rim.contains(&cap.sides[0].edge[0])) {
            return Err(bad("the selected rim already belongs to a quad cap"));
        }
        Self::quad_cap_sides(&cage.topology()?, layout, rim, unselected == caps.len())
    }

    fn quad_cap_sides(topology: &super::super::Topology, layout: &Layout, rim: &[usize], closes_skin: bool) -> Result<QuadCapRim> {
        if rim.len() > MAX_FILL_CELLS {
            return Err(bad("refined hole rim budget exceeded"));
        }
        let mut ordered = free_rim_sides(topology, layout, rim, 4, false)?;
        ordered.reverse();
        let sides = ordered
            .into_iter()
            .map(|side| QuadCapSide { edge: [side.edge[1], side.edge[0]], owner: side.owner })
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| bad("quad cap requires four complete logical rim sides"))?;
        Ok(QuadCapRim { sides, closes_skin })
    }

    /// Resolve two original free refined openings by the selected logical
    /// edges. Each returned pair follows its own skin winding. The first
    /// side's start corresponds to the second side's end.
    ///
    /// This establishes topology and chart ownership only. A caller must
    /// independently qualify coincidence of the native boundary curves.
    pub fn refined_weld_rims(&self, other: &Self, first_edge: [usize; 2], second_edge: [usize; 2]) -> Result<Vec<[FreeRimSide; 2]>> {
        let first = self.weld_rim(Some(first_edge))?;
        let mut second = other.weld_rim(Some(second_edge))?;
        if first.len() != second.len() {
            return Err(bad("Weld requires equally segmented logical rims"));
        }
        // Retain the selected seed pair and then advance in opposite winding.
        second.reverse();
        second.rotate_right(1);
        Ok(first.into_iter().zip(second).map(|(a, b)| [a, b]).collect())
    }

    /// The only original free opening of a Form, in skin winding order.
    pub fn single_refined_weld_rim(&self) -> Result<Vec<FreeRimSide>> {
        self.weld_rim(None)
    }

    fn weld_rim(&self, edge: Option<[usize; 2]>) -> Result<Vec<FreeRimSide>> {
        if !self.supports_hole_restoration() || self.has_quad_cap() {
            return Err(bad("refined Weld requires original skin charts without an extrusion, trim wall or cap"));
        }
        let layout = self.skin_layout()?;
        let (cage, _, _) = self.tags()?;
        let rims = match edge {
            Some(edge) => picked_rims(&cage, &layout, &[edge], &[])?.selected,
            None => cage.boundary_loops_where(|face| layout.owners.get(face).is_some_and(Option::is_some))?,
        };
        let [rim] = rims.as_slice() else { return Err(bad("select two Forms with one original free opening each")) };
        let mut sides = free_rim_sides(&cage.topology()?, &layout, rim, MAX_PICKS, true)?;
        if let Some([a, b]) = edge {
            let seed = sides
                .iter()
                .position(|side| key(side.edge[0], side.edge[1]) == key(a, b))
                .ok_or_else(|| bad("the selected Weld edge does not span a complete rim side"))?;
            sides.rotate_left(seed);
        }
        Ok(sides)
    }

    /// Restore the retained material under selected complete interior rims.
    /// `single` requires each opening to restore one logical quadrilateral.
    pub fn restore_deleted_holes(&self, edges: &[[usize; 2]], vertices: &[usize], single: bool) -> Result<Self> {
        self.restore_deleted_material(edges, vertices, single)?.ok_or_else(|| bad("this free rim has no retained material to fill"))
    }

    /// `None` means the selected complete rims consist exclusively of
    /// original free evaluator edges. Other unsupported or ambiguous rims
    /// return errors and never qualify as fresh openings through restoration.
    pub fn restore_deleted_material(&self, edges: &[[usize; 2]], vertices: &[usize], single: bool) -> Result<Option<Self>> {
        if !self.supports_hole_restoration() {
            return Err(bad("refined Fill Hole requires original leaf charts without an extrusion or trim wall"));
        }
        check_picks(edges, vertices)?;
        self.base.validate()?;
        let raw = self.raw_layout()?;
        let (cage, tags, levels) = self.tags()?;
        if self.details.len() != levels.len()
            || self.details.iter().zip(&levels).any(|(details, &count)| details.len() > count)
            || self.sharpness.len() > levels.len()
            || self.sharpness.iter().any(|level| {
                levels.get(level.level).is_none_or(|&count| {
                    level.vertices.len() > count || level.edges.len() > 4 * self.base.faces.len() * (1usize << (2 * level.level))
                })
            })
        {
            return Err(bad("invalid retained detail or crease budget"));
        }
        let (terminal, sizes) = self.retained_partition(&tags, &raw.owners)?;
        let layout = topology::dissolved_layout(raw, &self.dissolved_edges)?;
        let rims = picked_rims(&cage, &layout, edges, vertices)?.selected;
        if self.replay_quad_caps(&layout)?.iter().any(|(_, cap)| rims.iter().any(|rim| rim.contains(&cap.sides[0].edge[0]))) {
            return Err(bad("the selected rim already belongs to a quad cap"));
        }
        let topology = cage.topology()?;
        let mut original = false;
        let mut retained = false;
        for (a, b) in rims.iter().flat_map(|rim| pairs(rim)) {
            let edge =
                topology.lookup.get(&key(a, b)).and_then(|&id| topology.edges.get(id)).ok_or_else(|| bad("missing exact Fill Hole rim edge"))?;
            match edge.faces.as_slice() {
                [_] => original = true,
                [_, _] => retained = true,
                _ => return Err(bad("Fill Hole rim has ambiguous evaluator ownership")),
            }
        }
        if original {
            return if retained { Err(bad("the selected openings mix original free edges and deleted material")) } else { Ok(None) };
        }
        let mut restored = BTreeSet::new();
        let mut remaining = MAX_FILL_CELLS;
        for rim in rims {
            let cells = missing_disk(&cage, &topology, &layout.owners, &rim, &mut remaining)?;
            let mut leaves = BTreeMap::new();
            for cell in cells {
                let leaf = *terminal.get(cell).ok_or_else(|| bad("missing retained cell provenance"))?;
                *leaves.entry(leaf).or_insert(0usize) += 1;
            }
            if single && leaves.len() != 1 {
                return Err(bad("single-face filling requires one deleted quadrilateral leaf"));
            }
            for (leaf, count) in leaves {
                if self.detached_leaves.binary_search(&leaf).is_ok() {
                    return Err(bad("the selected material was detached by Unweld"));
                }
                if sizes.get(leaf) != Some(&count) || !restored.insert(leaf) {
                    return Err(bad("the rim does not enclose complete, distinct deleted leaves"));
                }
            }
        }
        let mut out = self.clone();
        let mut corners = BTreeMap::new();
        for &id in &restored {
            let leaf = out.leaves.get_mut(id).filter(|leaf| !leaf.active).ok_or_else(|| bad("missing deleted logical leaf"))?;
            leaf.active = true;
            let [u, v, x, y] = leaf.domain;
            for (s, t) in [(u, v), (x, v), (x, y), (u, y)] {
                corners.insert((leaf.root, s, t), None);
            }
        }
        for (face, (root, uv)) in cage.faces.iter().zip(&tags) {
            for (&vertex, &[u, v]) in face.iter().zip(uv) {
                if let Some(found) = corners.get_mut(&(*root, u, v))
                    && found.replace(vertex).is_some_and(|previous| previous != vertex)
                {
                    return Err(bad("ambiguous restored leaf corner"));
                }
            }
        }
        for corner in corners.into_values() {
            let vertex = corner.ok_or_else(|| bad("missing restored leaf corner"))?;
            let birth = levels.iter().position(|&count| vertex < count).ok_or_else(|| bad("missing restored control level"))?;
            out.controls.entry(vertex).or_insert(birth);
        }
        let filled = out.layout()?;
        if single && restored.iter().any(|&id| filled.faces.get(id).is_none_or(|face| face.len() != 4)) {
            return Err(bad("single-face filling requires four logical rim edges"));
        }
        Ok(Some(out))
    }

    // Subdivision appends children after their retired parent. Deletion
    // switches active flags, while dissolve changes only visible ownership.
    // Require a complete nested partition before treating any inactive slot
    // as missing material. This also rejects ambiguous saved leaf histories.
    fn retained_partition(&self, tags: &[Tag], active_owners: &[Option<usize>]) -> Result<(Vec<usize>, Vec<usize>)> {
        let side = 1usize << self.depth;
        let step = GRID / side;
        let roots = self.base.faces.len();
        let per_root = side * side;
        let count = roots
            .checked_mul(per_root)
            .filter(|&count| count == tags.len() && count == active_owners.len())
            .ok_or_else(|| bad("original leaf charts do not cover the evaluator"))?;
        let mut tiles: Vec<Option<usize>> = vec![None; count];
        let mut sizes = vec![0; self.leaves.len()];
        let tile = |root: usize, u: usize, v: usize| root * per_root + (v / step) * side + u / step;
        for (id, leaf) in self.leaves.iter().enumerate() {
            if leaf == &CAP_SLOT && (self.is_quad_cap_face(id) || self.retired_quad_caps.binary_search(&id).is_ok()) {
                continue;
            }
            let [u, v, x, y] = leaf.domain;
            if leaf.root >= roots || u >= x || v >= y || x > GRID || y > GRID || leaf.domain.iter().any(|n| !n.is_multiple_of(step)) {
                return Err(bad("invalid retained leaf domain"));
            }
            if id < roots && (leaf.root != id || leaf.domain != [0, 0, GRID, GRID]) {
                return Err(bad("missing original root leaf"));
            }
            let mut parent = None;
            for t in (v..y).step_by(step) {
                for s in (u..x).step_by(step) {
                    let slot = tiles.get_mut(tile(leaf.root, s, t)).ok_or_else(|| bad("missing retained evaluator tile"))?;
                    if id >= roots {
                        let previous = (*slot).ok_or_else(|| bad("a child leaf has no original parent"))?;
                        let old = self.leaves.get(previous).ok_or_else(|| bad("missing retained parent leaf"))?;
                        let [a, b, c, d] = old.domain;
                        if old.active
                            || old.domain == leaf.domain
                            || a > u
                            || b > v
                            || c < x
                            || d < y
                            || parent.replace(previous).is_some_and(|other| other != previous)
                        {
                            return Err(bad("retained leaves do not describe a complete subdivision history"));
                        }
                    }
                    *slot = Some(id);
                }
            }
        }
        let mut owners = Vec::with_capacity(count);
        for ((root, uv), active) in tags.iter().zip(active_owners) {
            let u = uv.iter().map(|p| p[0]).min().ok_or_else(|| bad("empty evaluator cell"))?;
            let v = uv.iter().map(|p| p[1]).min().ok_or_else(|| bad("empty evaluator cell"))?;
            if *root >= roots || u >= GRID || v >= GRID || !u.is_multiple_of(step) || !v.is_multiple_of(step) {
                return Err(bad("invalid original evaluator chart"));
            }
            let owner = tiles.get_mut(tile(*root, u, v)).and_then(Option::take).ok_or_else(|| bad("missing terminal leaf for evaluator cell"))?;
            let leaf = self.leaves.get(owner).ok_or_else(|| bad("missing terminal leaf"))?;
            if leaf.active.then_some(owner) != *active {
                return Err(bad("terminal leaves disagree with active material"));
            }
            *sizes.get_mut(owner).ok_or_else(|| bad("missing retained leaf size"))? += 1;
            owners.push(owner);
        }
        for (leaf, &size) in self.leaves.iter().zip(&sizes) {
            let [u, v, x, y] = leaf.domain;
            if size != 0 && size != ((x - u) / step) * ((y - v) / step) {
                return Err(bad("a retired parent is only partially covered by its children"));
            }
        }
        Ok((owners, sizes))
    }
}

fn check_picks(edges: &[[usize; 2]], vertices: &[usize]) -> Result<()> {
    if edges.len() > MAX_PICKS || vertices.len() > MAX_PICKS || (edges.is_empty() && vertices.is_empty()) {
        return Err(bad("select 1..256 logical rim edges or points to fill"));
    }
    Ok(())
}

struct PickedRims {
    selected: Vec<Vec<usize>>,
    unselected: usize,
}

fn picked_rims(cage: &Cage, layout: &Layout, edges: &[[usize; 2]], vertices: &[usize]) -> Result<PickedRims> {
    let mut segments = BTreeSet::new();
    let mut points: BTreeSet<_> = vertices.iter().copied().collect();
    if !points.is_empty() {
        let visible: BTreeSet<_> = layout.faces.iter().flatten().copied().collect();
        if !points.is_subset(&visible) {
            return Err(bad("pick visible logical rim points"));
        }
    }
    for &[a, b] in edges {
        let edge = [a.min(b), a.max(b)];
        let at = layout.paths.binary_search_by_key(&edge, |(id, _)| *id).map_err(|_| bad("pick a visible logical rim edge"))?;
        let (_, path) = layout.paths.get(at).filter(|(_, path)| path.len() >= 2).ok_or_else(|| bad("missing logical rim path"))?;
        segments.extend(path.iter().copied().zip(path.iter().copied().skip(1)).map(|(a, b)| key(a, b)));
    }
    let loops = cage.boundary_loops_where(|face| layout.owners.get(face).is_some_and(Option::is_some))?;
    let mut picked = Vec::new();
    let mut unselected = 0;
    for rim in loops {
        let mut selected = false;
        for (a, b) in pairs(&rim) {
            selected |= segments.remove(&key(a, b));
            selected |= points.remove(&a);
        }
        if selected {
            if rim.len() > MAX_FILL_CELLS || picked.len() >= MAX_HOLES {
                return Err(bad("refined hole rim budget exceeded"));
            }
            picked.push(rim);
        } else {
            unselected += 1;
        }
    }
    if picked.is_empty() || !segments.is_empty() || !points.is_empty() {
        return Err(bad("select edges or points on complete free rims"));
    }
    Ok(PickedRims { selected: picked, unselected })
}

fn free_rim_sides(
    topology: &super::super::Topology,
    layout: &Layout,
    rim: &[usize],
    max_sides: usize,
    original_only: bool,
) -> Result<Vec<FreeRimSide>> {
    let next: BTreeMap<_, _> = pairs(rim).collect();
    let mut covered = BTreeSet::new();
    let mut sides = BTreeMap::new();
    for (edge, path) in &layout.paths {
        let [first, second, ..] = path.as_slice() else { continue };
        let in_rim = |a, b| next.get(&a) == Some(&b);
        let reversed = if in_rim(*first, *second) {
            false
        } else if in_rim(*second, *first) {
            true
        } else {
            continue;
        };
        if sides.len() >= max_sides {
            return Err(bad("opening exceeds its logical rim side budget"));
        }
        let directed = |w: &[usize]| if reversed { (w[1], w[0]) } else { (w[0], w[1]) };
        let edge = if reversed { [edge[1], edge[0]] } else { *edge };
        let [a, b] = edge;
        let mut owner = None;
        for pair in path.windows(2) {
            let (from, to) = directed(pair);
            if !in_rim(from, to) {
                return Err(bad("logical side only partly follows the selected opening"));
            }
            let uses = topology.lookup.get(&key(from, to)).and_then(|&e| topology.edges.get(e)).ok_or_else(|| bad("missing evaluator rim edge"))?;
            if original_only && uses.faces.len() != 1 {
                return Err(bad("refined Weld requires an original free rim rather than a cut through retained material"));
            }
            let mut active = uses.faces.iter().filter_map(|&cell| layout.owners.get(cell).copied().flatten());
            let cell_owner = active.next().filter(|_| active.next().is_none()).ok_or_else(|| bad("rim side must be free"))?;
            if !covered.insert(from) || owner.replace(cell_owner).is_some_and(|old| old != cell_owner) {
                return Err(bad("rim side has ambiguous retained ownership"));
            }
        }
        let owner = owner.ok_or_else(|| bad("rim side has no retained owner"))?;
        let face = layout.faces.get(owner).ok_or_else(|| bad("missing rim side owner"))?;
        if !pairs(face).any(|pair| pair == (a, b)) {
            return Err(bad("rim winding disagrees with the retained skin"));
        }
        let mut path = path.clone();
        if reversed {
            path.reverse();
        }
        if path.first() != Some(&a) || path.last() != Some(&b) || sides.insert(a, FreeRimSide { edge, owner, path }).is_some() {
            return Err(bad("logical side has ambiguous rim endpoints"));
        }
    }
    if covered.len() != next.len() {
        return Err(bad("opening does not have complete logical rim sides"));
    }
    let start = *sides.first_key_value().ok_or_else(|| bad("empty logical rim"))?.0;
    let mut at = start;
    let mut ordered = Vec::with_capacity(sides.len());
    while !sides.is_empty() {
        let side = sides.remove(&at).ok_or_else(|| bad("logical rim does not form one cycle"))?;
        at = side.edge[1];
        ordered.push(side);
    }
    if at != start {
        return Err(bad("logical rim is open"));
    }
    Ok(ordered)
}

fn pairs(face: &[usize]) -> impl Iterator<Item = (usize, usize)> + '_ {
    face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(face.len())
}

fn missing_disk(
    cage: &Cage,
    topology: &super::super::Topology,
    owners: &[Option<usize>],
    rim: &[usize],
    remaining: &mut usize,
) -> Result<BTreeSet<usize>> {
    let first = pairs(rim).next().ok_or_else(|| bad("missing selected rim"))?;
    let edge = |(a, b)| topology.lookup.get(&key(a, b)).and_then(|&id| topology.edges.get(id)).ok_or_else(|| bad("missing evaluator rim edge"));
    let start = edge(first)?
        .faces
        .iter()
        .copied()
        .find(|&id| owners.get(id) == Some(&None))
        .ok_or_else(|| bad("this free rim has no retained material to fill"))?;
    let mut boundary: BTreeSet<_> = pairs(rim).map(|(a, b)| (b, a)).collect();
    let mut pending = vec![start];
    let mut cells = BTreeSet::new();
    let mut vertices = BTreeSet::new();
    let mut inner_edges = 0;
    while let Some(cell) = pending.pop() {
        if !cells.insert(cell) {
            continue;
        }
        *remaining = remaining.checked_sub(1).ok_or_else(|| bad("refined hole material budget exceeded"))?;
        let face = cage.faces.get(cell).ok_or_else(|| bad("missing deleted evaluator cell"))?;
        for (a, b) in pairs(face) {
            vertices.insert(a);
            let [first, second] = edge((a, b))?.faces.as_slice() else {
                return Err(bad("deleted material meets a pre-existing free boundary"));
            };
            let other = if *first == cell { *second } else { *first };
            match owners.get(other) {
                Some(None) => {
                    inner_edges += usize::from(cell < other);
                    if !cells.contains(&other) {
                        pending.push(other);
                    }
                }
                Some(Some(_)) if boundary.remove(&(a, b)) => {}
                _ => return Err(bad("deleted material has another rim or incompatible winding")),
            }
        }
    }
    if !boundary.is_empty() || vertices.len() + cells.len() != inner_edges + rim.len() + 1 {
        return Err(bad("the selected rim does not enclose one deleted material disk"));
    }
    Ok(cells)
}
