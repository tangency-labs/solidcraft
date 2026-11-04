//! A fillet saves `turn: "swept"` when two of its rounds that turn round a sharp side edge (the
//! rounds along a boss's base, at its upright edges) are swept round that edge, their corner an
//! exact torus; absent, the default `mitre`, they meet in a mitre, as designs before this change were
//! built. An older design has none and keeps its mitres. The last release refuses a design with
//! this change (the newer format number).
