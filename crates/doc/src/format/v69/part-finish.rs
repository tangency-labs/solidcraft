//! A part's finish (`"finish"`: free text such as "powder coat, RAL 9005, matt") is a SolidCraft key of
//! a component's `metadata` and of the design's own, and the design's own metadata may now hold the
//! static `"material"` a component's always could (a design that is one part has no component to hold
//! them). Both are plain text under the unprefixed keys already reserved for SolidCraft, in the same
//! `metadata` object as before: nothing else moves and no step rewrites an older design. An older
//! design has neither on its design, and a component's `material` means exactly what it did; its
//! parts have no finish, and a free property named `finish` still shows where a finish is shown.
//! The last release opens a design that has them with a warning about the newer format number, lists
//! them among the design's other data, and keeps them as they are when it saves.
