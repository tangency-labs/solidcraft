//! An extrude edited from one-sided to symmetric saves the way it went before
//! (`extent.one_sided_direction`: `positive` or `negative`), so the cap at the end where its
//! `start` cap lay also answers to `start`, and an edge or face picked on that cap is still
//! found on the cap facing the same way (#18459). Older designs need no transformation: a
//! symmetric extrude without it has no cap that answers to `start`, as before. The last release
//! reads a design that has it without the field, so a reference to that start cap fails there as
//! it did before.
