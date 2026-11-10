# general: PR #700 cache-wrapper rebase (2026-10-25)
Updates: plan/findings/general--521-topology-evaluation-status.md

Rebased on main `e0266d5af928b70bf6967afc442cbc7f8eb53796` after the anticipated
`FaceProps::set_props_stage3_cached` conflict appeared. The public wrapper still
creates a query cache; `set_props_with_seam` and `classify_loops_cached` keep the
caller's cache. Failure rollback belongs in the cached entry point, so both routes
reset incomplete Stage3A to Stage2 and preserve classification errors. The existing
retry regression now exercises both routes. All 2,133 kernel unit tests, formatting,
default workspace/all-target Clippy with warnings denied, and wasm pass.

Current main makes four more WR-266 healing-marker differences reachable. A local
raw-output probe ran each exact input against the unchanged C++ reference, this PR,
and current main. Main matches C++; this PR differs only at the listed stage markers.
Statuses, other values, integer fields and topology are identical. These are
individual known mismatches, never parity passes; `heal.face_props` stays partial.

| Case | Seed | Differing offsets in `values.v` (C++ 5, Rust 4) | Statuses immediately before those markers |
|---|---|---|---|
| gen0241 | 16562362309779923184 | 84 | 1040 |
| gen0662 | 16562362309779923605 | 1, 84, 167 | 1040, 1000, 1000 |
| gen1105 | 16562362309779924048 | 84 | 1040 |
| gen1377 | 16562362309779924320 | 167, 250, 333 | 1040, 0, 0 |

The later snapshots retain the earlier incomplete marker even when a subsequent
query succeeds; a successful query does not certify Stage3. All four original
WR-266 entries and all 40 current-main entries remain unchanged. No tolerance,
floor, native guard, shim or fixture was changed.

A separate `query.face_p4/gen0360` comparison (seed 4634847627957580463) fails
identically on this PR and on exact current main: `ints.classify_loop_ios[8]` is
C++ 1 versus Rust 2, `[10]` is 1 versus 0, `[11]` is 1 versus 5, and
`ints.looptree.len` is 17 versus 9. Raw C++ outputs and raw Rust outputs are each
identical across the two revisions. This pre-existing classification failure was
not suppressed or added to the exception list. Reproduce with
`difftest --unit query.face_p4 --filter gen0360`. The full query unit has 500 cases:
492 matching, two pre-existing known mismatches, five deferrals and this one
unexpected baseline failure. Canonical CI and a clean full native matrix are not
claimed.

The broader native run completed topology.edge (1,915/1,915), topology.vertex
(5,598/5,598) and topology.loopuse (8,854/8,854) with zero failures or deferrals, then
was stopped to investigate the targeted healing/query failures. Remaining units
from that attempted matrix are not represented as completed validation.

Fresh complete heal.face_props rerun after recording those four individually proven
differences: 1,528 cases, 1,505 matching, eight explicit known stage-marker
differences, 15 existing surface.surface_p2 deferrals, zero unexpected failures.
The eight remain genuine mismatches; the unit remains partial.
