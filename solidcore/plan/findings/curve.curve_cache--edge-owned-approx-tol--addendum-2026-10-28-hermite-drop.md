# topology.edgeuse_p3: real Hermite-edge UV-trim cache coverage (2026-10-28)

Updates: `plan/findings/curve.curve_cache--edge-owned-approx-tol.md`.

Unit: `topology.edgeuse_p3`. PR: #1428, built on #1320's published implementation and canonical main `abf00104c86d42d53a6f93ee70b4145c2145622e`.
Reference: SMLib v1.0.1, `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.

## Gap and independent input

#1320's completion table left `CreateUVTrimCurve` without a real face-owned Hermite/offset edge whose original curve cache depends on the edge tolerance. Existing B-spline-derived spans skip the distance test; a `CrvOnSurf` copy can avoid the original edge curve's cache, and even an interior constant-v Hermite can take `DropIsoCurve` before cache acquisition.

The new `hermite_sheet_face` source (shared block kind 15) takes only `scale` and `edge_tol`. Rust and native builders independently derive a cubic Hermite base C and the degree-3-by-1 sheet S(u,v)=C(u)+vD. The UV loop is (.2,.2), (.8,.7), (.8,.9), (.2,.4): two non-isoparametric diagonals and two isoparametric controls. Both use `MakeFaceWithCurves`, four real Hermite curves, unit domains, no preinstalled trims, face tolerance 1e-7, and the exact supplied edge tolerance. No C++ geometry or cache flags feed the Rust-built scene.

The C++ exporter represents Hermite curves as opaque, so C++ preparation now uses its native counts without importing an irrelevant Rust graph. Rust non-file builders preserve their own cache state; only the pre-existing file-reader path retains the legacy graph/cache adapter. This PR does not claim to migrate that reader.

## Native path proof

A throwaway native probe called the actual kind-15 factory. For scale 1e6 and edge tolerance 1e-10, primary diagonal eu0 had native type 14034 (`SmHermiteCurve`), no curve cache before the unmodified operation, `SM_SUCCESS`, a real returned UV curve, path 16, and a cache whose curve pointer was the original edge curve. A diagnostic-only fresh control cleared the curve owner; neither builder does that.

| Original curve owner during drop | Cache spans | Reported maximum surface distance |
| --- | ---: | ---: |
| Owning edge retained | 8192 | 2.5029282625656179e-10 |
| Owner cleared in throwaway control | 4 | 1.2628124321924076e-10 |

Both returned the same sampled UV pace to about 2.02e-13. The observed maximum sampled edge-to-lift gap and native reported deviation were about 2.48124e-7 at scale 1e6. This is **not** a claim that the lift is within the input edge tolerance; the reference's approximation is preserved.

## Real comparison and sensitivity

Six deliberate cases cover scale 1 / tolerance 1e-3 and scale 1e6 / tolerance 1e-10, primary and mate uses, diagonal and isoparametric paths, and optional diagnostics. They compare the complete returned UV record and the existing topology state. Opt-in `sample_lift` additionally compares the actual edge curve type and five edge XYZ / returned UV / lifted XYZ samples at the same edge parameter, without attaching or modifying the returned curve. Omitted/false sampling preserves existing case serialization, RNG calls and outputs.

Observed focused runs:

```text
PASS topology.edgeuse_p3 cases=6 passed=6 failed=0 deferred=0 max_rel_err=0e0 with_values=6 no_values=0 vacuous=0
SENSITIVE topology.edgeuse_p3 perturbed=6 detected=6 unperturbed=0 deferred=0
```

An existing `CrvOnSurf` Create case (sampling omitted) passed bit-identically. Four existing constructed binormal cases also passed bit-identically with their own Rust cache state. Three new FFI guard tests passed. No full local sweep or exact-base gate is claimed; canonical CI remains the merge gate.

## Review repairs and error-path proof

- A lifetime-counting throwaway subtype reproduced four leaked input curves when scale 1e-12 made native surface-degeneracy checks reject before consuming them (`SM_ERR_INVALID_INPUT`, 1007). Cleanup now frees remaining non-null, unowned array entries, respecting entries the factory deleted/nullified and curves already owned by edges. The same rejection then left zero live inputs; successful scale-1 construction kept four until B-rep deletion, then zero. A near-degenerate scale-5e-7 rejection also left zero.
- Shared guard consumers were traced through `sc_edgeuse_graph.h` / its `apply` bridge and the alternate `sc_topology_fixture.h` dispatcher. Both dispatch kind 15. An actual mirror fixture export produced one face, four edges and four vertices rather than the unrelated wire network. Native `Mirror` itself returned status 1001 on this bare Hermite sheet; successful mirroring is not claimed or changed here.
- Reserved shared kinds 10 through 14 remain rejected. The new guard uses existing finite bounds, positive scale and non-negative edge tolerance, including zero; no comparator floor, known mismatch or kernel numerical behavior changes.

## Remaining independent work

`DeleteTopologicalVertex`'s two-edge coincidence cache needs its own meaningful face/edge fixture. #1320's lazy edge-owned ellipse parameterization paths remain separate. No issue-closing link is claimed for either scope.
