//! A frame member saves its `corner` (none, mitre or butt), its `flip` (a butt's other member runs
//! through), `coped` (#17854: every butt joint of the frame cut to the other tube's real shape;
//! saved only when true), `gap` (the weld gap left at every welded end, a length; saved only when
//! set) and `corners`, the corners, crossings and overlaps finished their own way, each as
//! `{members: [{line}], kind, gap?}` (#4003's shape): its members in order, the first running through
//! (or staying whole at a crossing or overlap), and of three or more the first two making the corner.
//! An older design has none of these, so every member keeps its ends square to its line as before. A
//! design from a test build of this change is read as it opens, not by a step (it already lists this
//! change as applied): a member given as a bare line id reads as `{line}`, `corner` as `kind`, an old
//! `flip` is ignored (the members keep their written order), and `coped_butt` (the frame's or an
//! override's) reads as `butt` with `coped` (`Document::from_json`, `frame_corners::read_coped`). A
//! build before this change opens a design that uses them with a newer-format warning and keeps the
//! fields it cannot read.
