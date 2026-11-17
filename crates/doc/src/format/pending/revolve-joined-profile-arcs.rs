//! New solid Revolve features save `join_profile_arcs: true` and join contiguous pieces of the
//! same profile circle before turning it. A Revolve without the field keeps its separate faces
//! and edges, so downstream features can still resolve their saved references.
//! The last release opens a design with this format with a warning and keeps what it cannot read.
