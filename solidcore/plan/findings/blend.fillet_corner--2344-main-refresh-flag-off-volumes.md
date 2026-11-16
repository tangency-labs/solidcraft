# blend.fillet_corner: flag-off snapshots changed after main refresh (2026-11-03)

Units: `blend.fillet_corner_p1`–`p3` / executive integration.
PR: https://github.com/tangency-labs/solidcore/pull/2382

After merging authoritative main `ebe6722e09169cce29a20c455fc6ec606b57caa0`
and dependency #2369 `ee6e1b349e9ebae287246cc42a787a6620ab429e`, the
existing `fork_smlib104_setback_corner` regression has four failures, all in
its `fillet_blend_corner = false` body-volume snapshots. Its flag-on volumes
still pass. Tested merged code: `403d8d30b71d5e9ad7d97396752c44b5f5f45de6`.

Reproduce in Ubuntu WSL, from the assigned worktree:

```sh
PATH=/home/karlvonrichter/.cargo/bin:$PATH CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 \
  nice -n 10 cargo test --profile difftest -p solidcore \
  --test fork_smlib104_setback_corner
```

Inputs are the named tests' independently constructed boxes/rounds, with all
fixes except `geometric_g2_continuity`, and the blend-corner flag disabled:

| Test | Current Rust volume | Existing expected volume |
| --- | ---: | ---: |
| `asked_setback_is_the_setback_fill` | 47325.531676682454 | 47325.53167676825 |
| `asked_setback_too_short_names_what_builds` | 7873.2076875019675 | 7873.207687501852 |
| `concave_corner_is_the_setback_fill` | 7015.147100141569 | 7015.147079173456 |
| `three_unequal_rounds_are_the_certified_setback_fill` | 23842.340012777244 | 23842.34001926002 |

The existing snapshot assertion is 1e-14 relative. The concave difference also
exceeds the repository's 1e-9 parity threshold; the other differences do not.
These are previous regression expectations, not a new independent C++ run for
the flag-off construction. No expectation or tolerance was changed to hide them.

Suspected area: the legacy corner construction or its trimming/integration,
exposed by main's intervening changes. The scale-search construction is behind
the disabled flag here, so these failures do not demonstrate a scale-search
divergence. Disabling `keep_exact_pcurves` alone did not change the results;
disabling `ellipse_open_arc_keeps_end_angle` alone did not resolve them either.
Both diagnostic changes were removed. Exact root cause remains unconfirmed.

The separate new unequal (2, 2, 3) box regression passes both flag states after
updating only its flag-on volume from independent native 20b1c01 output.
The scale-search PR stays draft; this record is a reproducible blocker rather
than an assertion that all merged-head regression gates pass.
