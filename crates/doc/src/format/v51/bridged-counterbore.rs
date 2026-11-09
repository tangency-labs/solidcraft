//! A counterbore hole may save a `bridge` length: two printing layers that thick below the
//! counterbore's floor (a slot as wide as the hole along X, then a square) so a 3D printer bridges
//! the step. An older design has none and keeps a plain counterbore. The last release refuses a
//! design that has it (it asks for an update) instead of opening it without the bridge layers.
