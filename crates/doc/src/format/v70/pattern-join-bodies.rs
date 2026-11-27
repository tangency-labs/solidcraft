//! A Pattern's Join (#21625): `join: true` on a `pattern` feature joins the bodies it copies from
//! and their copies where they touch or overlap. Saved only when on, as Mirror's `combine`. Older
//! designs need no transformation: without it every copy is a body of its own, as before. The
//! last release opens such a design with a warning, builds the pattern without the join, and keeps
//! what it can't read when it saves.
