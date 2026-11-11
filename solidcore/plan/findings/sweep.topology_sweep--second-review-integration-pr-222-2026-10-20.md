# sweep.topology_sweep — second review integration (PR #222, 2026-10-20)

Merged canonical main through `95b823ed`, preserving both sides of the appended
findings and the part 7 integration notes. `SweepAccess` now calls the real ports
for `MakeShellVertex`, `MakeVertexLoop` (default opposite orientation),
`MakeEdgeInFace`, `RemoveFaces` (actual editing flag), `Transform` (default scale
and tolerance flags), `SmFace::SetTolerance`, HCR construction/visibility and
silhouette retrieval, `HasLaminaEdge`, and `Stitch::do_stitching`. Face tolerance
converts the double with the owning kernel's `zone_tol3d`, then uses the native
true/true flags, including cascading to boundary edges and vertices.

The shell/vertex-loop borrowing callback supplies persistent notification records,
attributes, identity resolution and the owning mutable notification context. The
part 7 face builder and copy path now receive their added `NotifyEnv` argument;
its curve-owner extension carries that environment through unchanged. The dual
B-rep borrowing callback passes a mutable destination kernel, matching the updated
part 6 `copy_edges` API. Integrating adapters must provide those services and
retain the entire `Hcr` session until its scene id is deleted. `Hcr::scene_brep_mut`
exposes the exact owned scene for mutable source borrows; no scene clone is used.
The concrete production adapter remains tracked in #255.

The sweep stitching callback implements `StitchCallback`, qualifies local ids by
the actual B-rep and delegates to the existing sweep decision/map-update routine.
Stitching uses the native true/true constructor options, default candidate lists,
no optional min-gap outputs, and the caller's manifold flag. A regression checks
map changes and a distinct B-rep namespace; default-service regressions check the
new dependency boundaries, editing refusal, empty-face-removal mark advancement
and release, identity transform, and context restoration after a stitch deferral.
Negative, default (non-default system zone 0.125) and positive face tolerances
check face-to-edge-to-vertex cascading.

The runner now compares untruncated `f64` distances and successful frames for ops
8/9 on both sides, in addition to the existing integer state. A native FFI
regression checks a fractional translation frame. Construction/HCR/stitching
continuations behind their own dependencies are still not claimed as differential
coverage; the default HCR constructor stops at the part 3 B-rep copy, and stitching
stops at the part 2 B-rep pre-edit notification.

The part 5 adapter also exposes `make_edge_in_face_with_uv_curve`, preserving the
polymorphic UV curve (including a real line). Its original B-spline-only Rust
entry point delegates after wrapping its curve, preserving its API and control
flow. The sweep does not approximate a line/conic as a B-spline or reject that
valid native subtype; a line-UV default-service regression reaches the real
`FindVertexSector` dependency boundary.

`MakeEdgeInFace` preserves the editing-disabled early return before checking its
unused vertex owners. The regression supplies foreign-B-rep vertex handles while
editing is disabled; active editing still refuses cross-B-rep handles instead of
resolving their local ids in the wrong arena.
