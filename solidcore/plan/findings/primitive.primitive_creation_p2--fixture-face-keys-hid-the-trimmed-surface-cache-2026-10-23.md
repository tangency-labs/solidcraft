# primitive.primitive_creation_p2: fixture face keys hid the trimmed surface cache (2026-10-23)

- **What.** The p2 runner's one-B-rep fixture gave new faces object keys from its own
  namespace (`FACE_TYPE`, `100000 + 97 * index`). The B-rep caches of the shell ray fire
  (`StitchAndOrient` -> region split -> `FindInfiniteRegion` -> `SmShell::RayFire`) cannot
  resolve those keys to a face, so the faces' surfaces were solved as untrimmed, which is
  the shortcut `SurfaceCache::get_tree` refuses for a face-owned surface. 12 cases
  (`pipe_sweep_curved_path` and 11 generated pipe/skin/planar-path cases) matched C++
  only because the record compares status and list sizes, not geometry.
- **How found.** With the same fixture, only the face keys changed to the production
  `notify_ref` formula, those 12 cases defer at `TrimSrfCache::build_tree_with_face for a
  face-owned surface` (passes 196 -> 184, `surface.trim_srf_cache` deferrals 6 -> 18).
- **Now.** The runner uses the production `PrimitiveModel`: 194 passes (the 10 cases that
  waited for `SmBrep::SmBrep` pass, the 12 above defer honestly), 0 failed, 114 deferred.
  The p3 fixture (#617) keys faces the same way; #615 moves p3 to the production adapter
  and reports the same 295 passes, so no p3 comparison depended on it.
- **Follow-up.** `PrimitiveModel::new_merge` still returns `Unported(SmMerge::SmMerge,
  boolean.merge_p2)` although the unit is ported (34 taper cases); `SweepStore` has the
  merge services.

### blend.fillet_executive_p1 — real solver loading and evaluation owner context

`LoadFilletSolver` now reads actual retained solvers, offset surfaces, edge curves
and continuity caches. Its executive registration resolves through a caller-owned
store; ordered pending edgeuses remain separate from completed corner objects.
Twenty-eight native comparisons cover repeated loads, incident edges, two executive
stores, closed circular wire, real G1 continuity, surface failures and a defined
failure after the native mutation prefix. All agree exactly. The original 61 law
and error-info comparisons also remain unchanged. The unit stays partial: corner
creation, other executive methods and legacy getter context bindings remain absent.

The G1 fixtures exercise the evaluation-copy correction already owned by PR #596.
The old `EvaluateBinormal` copies the face surface with `Clone` then restores only
its owner. `set_owner` clears the cached face tolerance, so a
subsequent owned-surface evaluation deferred at `SmFace::GetTolerance` although the
actual face record supplies that tolerance. With #596's `clone_for_evaluation`, the complete owner context and exact geometry
are preserved; all six G1 cases compare exactly without changing tolerances. The fixture supplies each
owned surface's actual retained face tolerance rather than clearing its owner.

The native loader reads uninitialized `pVerts` when an absent rail reaches an
existing filleted-vertex list. The shim guards that actual state before the call;
defined missing-surface errors still run. Rust reports the invalid native read
instead of inventing a vertex. No undefined native invocation is used as evidence.

The #596 dependency also enables an existing offset disagreement: generated
`offset.offset_geometry_creation` case `gen0730`, seed `9515163603058763486`, has
`state[87]` equal to zero in C++ and `1.1042240329208707e-61` in Rust. The full unit
has 831 passes, 66 deferrals and one failure on this integration. This remains a
real failure; the loader must not be marked ready while its dependency gate is red.
The loader contribution excludes #596's owned edgeuse source change.
