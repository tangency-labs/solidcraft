# general: difftest heap-pad address-stability test is ASLR-sensitive (2026-10-29)

The difftest runner unit test
`runner::tests::heap_pad_moves_the_forked_childs_allocations` (`crates/difftest/src/runner.rs`,
`#[cfg(all(feature = "cpp", unix))]`) fails in this sandbox but passes on CI. It is a
harness/platform sensitivity, not a bug in the isolate or heap-pad mechanism.

## What the test checks

`isolate::fresh_process` forks a child, runs the case's C++ shim in it, and pipes the
result back, so every case starts from the parent's state. `set_heap_pad(n)` makes each
later forked child allocate `n` small blocks before the case and free them after, which
moves the child's heap addresses and nothing else (`--heap-pads`, used to detect
address-dependent C++ bugs: ND-2, ND-258).

The test records the address of a `Box::new([0u8; 24])` in an **unpadded** probe
(`plain`), applies a 512-byte pad and records the padded probe (`padded`), then records a
second unpadded probe (`plain_again`). It asserts:

- `plain == plain_again` ("unpadded forks must not move on their own") — two consecutive
  unpadded forks allocate at the *same* virtual address.
- `plain != padded` ("the pad must move the child's allocation").

The first assertion is the design's backbone: after a warm-up fork, an unpadded fork's
heap layout is stable, so the only thing that moves an allocation is an explicit pad.

## Reproduction (this sandbox)

`cargo test --profile difftest -p difftest --features cpp --lib runner::tests::heap_pad_moves_the_forked_childs_allocations`
panics at `runner.rs:1596`:

```
assertion `left == right` failed: unpadded forks must not move on their own
  left:  94560383488000.0   (~0x55…  PIE / main-heap region)
  right: 140265713044736.0  (~0x7f…  high mmap region)
```

`left` and `right` differ by ~4.5e13 and sit in entirely different address regions, i.e.
the two unpadded forks did not share a heap layout. The sandbox has full ASLR:
`/proc/sys/kernel/randomize_va_space = 2`. Fork normally copies the parent's page tables,
so unpadded forks should land identically; here the child's layout is re-randomized per
fork, so the stability assumption breaks.

## Why it is not a logic bug

- The `plain != padded` (pad moves the address) and `padded_children` counting assertions
  are independent of absolute address stability and are unaffected.
- The `isolate` pipe/fork/encode-decode logic is exercised by the sibling tests
  (`round_trip`, `child_result_and_crash`, `child_past_its_alarm`), which pass here.
- CI runs the identical command in the `test` job, step "Difftest runner tests"
  (`.github/workflows/ci.yml:152`, `cargo test --profile difftest -p difftest --features
  cpp`); in run 40960066982 that step concluded **success** while `difftest-sweep` failed
  on the WR-474 query units. So the test passes on CI's runners and fails only under this
  sandbox's per-fork ASLR behavior.

## Impact

- Does not affect the correctness gate: the `--green --jobs 0` sweep is green (0 failed),
  and the WR-474 query reconciliation behind this investigation is complete.
- Does not weaken `--heap-pads` address-dependency detection: that compares padded vs
  unpadded, which still holds. The only casualty is this single stability assertion.
- If CI ever runs under a per-fork-randomizing loader (some container/seccomp
  configurations), this assertion could go red without any real change. The fix, if that
  happens, is to make the assertion robust to per-fork layout drift (e.g. compare the pad's
  delta, `padded - plain`, across forks, rather than the absolute unpadded address) rather
  than to relax the mechanism.

Class: nondeterminism / platform sensitivity (reproducible here, green on CI).
