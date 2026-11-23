//! A Plane at Angle turned about a construction axis given by name saves a reference to that axis
//! (`line: {type: "axis", name}`), found again on every evaluation, so the plane follows the axis
//! when it moves (#16527). Older builds saved the axis's position and direction at that moment
//! as plain numbers (`axis_origin`, `axis_dir`) and the plane stayed where it was. A plane saved
//! that way, without a `line`, is read as before: its axis is fixed. No step is needed. The last
//! release opens a design that has the reference with a newer-format warning; it cannot find the
//! axis there, so it reports the plane as failed and keeps what it cannot read.
