//! A design can hold frame corners (#4003): a `frame_corner` feature with the two `members` it joins
//! (each a frame member feature by name and a `line` of it), its `kind` (`mitre`) and the weld `gap`
//! between the cut faces. A corner keeps its members' `frame_members` records as a trim does.
//! Older designs need no transformation: they have no such feature. The last release refuses a
//! design saved with this (the newer format number).
