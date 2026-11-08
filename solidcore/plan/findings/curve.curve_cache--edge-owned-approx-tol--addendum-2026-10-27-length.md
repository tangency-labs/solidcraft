# curve.curve_cache: common-edge length retains its owner tolerance (2026-10-27)

Updates: `curve.curve_cache--edge-owned-approx-tol.md`.

Unit: `curve.curve_p5`; caller: `topology.brep_p5` coincident-vertex combination.
PR: #1334, an integration follow-up to #1286 and the length work explicitly
released by #1320. Other caller sites owned by #1320 and the trim-cache work
in #1307 are unchanged.

## Evidence

The native CI sweeps for #1334 (run 40538974051) and #1335 (run 40541059175),
using upstream 1.0.1 (`3fd5b9548d19`), report the same unresolved
`SmTol::GetZoneTol3d(const SmObject*)` dependency:

- `boolean.topology_intersector_p2`: `gen0146`, `gen0180`, `gen0273`,
  `gen0332`, `gen0338`, `gen0386`, `gen0771`;
- `boolean.topology_intersector_p3`: `gen0008`, `gen0047`, `gen0049`,
  `gen0095`, `gen0163`, `gen0244`;
- `topology.brep_p10`: `gen0115`.

These cases were deferred, not numerical mismatches: Rust returned `Unported`
and compared no values. A narrowed native run of p2/`gen0146` reproduces the
deferral. LLDB stops in `Curve::length_base`, called from
`topology::brep::p5::combine_with_notifications` through the intersector's
`brep_combine_coincident_vertices` and `check_for_squeeze_vertices` adapter.

After the repair, the full native units pass: p2 910/910 (908 value-comparing),
p3 271/271, and brep_p10 1,397 value-comparing passes plus seven legitimate
rejections. All have zero failures/deferrals and maximum relative error zero.
Every one of the 14 formerly deferred cases now compares real values; brep_p10's
`gen0115` also compares 6,164 exact topology fields. Sensitivity detects the
perturbation of p2/`gen0146` (1/1).

The directly affected `topology.brep_p5` (797), `curve.curve_p5` (3,140), and
the host branch's `curve.crv_in_volume` (2,053) also pass with real values,
zero failures/deferrals and maximum relative error zero. No fixture, guard,
tolerance, comparison or known-mismatch entry was changed.

## Cause and repair

#1286 correctly replaces the length cache's copy-constructor clone with an
exact evaluation copy that retains the curve's owner. This exposes a missing
owner-tolerance handoff that the owner-resetting copy had concealed.

`SmBrep.cpp:1443` calls the common edge's `Length` with the new vertex tolerance
as the desired integration accuracy. Independently, the temporary curve cache
reads the curve's approximation tolerance from its owning edge. The integration
accuracy must not be substituted for that edge tolerance.

The crate-private `Curve::length_with_zone_tol` carries the actual common-edge
record's tolerance into the existing `CurveCache::set_owner_zone_tol_3d`
mechanism before tessellation. The line override, exact evaluation copy,
integration accuracy, numerical formulas and iteration order remain unchanged.
There is no additional geometry copy. Existing public length entry points
retain their signatures; unresolved owned curves at other callers still return
`Unported` rather than silently using a system tolerance.
