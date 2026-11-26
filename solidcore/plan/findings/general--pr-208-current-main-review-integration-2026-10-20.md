# PR #208 — current main review integration (2026-10-20)

Merged canonical main `e5f3e441` without rebasing. Both appended sections in
`findings.md` survive, including the query-face and boolean-merge entries. The
p1 manifest retains the round-4 counts and PR #208 integration text together
with main's `CO-PORTED in blend.fillet_intersector (PR #179)` attribution.
A comment at `corner_vertex` records why the recovery context's solver is the
current geometry's own solver in `ProcessRollOver`. No numeric algorithm or
coverage boundary changes. Validation results are recorded on the final PR head.
