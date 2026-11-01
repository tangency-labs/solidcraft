//! Stable palette slots for the model scene.
//!
//! Cached block bytes index a scene palette (`solidcraft_gpu::view::Palette`). A body's faces,
//! edges and placement keep their slots while the scene rebuilds, so an appearance or theme edit
//! changes only palette entries — never a block's vertex bytes. Slots are append-only for the
//! session and are reset when a different design replaces the one they belonged to.
//!
//! The model scene needs at most one stable slot per visible body face (plus a few per body), so
//! the arena is capped by [`MAX_PALETTE`]. Past the cap a face falls back to its body's slot:
//! editing such a face once rebuilds that body's block (the build still succeeds).

use std::collections::HashMap;

use crate::gpu::{MAX_PALETTE, PaletteIndex};

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
    /// Bumped when the arena is reset (a different design); cached blocks must rebuild then.
    generation: u64,
}

impl PaletteArena {
    /// The stable entries, copied into the scene's palette.
    pub fn entries(&self) -> Vec<[u8; 4]> {
        self.entries.clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The slots of `name`, allocating them (and `face_count` face slots) the first time.
    pub fn body(&mut self, name: &str, face_count: usize) -> BodySlots {
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
        let body = self.push();
        let edge = self.push();
        let hidden_edge = self.push();
        let instance = self.push();
        let slots = BodySlots { body, faces: u32::MAX, face_capacity: 0, edge, hidden_edge, instance };
        let slots = self.grow_faces(&slots, face_count);
        self.bodies.insert(name.to_string(), slots);
        slots
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
            self.entries.resize(self.entries.len() + need, [0; 4]);
            return BodySlots { face_capacity: want, ..*slots };
        }
        if want as usize <= room {
            let base = u32::try_from(self.entries.len()).unwrap_or(u32::MAX);
            self.entries.resize(self.entries.len() + want as usize, [0; 4]);
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
            self.bodies.clear();
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
        // A new design with no shared names resets the slots.
        let generation = arena.generation();
        arena.sync(["B"]);
        assert_eq!(arena.generation(), generation.wrapping_add(1));
        assert!(arena.entries().is_empty());
        // An empty scene does not reset.
        let before = arena.generation();
        arena.sync([]);
        assert_eq!(arena.generation(), before);
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
}
