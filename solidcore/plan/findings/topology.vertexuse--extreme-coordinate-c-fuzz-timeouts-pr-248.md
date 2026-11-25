# topology.vertexuse: extreme-coordinate C++ fuzz timeouts (PR #248)

While validating the empty-knot encoder fix for issue #198, the Linux run against
upstream `c9c5979` reported:

```text
FUZZ-FAIL topology.vertexuse cases=2000 rejected=327 malformed=433 ran=1238 differs=0 crashes=0 panics=0 timeouts=2
```

Reproduce with `cargo run --profile difftest -p difftest --features cpp -- --unit
topology.vertexuse --fuzz 2000` (default seeds). Both cases exceeded the harness's
20-second deadline on the C++ side, including its automatic individual retry:

- `fuzz01312`, seed `13409719046073002912`, base `face4_vu0_sector1`:
  `/brep/surface/bspline/net/0/1/1 = 1e300`.
- `fuzz01688`, seed `13409719046073003288`, base `adv_bss_sliver_vu2_seam`:
  `/brep/surface/bspline/net/2/1/1 = -1e300`.

The harness writes complete inputs to `bin/fuzz/topology.vertexuse/fuzz01312.json`
and `fuzz01688.json`. Both have nonempty U/V knots, so the checked subtraction
introduced in #248 encodes the same values as the previous subtraction. They do
not exercise the empty-knot rejection. The exact C++ call responsible has not
been isolated; a timeout is not proof of an infinite loop. Do not loosen the
timeout or reject all large coordinates without determining the valid-input
boundary. No Rust/C++ value mismatch was observed in the completed fuzz cases.
The ordinary unit run remains 2686 cases, 2250 passed, 436 deferred, 0 failed,
with maximum relative error 0.
