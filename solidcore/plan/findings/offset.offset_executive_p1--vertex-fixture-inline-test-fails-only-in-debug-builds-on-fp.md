# offset.offset_executive_p1: vertex-fixture inline test fails only in debug builds on fp-noise net values (2026-10-24)

Found on upstream/main 859e06bcc, macOS arm64 (diag lane `diag/offset-exec-test`):
`cargo test -p difftest --features cpp` fails
`offset_offset_executive_p1::tests::vertex_fixtures_compare_actual_caps_not_only_presence`
asserting `compare(&native, &rust, &[])` on fixture `vertex_strut_closed_circle_360`
(vertex.json#8): `values.vertex.surface.net[8]` cpp `1.6653345369377348e-16` vs rust
`2.7755575615628914e-16` (and 8 more entries, incl. `[20,32,44,...]`), rel_err 0.4.

Classification: **profile-sensitive near-zero disagreement; first causal
operation untraced.** The observations below do not establish whether the
cause is code generation, an arithmetic-order difference or a profile-sensitive
branch; no kernel correction is proposed in this diagnosis. Evidence:

1. The unit runner on the same lane compares the same case:
   `difftest --unit offset.offset_executive_p1 --jobs 2` → PARTIAL cases=1048
   passed=909 failed=0 deferred=138 max_rel_err=1.850371707708594e-16; the
   `vertex_strut_closed_circle_360` entry in `reports/difftest/offset.offset_executive_p1.json`
   is `outcome: pass, max_rel_err: 0.0` (exact match, 204 values compared).
2. The same inline test **passes** under `cargo test --release --features cpp` on
   this machine and **passes** in the optimized difftest-profile runner. It fails
   only under the unoptimized test (debug) profile. The C++ reference is
   the same prebuilt `bin/cpp` library in both runs (built `-ffp-contract=off`),
   so the observed discrepancy depends on the Rust build profile with the
   native reference held fixed; a codegen cause has not been isolated.
3. The mismatching magnitudes are 1e-16 arithmetic residue (both `cpp` and `rust`
   values are multiples of ~5.55e-17 around a should-be-zero net entry); with a
   pure `|a-b|/max(|a|,|b|)` relative rule the reported pair has relative error
   0.4 despite an absolute difference of about 1.1e-16. The rule has no absolute
   epsilon and the unit (`Runner.abs_floors()`, asserted empty in the test)
   documents none.

So the port's vertex-cap net matches the C++ reference bit-exactly in optimized
builds, and differs by ~1.1e-16 absolute on 9 near-zero net entries in debug
builds. A mul-add/summation lowering difference is a hypothesis, not a traced
cause. The standard CI test job does not exercise this cpp-gated test because it
does not enable cpp; compile-only coverage does not reproduce the runtime check.

Recommended follow-up (owner brett19 via PR #737, which already touches this
file — that overlap is why this is diagnose-only):
- Decide policy for fp-residue keys ("values that should be zero"): either a
  documented `AbsFloor` for `vertex.surface.net` (~1e-15 would cover the observed
  residue while keeping real signal, note it in the unit notes per the gate's
  abs-floor procedure) or an explicit near-zero rule in the test's comparison.
  Do NOT loosen `REL_TOL`.
- Or pin the inline test to a fused-math-consistent comparison context (run the
  fixture both profiles? not portable) — the abs-floor route is simpler.
- Independently, consider enabling at least one cpp-feature smoke test in CI so
  this class of failure is exercised on main.

No repo source change, no test change, no tolerance change was made for this.

### Real-solid fillet checkpoint: corner vertices are outside the pseudo owning list (2026-10-24)

`blend.fillet_executive_p3`, fixture `real_solids.json`, case
`real-box-first_topology-actual-session`: a 10×20×30 box, edge 0, radius 0.5.
Actual shared setup and corner creation matched. The first direct Nx1 topology
operation initially exposed seven integer mismatches: native `GetVertices`
returned only the pseudo shell placeholder (1), while Rust also linked the two
corner-owned fillet vertices (3). Their owning-list labels likewise differed.

The producer now allocates those vertices into the arena without linking them
into the B-rep's owning list. All original checks remain, with additional actual
FV→corner, edge→corner and edgeuse→fillet-geometry backlinks. Four paired setup/
corner/topology fixtures pass exactly; the full API fixture remains deferred.
The direct topology checkpoint stops before vertex geometry and omits smoothing
on both sides; it does not claim the full executive prelude or a finished fillet.
### 2026-10-24 — Nx1 corner topology co-port for the live fillet session

The real box-edge session creates two `NByOne` corners, then reaches the missing
`MakeCornerTopology` producer. Neither `SmFilletNx1Corner::MakeCornerTopology` nor
`sm_FindSurfaceExtensionDistAtCorner` is assigned in `harness/callgraph/units.json`.
This follow-up co-ports the ordinary Nx1 producer and its three static side-edge /
convexity helpers into `blend::fillet_corner::topology`, tracked with corner p3.
It borrows the actual target, pseudo-B-rep, loaded solver and corner identity.
The first real-box topology test retains two classified corner vertices and one
end rib bound to the actual target end face; native paired-prefix proof is separate.

Mixed-convexity extension remains unported at the actual missing
`sm_FindSurfaceExtensionDistAtCorner` call, after the native two-vertex / rail prefix.
Its `blend.fillet_corner_p1` tracking label is an explicit proposed home for that
unassigned helper, not a claim of prior callgraph ownership. Keep this continuation
visible and both corner units partial. No fake radius, solver table or extended
surface is supplied. The native pseudo owning-vertex list differs from corner-owned
fillet vertices; compare the two traversals separately.
