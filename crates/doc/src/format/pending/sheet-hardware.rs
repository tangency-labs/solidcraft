//! A Hole may save the press-in hardware it is the mounting hole of (#17752) as `hardware`:
//! `{"kind": "nut" | "flush_nut" | "stud" | "standoff", "thread": "M3", "length": "10 mm", "part":
//! "nut.m3"}`, where `length` is saved only for a stud or a standoff. Such a hole is a plain
//! through hole and has no `thread` of its own. The key is saved only when set, so a design
//! with no hardware saves as before, and older designs open with none and build as they did. The
//! last release opens a design that has it with a warning about the newer format number, drills
//! the hole as a plain hole and keeps the field it doesn't read.
//!
//! The model cache a design file carries keeps the same hardware for each sheet's flat, as a
//! `hardware` list on the sheet: `{"feature": 12, "kind": "nut", "thread": "M3", "length": 10.0,
//! "part": "nut.m3", "center": [x, y], "radius": 2.125}`, lengths in millimetres, the centre in the
//! flat's own coordinates and the radius of the hole cut (the flat pattern reports twice it as
//! `hole_diameter`). `length` is left out for a nut, and the list for a sheet with none, so a design
//! without hardware saves a cache as before, and a cache written before any hardware existed reads
//! with none, which is what it had.
