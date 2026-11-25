# PR #233 — caller Kernel integration (2026-10-21)

Main #133 made curve and surface evaluation explicitly take the caller Kernel.
The primitive p1/p3 and sweep stack now forwards its existing owner-qualified
Kernel through those calls. The retained `Curve::is_linear_after_ends` helper
and `ruled_surface_from_offset` require the same Kernel, including for interior
samples, lengths and degeneracy checks. No production default Kernel is added.
Numerical formulas, operation order, tolerances and error handling are unchanged.

Main through `100535f4` is integrated, including fixture export memoization and
executive updates. Both parents' findings and main's complete p5/p7/HCR notes
remain, with the earlier API extensions and virtual/lazy-NURB changes retained.
The native p3 fixtures still compare the ruled-offset helper and early/empty
state; they do not certify successful full primitive/cap/stitch topology.
The landed brep_p6 MakeFaceTopology requires a mutable Kernel: the curve-owner
wrapper, primitive detailed/trimmed-face adapters and cylindrical box now forward
the existing caller mutable Kernel. No clone/default hides those context mutations.
The actual-head validation results are tracked in the PR discussion.
