# sweep.topology_sweep: bottle validity after UB-170 (2026-10-31)

PR: #1937. Units: `sweep.topology_sweep`, `sweep.trans_sweep_geometry`.
Default behavior reproduces the native defect. The opt-in repair below removes the
new sweep defects; it does not repair pre-existing input gaps or count as a new
native parity pass.

Input: pinned v1.0.1 `TestFiles/pt_TestFiles/Fillet/bottle.smb`, whole-body
translation `(0,0,4)`, six null target arrays, stitching and merge disabled,
one repetition, default kernel fixes and sweep epsilon.

The Rust reader and sweep now reproduce the private native diagnostic's result:

| Observation | Rust | Private native diagnostic |
|---|---|---|
| Before pointer validation | 1000 | 1000 |
| Before AssertValid return / reports | true / 8 | true / 8 |
| Sweep status | success | 1000 |
| After pointer validation | 1000 | 1000 |
| After AssertValid return / reports | false / 6 | false / 6 |

Three after-sweep reports are surviving input edge/surface tolerance gaps. The new
reports are surface type 15001 tests 1 (flat corner) and 6 (partially coincident
boundary), and trimming type 12225 test 2 (V endpoint gap). The native V gap is
`0.06984669271518007`, tolerance `0.0069846692715180075`; Rust's formatted report is
`0.069846693`, tolerance `0.0069846693`. No tolerance was increased.

The native reference without the private UB-170 correction crashes. The private
object changes only the `SmHCR.cpp` edge-copy loop bound to the edge-array size.
It is **not** the official parity reference. Rust's merged UB-170 correction uses
the minimum of the vertex and edge counts; the matching bottle results do not
establish equivalence of those two bounds on other inputs.

The reduced wire-edge case was also run directly against the **unmodified** native
libraries, without the private HCR object: `SmLine::CreateLineSegment`,
`SmBrep::CreateWireEdgeFromCurve` with interval `[0,1]`, then
`SmTranslationalSweepGeometry::EdgeSweepHigher` with analytics disabled. It returns
1000 and a surface whose AssertValid returns false with exactly type 15001 tests
1 and 6, matching the reduced Rust test. This case does not invoke HCR.

The failed extrusion's generator goes from
`(-0.30118110236220502,0,4.3331526241915306)` to
`(-0.30118297798525001,0,4.2633059315015203)` over native interval
`[0.71410363017993395,0.78395032289511402]`. Eleven native samples lie nearly on
that line. Its transverse displacement is about `1.876e-6`, below the B-rep's
`1e-5` tolerance. A Rust wire edge made from those endpoints reproduces both
surface failures without the bottle reader or HCR. This localizes the surface
failure to the accepted nearly parallel sweep, but does not prove that rejecting
or omitting the edge would preserve the whole swept solid.

Repeated validation is significant: a second Level0 check of the **unchanged**
Rust result returns pointer status 1000 but adds failed reports for trimming tests 2, 3 and 4, and skips
AssertValid. The first check populated derived UV curves. The additional
orientation / zero-area reports identify face 101 in this traversal; the bad
extrusion is face 77, loop 117. These local arena indices are diagnostic, not stable
fixture identifiers. The repeated-check behavior has not been tested in C++.

## Opt-in repair

`Kernel::fixes.translation_sweep_regularize` (off by default) regularizes a
non-merged, in-place whole-body translation after its final repetition. It tests
newly swept higher faces for degeneracy at one quarter of the existing body
tolerance and squeezes them using the existing notification-aware Euler operation.
It also deletes isolated open-edge inner slit loops: exactly two opposite uses
of the same edge, four uses across the mates, and no adjacent different face.
It restores editing state, refreshes derived topology caches, and clears result
mappings to deleted entities even if an Euler operation fails.

For the bottle, it squeezes one narrow extrusion and removes 40 inner slit edges.
The result retains 84 faces, 317 edges, 377 vertices and two regions. No tolerance
is increased. The first Level0 check returns pointer status 1000, no pointer
reports, AssertValid true, and the three surviving input edge/surface gap reports.
The repeated check returns true with no reports. The initial reports are checked
against the corresponding input reports, not filtered out of validation.

Independent validation of the Rust-written ASCII output with the unmodified
native libraries gives the same results: first Level0 true with three inherited
reports, repeated Level0 true with no reports. Level2 still returns false with
one vertex gap, `4.7135458964e-5` versus tolerance `2e-5`. The same Level2 defect
is present in the unswept native input (which has nine reports). Thus this repairs
the introduced surface and loop defects, not every defect in the source bottle.

The first squeeze experiment failed because B-rep editing was disabled; enabling
editing makes the Euler operation succeed. Replacing the support with a plane
was rejected: the face remains geometrically degenerate at the existing tolerance.
No plane replacement, tolerance increase, or validation suppression is included.

## Reproduction

`crates/solidcore/tests/issue_1937_bottle_sweep.rs` checks default parity and the opt-in repair.
The reduced test runs normally. The full-body tests are ignored only because it
requires the untracked upstream fixture; run it explicitly:

```sh
CARGO_BUILD_JOBS=2 nice -n 10 cargo test --profile difftest -p solidcore \
  --test issue_1937_bottle_sweep -- --include-ignored --nocapture
```

The default test pins the native failures; the opt-in test pins the repaired
topology, surviving input reports, repeated validation, and live result mappings.
Ordinary and near-parallel box sweeps supply additional regression coverage.
The caller supplies distinct topology identities to both the reader and
the borrowed `SweepStore`, following the whole-body difftest runner. The imported
B-rep's own key is assigned the caller's body handle before registration; kernel
identity checks are unchanged.
