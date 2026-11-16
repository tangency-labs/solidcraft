//! `assembly.rigid_groups` lists the parts an import placed, each list held as one:
//! `{"id": 41, "name": "Board", "component": 7, "members": [8, 9, 12]}` keeps occurrences 8, 9
//! and 12 where they are in their parents, as one rigid joint per placement used to (the import
//! made those before); `component` is the import's component, which `component.unlock` takes.
//! Older designs have no `rigid_groups` and keep their rigid joints, so nothing in them is
//! rewritten. The last release opens a design that has them with a warning about the newer
//! format and keeps the key as it came; there the parts of such an import can be moved apart,
//! as after `component.unlock`.
