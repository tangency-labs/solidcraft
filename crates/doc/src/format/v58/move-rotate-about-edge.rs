//! A `move` feature can carry `rotate_about`: the model edge or round face its turn was picked
//! on, saved as a circular pattern's `axis_ref` is. The move finds that edge again on each
//! rebuild and turns about where it is now, so a step inserted earlier that moves the body takes
//! the turn with it (#17974). Its `rotate_axis` and `origin` are still saved, and are used when
//! the edge can no longer be found. Older designs have no `rotate_about`, so nothing in them is
//! rewritten and their moves keep turning about the saved line. The last release opens a design
//! that has the field with a warning about the newer format, turns about the saved line, and
//! keeps the field it cannot read when the design is saved.
