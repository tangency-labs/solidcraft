# general: PR #700 after the cache-owner rebase (2026-10-25)
Updates: plan/findings/general--521-topology-evaluation-status.md

Rebased on main `86f600878928a11af49e6a8c7b15cdcfc4f46044`; the conflict
was in `plan/units/heal.face_props.toml`. Both main's newer green validation notes
and this PR's historical evidence are retained. The four kernel modules and
vertex fixture corpus are byte-identical to the preceding PR head. Main's
cache-owner changes eliminate the 15 previous healing deferrals.

Fresh full native `heal.face_props` on source head
`87c6dbfe947173e2de195c8536090dd8051dfc18`: **1,528 cases, 1,520 matching,
eight existing known differences, zero deferred, zero unexpected failures**.
Every recorded mismatch in those eight cases was checked: all are exclusively
`values.v` stage markers, C++ 5 versus Rust 4, with no additional mismatches hidden
by a record. No case, exception, guard or comparison rule changed. This PR's
healing unit stays partial for that deliberate retry-state difference, even though
current main's unmodified unit is green.

Formatting, default workspace/all-target Clippy with warnings denied, all 2,139
kernel unit tests and wasm pass. Other native figures and the previously observed
query.face_p4/gen0360 baseline failure retain their earlier validation bases; they
were not re-certified by this targeted healing run. Canonical CI remains pending.
