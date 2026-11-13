//! A design can hold frame members (#4003): a `frame_member` feature with its frame sketch
//! (`path_sketch`), the `lines` it runs along (none: every straight line drawn in it), its
//! `profile` (a built-in profile by `source: library`, its dimensions saved as expressions, or a
//! profile sketch by `source: sketch`), the profile's `anchor`, `angle` and `mirror`, and its
//! members' names: `body_name` and `names` (each member's body name by its line's id). Older
//! designs need no transformation: they have no such feature. The last release refuses a design
//! saved with this (the newer format number).
