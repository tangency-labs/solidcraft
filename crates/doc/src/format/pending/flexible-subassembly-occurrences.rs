//! Each occurrence of a linked subassembly (a link that saves `tree`, `link-component-tree`) is
//! rigid or flexible (#5526, #1516). `tree.flexible` lists the flexible occurrences by id, each
//! with the joints here that stand for the linked design's own, by their ids there
//! (`{"21": {"7": 30}}`), and `tree.fixed` the occurrences kept for that design which it holds
//! still. A flexible occurrence has its own copy of the occurrences inside it: a side of one of
//! its joints is `{"occurrence": 21, "inside": [13]}` (`joint-origin-inside`), and where a copy
//! stands is an entry of the design's `assembly.poses`
//! (`{"occurrence": 21, "inner": 13, "transform": [...]}`), kept in order of `occurrence`, then
//! `inner`. A pose can also be for an occurrence the link keeps: the pose the linked design has
//! for a flexible occurrence inside it. An older design has none of these keys: every
//! occurrence of its linked subassemblies is rigid and shows the linked design as it was
//! saved. The last release opens a design that has them with a warning about the newer format,
//! shows each occurrence as the linked design was saved, places nothing by a flexible
//! occurrence's joints (each joins an occurrence to itself), and saves the keys back as they
//! came (`crates/doc/src/newer_file.rs`).
