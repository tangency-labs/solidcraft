//! A joint origin can sit on an edge of a planar face (`Snap::Edge`, saved as `"type": "edge"`):
//! at the edge's middle or at a corner where it meets another edge, all three found again by name.
//! Older designs need no transformation: they have no such origins. The last release refuses a
//! design saved with this (the newer format number).
