//! A Pattern on Path can save a start point (#6054): `start`, a length expression for how far
//! along the path the original sits, or `start_fraction`, the same as 0…1 of the path's length.
//! The copies are measured along the path from there. Older designs need no transformation:
//! without either, the pattern starts at the path's start, as before. The last release refuses a
//! design saved with this (the newer format number) instead of dropping the start point.
//!
//! A Pattern on Path made now also saves `follow_frame: true`: with Path direction, its copies
//! follow a rotation-minimising frame along the path. A design saved before has no such field
//! and keeps what it built: its copies turn about the sketch's normal only.
