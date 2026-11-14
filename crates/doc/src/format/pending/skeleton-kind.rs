//! A component saves `kind: "skeleton"` when it is a skeleton (a reference component with no solid
//! bodies); a normal component saves nothing. Older designs have no `kind` and read as normal
//! components, unchanged. The last release opens a design that has a skeleton with a warning and
//! keeps the `kind` it can't read.
