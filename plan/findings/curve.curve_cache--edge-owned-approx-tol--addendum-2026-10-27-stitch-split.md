# heal.stitch_p1: split-point cache uses its owning edge tolerance (2026-10-27)

Updates: `curve.curve_cache--edge-owned-approx-tol.md`.

Units: `heal.stitch_p1`, caller `heal.stitch_p2`. PR: #1355; reuses #1320's
curve owner-tolerance metadata and #1297's stitch harness/digest.

## Observed gap

`sm_TestAndSplitEdge` (`SmStitch.cpp:546`) drops the point on the edge's
curve before deciding whether to split. The curve cache uses
`SmTol::GetApproxTol3d`, half the actual edge tolerance, independently of the
caller's stitch/snap tolerance. The Rust adapter held that edge but had not
recorded its tolerance for the cache builder.

Small-scale initial inputs agreed and did not establish a discrepancy. The
final five fixtures in `fixtures/heal.stitch_p1/owner_zone_tol.json` include
a rounding-sensitive Hermite edge at scale1e6, edge tolerance1e-10 and caller
tolerance1e-5 (`ozt_hermite_hook_off_center`). Before the fix that case fails:
native output length62,169, Rust105; the other four controls pass. Both sides
construct their own real Hermite wire, without transporting C++ geometry or
substituting a B-spline. The compared output contains actual drop/refusal
results and computed auxiliary-span geometry, not an echoed tolerance value.

## Repair and scope

`test_and_split_edge_with` borrows the actual edge record once, records its
zone tolerance on its own curve through the existing metadata API, and uses
that same value in the unchanged later `max(stitch_tol, edge_tol)` test.
There is no extra curve copy, allocation, arena lookup or numerical formula
change. Both direct and contextual stitch paths share this helper.

The optional Hermite-only cache probe reads the native curve's attached cache.
Rust requests the post-call curve's cache through the solve's existing builder,
reusing its memo when valid and supplying no separate tolerance override.
The new fixtures compare refusal paths plus cache geometry; they do not prove
a successful nonlinear wire split. Existing topology mutation comparisons
from #1297 remain intact. False optional fields are omitted from serialized
generated inputs, preserving the original seeded corpus.

## Focused native proof

Upstream1.0.1 `3fd5b9548d19`, matching headers/libraries, macOS ARM64:

- After the fix all five new cases pass at zero relative error. The sensitive
  case now compares all62,169 native values; controls compare105 or195 values.
- Perturbation detects all five cases (5/5).
- `heal.stitch_p1`:456 cases,447 passes (446 with values), zero failures or
  deferrals, nine inherited known mismatches.
- `heal.stitch_p2`:839 cases,826 passes (825 with values), zero failures or
  deferrals, thirteen inherited known mismatches.
- A throwaway Box/probe input is explicitly rejected by both runner paths
  because this probe is limited to Hermite edges; the throwaway was removed.
- Changed Rust files pass formatting; the changed C++ shim passes syntax
  checking and actually runs in the native comparison.

Both units stay partial. The22 region-nesting mismatches inherited from
#1297 are unchanged and remain visible; no registry, waiver, skip or comparison
tolerance was added, removed or loosened by this slice. Repeated solves after
independent edge-tolerance edits are not newly covered here.
