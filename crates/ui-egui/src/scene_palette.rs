//! Stable palette slots for the model scene.
//!
//! Cached block bytes index a scene palette (`solidcraft_viewport_gpu::view::Palette`). A body's faces,
//! edges and placement keep their slots while the scene rebuilds, so an appearance or theme edit
//! changes only palette entries — never a block's vertex bytes. Slots are append-only for the
//! session and are reset when a different design replaces the one they belonged to.
//!
//! Each body also gets a slot of the renderer's motion table (`solidcraft_viewport_gpu::view::Motions`),
//! named by every one of its palette entries: whatever is drawn in a body's colours can then be
//! drawn moved by writing that slot's motion, without building anything again. A design with more
//! bodies than the table has slots leaves the rest without one (`BodySlots::motion` 0).
//!
//! Copies of a part placed many times that look alike share their face entries
//! ([`PaletteArena::shared`]): an entry per face of every copy filled the palette. Each copy still
//! has its own colour, edge, hidden-edge and placement entries and its own motion slot, which
//! those entries name and its instanced placements carry, so a joint drag moves one copy and not
//! the others. The shared face entries name no motion: a copy drawn from its own triangles in
//! them cannot be drawn moved ([`PaletteArena::motion`]).
//!
//! Entry 0 belongs to no body. The renderer and a full arena use it as the stand-in (the ground
//! the ambient occlusion darkens, a body past the cap), and a stand-in must not move with
//! whichever body was allocated first.
//!
//! The model scene needs at most one stable slot per visible body face (plus a few per body), so
//! the arena is capped by [`MAX_PALETTE`]. Past the cap a face falls back to its body's slot:
//! editing such a face once rebuilds that body's block (the build still succeeds).

use std::collections::HashMap;

use crate::gpu::{MAX_MOTIONS, MAX_PALETTE, Material, PaletteIndex, UNMOVED_SLOT};

/// The stable slots of one body: its own colour, its face range, its edges and its instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BodySlots {
    /// The body's own colour: the fallback for faces past the allocated range.
    pub body: PaletteIndex,
    /// The first face slot, or [`u32::MAX`] when the arena is full (every face uses `body`).
    pub faces: u32,
    pub face_capacity: u32,
    pub edge: PaletteIndex,
    pub hidden_edge: PaletteIndex,
    pub instance: PaletteIndex,
    /// The body's slot in the motion table, which all its own palette entries name (not the face
    /// entries it shares with other copies, [`PaletteArena::shared`]); [`UNMOVED_SLOT`] when the
    /// table had none left or the arena was full: it cannot be drawn moved.
    pub motion: u32,
}

impl BodySlots {
    /// The slot of face `face`, or the body's slot when the face has none.
    pub fn face(&self, face: usize) -> PaletteIndex {
        if self.faces == u32::MAX || face >= self.face_capacity as usize {
            return self.body;
        }
        PaletteIndex(self.faces.saturating_add(face as u32))
    }
}

#[derive(Default)]
pub(crate) struct PaletteArena {
    entries: Vec<[u8; 4]>,
    bodies: HashMap<String, BodySlots>,
    /// The bodies whose face slots are another key's ([`Self::shared`]), by name.
    shared: std::collections::HashSet<String>,
    /// The shared bodies the last scene drew from their own triangles in the shared face slots
    /// ([`Self::drawn_in_shared`]): nothing moves them with their motion.
    unmoved: std::collections::HashSet<String>,
    /// PBR finishes by slot (none past its end): the model and highlight scenes both draw them.
    materials: Vec<Material>,
    /// The motion slot each entry names (none past its end), and how many slots bodies have taken.
    motions: Vec<u32>,
    motion_slots: u32,
    /// Bumped when the arena is reset (a different design); cached blocks must rebuild then.
    generation: u64,
}

/// The palette entry a whole-body hover is drawn in: reserved at a fixed index (never
/// allocated to a body), so recolouring a hovered placement is a word in its instance row
/// rather than a copy of its geometry.
pub const HOVER_ENTRY: u32 = 1;

/// The colour a whole-body hover is drawn in, as a palette entry's bytes: the theme's hover lift,
/// the same colour the highlight pass tints with (`Frame::hover_tint`).
pub fn hover_rgba(t: &solidcraft_theme::Tokens) -> [u8; 4] {
    [t.hover_face_lift, t.hover_face_lift, t.hover_face_lift, 255]
}

impl PaletteArena {
    /// The reserved hover entry ([`HOVER_ENTRY`]); its colour is set by [`Self::set_hover_colour`].
    pub fn hover_entry(&self) -> [u8; 4] {
        self.entries.get(HOVER_ENTRY as usize).copied().unwrap_or([0; 4])
    }

    /// Set the hover entry's colour without interning: a fixed index, so every scene built from
    /// this arena (the model's and the highlight's) has the same entry.
    pub fn set_hover_colour(&mut self, rgba: [u8; 4]) {
        if self.entries.len() <= HOVER_ENTRY as usize {
            self.entries.resize(HOVER_ENTRY as usize + 1, [0; 4]);
        }
        if let Some(e) = self.entries.get_mut(HOVER_ENTRY as usize) {
            *e = rgba;
        }
    }

    /// The stable entries, copied into the scene's palette.
    pub fn entries(&self) -> Vec<[u8; 4]> {
        self.entries.clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The slots of `name`, allocating them (and `face_count` face slots) the first time. A body
    /// that shared another key's slots until now ([`Self::shared`]) gets its own.
    pub fn body(&mut self, name: &str, face_count: usize) -> BodySlots {
        if self.shared.remove(name) {
            self.unmoved.remove(name);
            // Its own colour, edge and placement slots and its motion stay; only its faces get
            // slots of their own (a new motion slot each time would use the table up).
            if let Some(base) = self.bodies.remove(name) {
                let slots = self.grow_faces(&BodySlots { faces: u32::MAX, face_capacity: 0, ..base }, face_count);
                self.bodies.insert(name.to_string(), slots);
                return slots;
            }
        }
        self.own(name, face_count, true)
    }

    /// Body `name` drawn in the face slots of `key` (allocated the first time, as
    /// [`Self::body`]): copies of one part placed many times share one face range, which a slot
    /// per face of every placement would exhaust. Its colour, edge, hidden-edge and placement
    /// slots and its motion slot stay its own (kept from its own slots when it had them), so it
    /// is drawn moved apart from the other copies. Looking `name` up ([`Self::slots`]) finds them.
    pub fn shared(&mut self, name: &str, key: &str, face_count: usize) -> BodySlots {
        // The key is no body: its faces name no motion.
        let faces = self.own(key, face_count, false);
        let base = match self.bodies.get(name).copied() {
            Some(slots) => slots,
            None => self.base(true),
        };
        let slots = BodySlots { faces: faces.faces, face_capacity: faces.face_capacity, ..base };
        if self.bodies.get(name) != Some(&slots) {
            self.bodies.insert(name.to_string(), slots);
        }
        // Most rebuilds find every copy already there: no name copied for each.
        if !self.shared.contains(name) {
            self.shared.insert(name.to_string());
        }
        slots
    }

    /// Whether the scene draws shared body `name` from its own triangles in the shared face slots
    /// (`yes`), or otherwise (its placements, or its own colour): for [`Self::motion`].
    pub fn drawn_in_shared(&mut self, name: &str, yes: bool) {
        if yes && self.shared.contains(name) {
            if !self.unmoved.contains(name) {
                self.unmoved.insert(name.to_string());
            }
        } else if !self.unmoved.is_empty() {
            self.unmoved.remove(name);
        }
    }

    /// The motion slot that draws all of body `name` moved; [`UNMOVED_SLOT`] when it has none or
    /// is drawn in face slots it shares with other copies, which name no motion.
    pub fn motion(&self, name: &str) -> u32 {
        if self.unmoved.contains(name) {
            return UNMOVED_SLOT;
        }
        self.bodies.get(name).map_or(UNMOVED_SLOT, |s| s.motion)
    }

    /// The slots of `name` (a body, or a key whose face slots copies share: `moves` false, no
    /// motion slot), allocated the first time.
    fn own(&mut self, name: &str, face_count: usize, moves: bool) -> BodySlots {
        if let Some(slots) = self.bodies.get(name).copied() {
            let grown = self.grow_faces(&slots, face_count);
            // Written back only when it grew: most rebuilds find every body as it was.
            if grown != slots
                && let Some(kept) = self.bodies.get_mut(name)
            {
                *kept = grown;
            }
            return grown;
        }
        let slots = self.base(moves);
        let slots = self.grow_faces(&slots, face_count);
        self.bodies.insert(name.to_string(), slots);
        slots
    }

    /// New colour, edge, hidden-edge and placement slots, no faces yet, with a motion slot of
    /// their own that they name when `moves`.
    fn base(&mut self, moves: bool) -> BodySlots {
        // Entries 0 and 1 are reserved: 0 is the stand-in (kept before the first body's
        // entries, never named by a slot), 1 is the whole-body hover colour
        // ([`PaletteArena::hover_entry`]), whose index must not depend on how many entries a
        // scene happens to hold, so it is always there and always the same.
        if self.entries.is_empty() {
            self.entries.push([0; 4]);
            self.entries.push([0; 4]);
        }
        // A body whose entries are its own gets a motion slot; one that has to share the stand-in
        // (the arena full) gets none, so it is drawn where it was built.
        let own = self.entries.len() + 4 <= MAX_PALETTE;
        let motion = if moves && own && self.motion_slots + 1 < MAX_MOTIONS {
            self.motion_slots += 1;
            self.motion_slots
        } else {
            UNMOVED_SLOT
        };
        let body = self.push();
        let edge = self.push();
        let hidden_edge = self.push();
        let instance = self.push();
        if motion != UNMOVED_SLOT {
            for slot in [body, edge, hidden_edge, instance] {
                self.name_motion(slot.0 as usize, 1, motion);
            }
        }
        BodySlots { body, faces: u32::MAX, face_capacity: 0, edge, hidden_edge, instance, motion }
    }

    /// The `count` entries from `first` name motion slot `motion`.
    fn name_motion(&mut self, first: usize, count: usize, motion: u32) {
        let end = first.saturating_add(count).min(self.entries.len());
        if motion == UNMOVED_SLOT || first >= end {
            return;
        }
        if self.motions.len() < end {
            self.motions.resize(end, UNMOVED_SLOT);
        }
        if let Some(named) = self.motions.get_mut(first..end) {
            named.fill(motion);
        }
    }

    /// The motion slot each stable entry names, for the scene's palette.
    pub fn motions(&self) -> Vec<u32> {
        self.motions.clone()
    }

    /// Extend a body's face range when its face count grew. The range is reused when it is at the
    /// end of the arena; otherwise a new range is appended (the old one is left unused rather
    /// than moving an index cached blocks may refer to).
    fn grow_faces(&mut self, slots: &BodySlots, face_count: usize) -> BodySlots {
        let want = u32::try_from(face_count).unwrap_or(u32::MAX);
        if slots.faces != u32::MAX && slots.face_capacity >= want {
            return *slots;
        }
        let room = MAX_PALETTE.saturating_sub(self.entries.len());
        let need = usize::try_from(want).unwrap_or(usize::MAX).saturating_sub(slots.face_capacity as usize);
        let at_end = slots.faces != u32::MAX && slots.faces as usize + slots.face_capacity as usize == self.entries.len();
        if at_end && need <= room {
            let first = self.entries.len();
            self.entries.resize(first + need, [0; 4]);
            self.name_motion(first, need, slots.motion);
            return BodySlots { face_capacity: want, ..*slots };
        }
        if want as usize <= room {
            let first = self.entries.len();
            let base = u32::try_from(first).unwrap_or(u32::MAX);
            self.entries.resize(first + want as usize, [0; 4]);
            self.name_motion(first, want as usize, slots.motion);
            return BodySlots { faces: base, face_capacity: want, ..*slots };
        }
        // Out of room: every face falls back to the body's own slot.
        BodySlots { faces: u32::MAX, face_capacity: 0, ..*slots }
    }

    /// Append a zero entry; [`PaletteIndex(0)`] when the arena is full (never a panic).
    fn push(&mut self) -> PaletteIndex {
        if self.entries.len() >= MAX_PALETTE {
            return PaletteIndex(0);
        }
        let i = u32::try_from(self.entries.len()).unwrap_or(0);
        self.entries.push([0; 4]);
        PaletteIndex(i)
    }

    /// The slots of `name`, if it has any (nothing is allocated).
    pub fn slots(&self, name: &str) -> Option<BodySlots> {
        self.bodies.get(name).copied()
    }

    /// The colour at `slot`, if it is allocated.
    pub fn get(&self, slot: PaletteIndex) -> Option<[u8; 4]> {
        self.entries.get(slot.0 as usize).copied()
    }

    /// Set a slot's colour; out-of-range slots are ignored.
    pub fn set(&mut self, slot: PaletteIndex, rgba: [u8; 4]) {
        if let Some(entry) = self.entries.get_mut(slot.0 as usize) {
            *entry = rgba;
        }
    }

    /// Set a slot's PBR finish (`Material::default()`: none). The list grows only for a real
    /// finish, so a design without one keeps it empty.
    pub fn set_material(&mut self, slot: PaletteIndex, m: Material) {
        let at = slot.0 as usize;
        if at >= self.materials.len() {
            if !m.pbr || at >= self.entries.len() {
                return;
            }
            self.materials.resize(at + 1, Material::default());
        }
        if let Some(entry) = self.materials.get_mut(at) {
            *entry = m;
        }
    }

    /// The PBR finishes by slot, for the scene's palette.
    pub fn materials(&self) -> Vec<Material> {
        self.materials.clone()
    }

    /// Drop every body when the scene belongs to a different design (no body name survives).
    /// A design that removed every body and then adds one keeps its slots (the empty set does
    /// not reset the arena).
    pub fn sync<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        if self.bodies.is_empty() {
            return;
        }
        let mut names = names.into_iter().peekable();
        if names.peek().is_none() {
            return;
        }
        if !names.any(|n| self.bodies.contains_key(n)) {
            self.entries.clear();
            self.materials.clear();
            self.motions.clear();
            self.motion_slots = 0;
            self.bodies.clear();
            self.shared.clear();
            self.unmoved.clear();
            self.generation = self.generation.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_survive_rebuilds_and_faces_fall_back_at_the_cap() {
        let mut arena = PaletteArena::default();
        let first = arena.body("A", 2);
        let again = arena.body("A", 2);
        assert_eq!(first, again);
        assert_ne!(first.face(0), first.face(1));
        assert_eq!(first.face(2), first.body);
        arena.set(first.face(0), [1, 2, 3, 4]);
        assert_eq!(arena.entries()[first.face(0).0 as usize], [1, 2, 3, 4]);
        // A grown face count reuses the range at the end of the arena.
        let grown = arena.body("A", 4);
        assert_eq!(grown.faces, first.faces);
        assert_ne!(grown.face(3), grown.body);
        // A plain slot leaves the finishes empty; a real one grows them to its slot, no further.
        arena.set_material(grown.face(3), Material::default());
        assert!(arena.materials().is_empty());
        let brass = Material { metallic: 1.0, roughness: 0.3, pbr: true, ..Default::default() };
        arena.set_material(grown.face(3), brass);
        arena.set_material(PaletteIndex(u32::MAX), brass);
        assert_eq!(arena.materials().len(), grown.face(3).0 as usize + 1);
        assert_eq!(arena.materials().last(), Some(&brass));
        // A new design with no shared names resets the slots.
        let generation = arena.generation();
        arena.sync(["B"]);
        assert_eq!(arena.generation(), generation.wrapping_add(1));
        assert!(arena.entries().is_empty() && arena.materials().is_empty());
        // An empty scene does not reset.
        let before = arena.generation();
        arena.sync([]);
        assert_eq!(arena.generation(), before);
    }

    /// Each body gets a motion slot of its own that all its entries name, its faces added later
    /// too; another design starts the slots again; a body that has to share entries (the arena
    /// full) or comes after the table's last slot gets none, so it is never moved with another.
    #[test]
    fn each_body_names_its_own_motion_slot_in_all_its_entries() {
        let mut arena = PaletteArena::default();
        let (a, b) = (arena.body("A", 2), arena.body("B", 1));
        assert!(a.motion != UNMOVED_SLOT && b.motion != UNMOVED_SLOT && a.motion != b.motion);
        let named = |arena: &PaletteArena, slot: PaletteIndex| arena.motions().get(slot.0 as usize).copied().unwrap_or(UNMOVED_SLOT);
        for slot in [a.body, a.edge, a.hidden_edge, a.instance, a.face(0), a.face(1)] {
            assert_eq!(named(&arena, slot), a.motion);
        }
        for slot in [b.body, b.edge, b.hidden_edge, b.instance, b.face(0)] {
            assert_eq!(named(&arena, slot), b.motion);
        }
        // A's faces grow past B's entries: the new range names A's slot, and A keeps it.
        let grown = arena.body("A", 5);
        assert_eq!(grown.motion, a.motion);
        assert!((0..5).all(|f| named(&arena, grown.face(f)) == a.motion));
        // Another design: the slots start again.
        arena.sync(["C"]);
        assert!(arena.motions().is_empty());
        assert_eq!(arena.body("C", 1).motion, a.motion);
        // Entry 0 is no body's: the stand-in names no slot, whichever body came first.
        assert!(a.body.0 != 0 && named(&arena, PaletteIndex(0)) == UNMOVED_SLOT);
        // A full arena beside a real first body: the late body shares the stand-in, gets no slot,
        // and so is not moved with the first.
        let mut full = PaletteArena::default();
        let first = full.body("First", 1);
        full.entries.resize(MAX_PALETTE, [0; 4]);
        let late = full.body("Big", 3);
        assert_eq!((late.motion, late.body), (UNMOVED_SLOT, PaletteIndex(0)));
        assert_ne!(first.motion, UNMOVED_SLOT);
        assert_eq!(named(&full, late.body), UNMOVED_SLOT, "the stand-in entry names no slot");
        // The table's last slot taken: the next body gets none.
        let mut last = PaletteArena { motion_slots: MAX_MOTIONS - 1, ..Default::default() };
        let late = last.body("Late", 1);
        assert_eq!(late.motion, UNMOVED_SLOT);
        assert!(last.motions().is_empty());
    }

    #[test]
    fn the_cap_falls_back_to_the_body_slot_without_panicking() {
        // The arena cannot allocate past MAX_PALETTE; simulate a full one by filling entries.
        let mut arena = PaletteArena { entries: vec![[0; 4]; MAX_PALETTE], ..Default::default() };
        let slots = arena.body("Big", 10);
        assert_eq!(slots.faces, u32::MAX);
        assert_eq!(slots.face(3), slots.body);
        // Every push after the cap is index 0, never an out-of-range write.
        assert_eq!(arena.push(), PaletteIndex(0));
    }

    /// Placements of one part that look the same share one face range, found by each
    /// placement's name, and keep their own colour, edge and placement slots and motion: a
    /// placement takes four slots however many faces the part has. A placement that stops sharing
    /// gets slots of its own.
    #[test]
    fn placements_share_their_parts_face_slots_until_one_looks_different() {
        let mut arena = PaletteArena::default();
        let a = arena.shared("Base (Bracket:1)", "part", 30);
        let b = arena.shared("Base (Bracket:2)", "part", 30);
        assert_eq!((a.faces, a.face_capacity), (b.faces, b.face_capacity));
        assert_eq!(arena.shared("Base (Bracket:1)", "part", 30), a, "found again as it was");
        let used = arena.entries().len();
        for k in 3..100 {
            arena.shared(&format!("Base (Bracket:{k})"), "part", 30);
        }
        assert_eq!(arena.entries().len(), used + 4 * 97, "more placements take four slots each, not one per face");
        assert_eq!(arena.slots("Base (Bracket:2)"), Some(b));
        let own = arena.body("Base (Bracket:2)", 30);
        assert_ne!(own.faces, a.faces);
        assert_eq!(arena.slots("Base (Bracket:1)"), Some(a), "the others still share");
    }

    /// Each copy sharing a part's faces is moved on its own (a joint drag moves one copy, not
    /// all of them): its own entries name a motion slot of its own, the shared faces name none.
    /// One drawn in those shared faces cannot be drawn moved at all, until it is drawn otherwise
    /// or stops sharing.
    #[test]
    fn copies_sharing_their_faces_each_have_their_own_motion() {
        let mut arena = PaletteArena::default();
        let a = arena.shared("Screw (Carriage:1)", "screw", 4);
        let b = arena.shared("Screw (Frame:1)", "screw", 4);
        assert!(a.motion != UNMOVED_SLOT && b.motion != UNMOVED_SLOT && a.motion != b.motion);
        assert!([a.body, a.edge, a.hidden_edge, a.instance].iter().all(|e| ![b.body, b.edge, b.hidden_edge, b.instance].contains(e)));
        let named = |arena: &PaletteArena, slot: PaletteIndex| arena.motions().get(slot.0 as usize).copied().unwrap_or(UNMOVED_SLOT);
        for (copy, slots) in [(a.motion, a), (b.motion, b)] {
            assert!([slots.body, slots.edge, slots.hidden_edge, slots.instance].iter().all(|e| named(&arena, *e) == copy));
        }
        assert!((0..4).all(|f| named(&arena, a.face(f)) == UNMOVED_SLOT), "the shared faces move no copy");
        assert_eq!(arena.motion("Screw (Carriage:1)"), a.motion);
        // Drawn in the shared faces (its own triangles, faces of their own looks): not moved.
        arena.drawn_in_shared("Screw (Carriage:1)", true);
        assert_eq!(arena.motion("Screw (Carriage:1)"), UNMOVED_SLOT);
        assert_eq!(arena.motion("Screw (Frame:1)"), b.motion, "the other copy is not");
        arena.drawn_in_shared("Screw (Carriage:1)", false);
        assert_eq!(arena.motion("Screw (Carriage:1)"), a.motion);
        // A body of its own is never held still that way, nor one that stops sharing.
        let own = arena.body("Bracket", 2);
        arena.drawn_in_shared("Bracket", true);
        assert_eq!(arena.motion("Bracket"), own.motion);
        arena.drawn_in_shared("Screw (Carriage:1)", true);
        let alone = arena.body("Screw (Carriage:1)", 4);
        assert_eq!(arena.motion("Screw (Carriage:1)"), alone.motion);
        assert_ne!(alone.motion, UNMOVED_SLOT);
    }
}
