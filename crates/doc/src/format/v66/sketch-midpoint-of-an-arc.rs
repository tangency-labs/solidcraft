//! A MidPoint constraint can tie a sketch point to an arc, holding it halfway along the arc
//! (#16860). It is saved as before, a `midpoint` constraint with its `point` and its `entity`,
//! and the entity is an arc where it used to be a line. Older designs have only lines there and
//! read as they did. No step is needed. The last release opens a design that has one with a
//! newer-format warning; it refuses the constraint (a MidPoint needs a line there), so its
//! sketch is reported as failed and what it cannot read is kept.
