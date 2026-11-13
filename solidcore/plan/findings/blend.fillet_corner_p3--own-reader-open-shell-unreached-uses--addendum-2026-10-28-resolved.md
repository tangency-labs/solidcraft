Updates: plan/findings/blend.fillet_corner_p3--own-reader-open-shell-unreached-uses.md

# Resolved: the open_shell.smb unreached uses are the same state in both kernels (2026-10-28)

Not a Rust reader divergence. During `ReadFromFile`'s healer, `Fix_CoinVertices`
combines vertices 2 and 3 and `SqueezeEdge`s their short common edge (edge 2), after
`Cache_EdgeProps` cached the CCW gaps of its neighbours. Both kernels then hold the same
live topology (62 edge uses, 21 edges, 16 vertices reached) and the same 4 stale gaps
(on the reached uses that were the CW neighbours of edge 2's uses), with the same gap
values and edge parameters.

- C++ (private probe, `free` wrapped, reference libraries): the 4 stale
  `m_pEdgeuse1` pointers are not reached by the walk and were all freed during the read:
  the gaps dangle. Upstream defect: `plan/upstream-issues/UB-271--squeezeedge-leaves-ccw-gap-naming-freed-edgeuse.md`.
- Rust: `topology::brep::p3::squeeze_edge` unlinks the edge uses, edge and vertex from
  every list but leaves their arena records, so the stale gaps name unlinked records.

Fix (harness only): `own_builder` renumbering maps a cached CCW gap's second edge use
that no walk reaches to the edge use's own CCW edge use, the convention the C++ export
import already uses (the export carries only `IsInit`). Any other unreached reference
is still an error. No C++ state reaches the Rust side. `Fillet/open_shell.smb` is off
`KEPT_ON_IMPORT` in `blend.fillet_corner_p3`, and its 31 cases pass on the own reader.

Not changed: `squeeze_edge` keeping the arena records of what it deletes (upstream
`delete`s them). Removing them would leave the stale gaps naming removed ids, so the
renumbering would need the same rule.
