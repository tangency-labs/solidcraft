# blend.fillet_corner_p3: reachable closed teardrop angle and split allocation (#2349, 2026-11-04)

Units: `blend.fillet_corner_p3`, `blend.fillet_geom_p3`, `blend.fillet_executive_p3`.
PR: https://github.com/tangency-labs/solidcore/pull/2381.

## Operand and reproduction

`crates/solidcore/tests/issue_2349_closed_angle.rs` constructs one nonrational
clamped cubic Bezier with poles `(0,0,0)`, `(20,-10,0)`, `(20,10,0)`, `(0,0,0)`,
knots `[0,1]`, multiplicities `[4,4]`, extruded 10 along Z with caps. The operand
has 3 faces, 3 edges and 2 vertices and passes Level2 validation. Its upper rim
is one closed edge with a 53.130102354-degree non-G1 seam.

The real session selects `NByOneClosed`, S=1/T=2. The corner contains a
`FilletXFillet` edge and vertex, and its geometry requires both rails to split.
The radius-0.5 round completes with five Level2-valid faces. Tests cover defaults
and the generic-blend profile used by #2499 (all fixes except geometric G2).
#2499's six-pole, 90-degree teardrop tests completed rounding and volume; this
four-pole operand adds intermediate state and split allocation coverage.

The serialized independent Rust/C++ input is
`fixtures/blend.fillet_executive_p3/data/issue_2349_teardrop.smb`; its ten cases are
in `fixtures/blend.fillet_executive_p3/issue_2349_closed_angle.json`. Every original
checkpoint field and comparison tolerance is retained. Regenerate the input by
setting `SOLIDCORE_2349_WRITE_FIXTURE` and running only the reachability test with
one test thread; the committed ASCII copy has trailing spaces stripped.

```sh
export PATH=/home/karlvonrichter/.cargo/bin:$PATH
export CARGO_BUILD_JOBS=2
if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
else
  unset RUSTC_WRAPPER
fi
nice -n 10 cargo test --profile goldens -p solidcore --test issue_2349_closed_angle
nice -n 10 cargo run --profile goldens -p goldens -- \
  --unit blend.fillet_executive_p3 --filter issue-2349 --goldens
```

Run in Ubuntu WSL from this worktree's `/mnt/c` path. Since the goldens migration,
the runner neither builds nor runs C++. Eight intermediate checkpoints have goldens;
the two full-body cases remain known failures rather than blessing an incorrect
closed-angle construction. The native comparisons below are historical evidence.

## Resolved allocation mismatch

Before the fix, `solver_geometry` and `corner_edges` each exposed two new corner
vertices with C++ tolerance `0` versus Rust `1e-5`. The root cause was ordinary
B-rep vertex allocation applying the default tolerance and owning-list membership.
`FilletBrep::topology_split` now allocates unlinked `Vertex::default()` records,
preserving subsequent geometry and tolerance updates. The direct unit assertion
and real-session regression require exact zero tolerance and absence from the
owning vertex list; all ten native checkpoints pass without relaxed comparisons.

Permitted fork source at `a18bbe523b9c7819f1a9da8c40a54c15f9936125` confirms the
same constructor/attachment contract (`SmFilletVertex`, `SmVertex`,
`AddFilletVertex`). The configured native libraries used for the reported difftest
are the old 1.0.1 reference, not a certified fork build. Historical exploratory
fork archives had no producer manifest tying their hashes to source; those runs
cannot certify revision-specific fork parity. The exploratory Python probe and
its unverified sampled-value regression were removed from this allocation PR.
Their local copies are retained outside the tracked contribution.

## Remaining #2349 contracts

Two explicit ignored tests retain demonstrated failures:

- `closed_angle_is_a_blend_event_instead_of_the_legacy_corner`: the radius-0.5
  factory still leaves `corner.blend=None`, selecting the legacy closed corner.
- `closed_angle_round_builds_validly_or_refuses_with_a_fit_size`: radius 3 refuses
  with size `1.8749999999999993`, but a fresh build there fails Level2 validation.
  The midpoint profile curvature radius is exactly 1.875; no safe margin is guessed.

Run these with `--ignored --nocapture` to reproduce. They are future contracts,
not passing construction evidence. Exact curved-spine self-mitre, both exact
pcurves, a proven fitting size, T=3 and revolved kinked seams remain unfinished.
#2307 and #2353 are merged and no longer dependencies. Design questions and scope
coordination are on open #2349:
https://github.com/tangency-labs/solidcore/issues/2349#issuecomment-6624323436.
Shared near-bound assembly work (#2404) is separate; no competing change is made.

## Verification scope

Before cleanup: the full configured-reference executive unit passed 796 of 798
cases, with 2 rejections, no failures/deferred cases and `max_rel_err=0e0`.
Fmt, Clippy, wasm and focused allocation tests also passed on earlier heads.
Those results predate #2400, which deleted the old corner code and the
`fillet_blend_corner` flag and left the executive unit `partial`. After rebasing
onto it, these tests run on the default kernel only; the difftest, the tests and
the two ignored contracts above have not been re-run locally.
Current cleanup verification is reported in the PR body, with fresh results
separated from historical results. No certified self-mitre or full fork-construction
parity is claimed; #2349 remains open.
