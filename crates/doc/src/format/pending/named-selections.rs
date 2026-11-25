//! A design can save `named_selections` (#18716): a list of `{name, items, generated}`, each a
//! selection kept under a name, its items as `select.set` takes them (placements by path, bodies
//! by name, sketches and other features by id). A body, face or edge also keeps `made_by`, the
//! feature that made its body, so a body given the name after that one was deleted is not
//! selected in its place, and gets `gone: true` once another body is renamed to the name of a
//! body that went (a pattern copy a lower count removed). A face or edge of a solid also keeps
//! its persistent `name`, by which it is found again after an edit renumbers the body's faces.
//! `generated` (left out when false) marks a name SolidCraft made, which follows an update; a
//! typed name stays. The Browser lists them in a Named Selections folder, and one click selects
//! the items again. Older designs have none, so nothing is rewritten. The last release does not
//! know the key: it opens a design that has it with a warning about the newer format, builds
//! everything, and keeps the selections when it saves.
