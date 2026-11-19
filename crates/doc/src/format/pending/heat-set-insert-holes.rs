//! A Heat-Set Insert (#12378) is a hole feature that saves two more fields: `insert`, the insert
//! its holes are for (size, outer diameter, length, least wall and floor), and `more`, further
//! places drilled as `position` is, with the same direction. A hole without them is an ordinary
//! Hole, and older designs have neither, so no step is needed. The last release opens a design
//! that has them with a newer-format warning: it drills only the hole at `position` (or at the
//! sketch points) as a plain Hole, without the insert's check, and keeps what it cannot read.
