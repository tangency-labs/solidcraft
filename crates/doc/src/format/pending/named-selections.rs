//! A design can save `named_selections` (#18716): a list of `{name, items}`, each a selection
//! kept under a name, its items as `select.set` takes them (placements by path, bodies by name,
//! sketches and other features by id; a face or edge also keeps its persistent `name`, by which
//! it is found again after an edit renumbers the body's faces). The Browser lists them in a Named Selections folder,
//! and one click selects the items again. Older designs have none, so nothing is rewritten. The
//! last release does not know the key: it opens a design that has it with a warning about the
//! newer format, builds everything, and keeps the selections when it saves.
