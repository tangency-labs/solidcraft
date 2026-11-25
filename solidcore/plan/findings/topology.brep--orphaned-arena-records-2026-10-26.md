# topology.brep: construction leaves unreachable use records in the arenas (2026-10-26)

**Unit:** topology.brep (the arena model in `crates/solidcore/src/topology.rs`), seen
through the primitive builders and the Boolean.

**Observed.** The first version of `topology::integrity::check` (edge-ends work) walked
every live record of every arena. On a fresh `primitive::api_primitives::create_box` it
failed with `NotFound { operation: "topology::integrity::check: vertex use vertex",
entity: Vertex(VertexId(0)) }`: a vertex use record still in the arena names a vertex
that was removed. The same kind of record turned up on fresh cylinders and on several
Boolean results (vertex uses naming removed vertices, loop uses whose start names a
removed edge use). Restricting the check to what the B-rep holds (its region, face, edge
and vertex lists and what their links reach, the roots `SmBrep::ValidatePointers` walks)
made the box pass, so these records are unreachable: no list or link of the B-rep leads
to them.

**Where they come from (as far as known).** Upstream frees an object when its topology
is deleted (`delete pVU`, `delete pEU`, ... in `SmBrep::DeleteEdge`, the Euler operators
and the primitive builders' clean-up). The port's deletion paths unlink a record from
its owners but, on some paths, do not remove it from its arena, so the record stays live
in the arena with links to records that were removed. Which paths do this has not been
traced; the box case shows it happens during primitive construction (vertex merges or
edge splits there), and the Boolean adds more.

**Consequences.**
- Correctness: none known. Nothing in the B-rep reaches these records, and every query
  walks the B-rep's lists and links.
- Memory: the arenas grow with every operation that orphans records and never shrink, so
  a long session (SolidCraft keeps bodies across many operations) holds dead records
  for the life of each `Brep`.
- Tooling: anything that iterates an arena directly (rather than the B-rep's lists) sees
  them; `integrity::check` now walks only reachable records for this reason.

**Follow-up (low priority, after edge-ends ships):** find the orphaning paths (a
diagnostic counting arena records not reached from the B-rep after each public
operation would locate them), decide per path whether the port should remove the record
as upstream frees it, and whether `Brep` needs a compaction step (renumbering ids is not
possible while callers hold them, so likely a free list or a tombstone sweep).
