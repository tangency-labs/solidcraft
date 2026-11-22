# heal.heal_data_p1 / p2 / p3 — architecture-associated cache records (2026-10-24)

Fresh full runs, with all outcome categories kept separate:

- `heal.heal_data_p1`: 495 cases, 432 passed, 24 failed, 11 deferred, 0
  `skipped(platform)`, 0 rejected, 28 known mismatches; `max_rel_err=1`.
  `sphere_low_op` (radius `0.001`, stage-2 face snapshot) has
  `values.out.len=2951/2961`; `sphere_big` (radius `1000`, swapped) and
  `closed_sphere_swap0` (radius `1`, unswapped) have `3031/3041`.
  These sequences include coincident-vertex caching and face-props snapshots.
  Existing known mismatches are not counted in the 24 failures.
- `heal.heal_data_p2`: 738 cases, 478 passed, 260 failed, 0 deferred, 0
  `skipped(platform)`, 0 rejected, 0 known mismatches; `max_rel_err=1`.
  `tiny_box` (sizes `[1e-4,1e-4,1e-4]`) has
  `values.out[3378]=0/12`; `flat_box` (`[1e4,1e-4,1]`) has `0/4` there;
  `duplicate_targets` (unit box, repeated edge/face/vertex targets) has
  `values.out.len=3032/3057`.
- `heal.heal_data_p3`: 478 cases, 450 passed, 28 failed, 0 deferred, 0
  `skipped(platform)`, 0 rejected, 0 known mismatches; `max_rel_err=1`.
  `review_valid_no_prior_targets` and
  `review_valid_remove_all_prior_vertices` (box `[1,2,3]`, cache props then
  update targets with operation 6) have `values.out.len=2833/2848` and
  `3369/3384`; `gen0001` (`sphere.smb`, cache/fix coincident vertices,
  voxels `[12,12,12]`) has `2856/2876`.

Class: architecture-associated histogram/voxel/cache-layout divergence, not a
tiny-value tolerance failure. UB-194 (`SmTArray.h:2395-2397`, used by
`SmHealData::Cache_EdgeProps`) explains short-edge histogram bucket changes;
UB-35 explains coincident-vertex voxel membership and resulting record sizes.
These mechanisms and older ARM64 reproductions are already documented under
General and the healer findings. The present samples alone do not prove the
first causal cache operation for every one of the 24/260/28 failures.
What would settle that remaining attribution: compare these exact records with
the same-revision x86-64 oracle and trace the first cache divergence per sample;
coordinate unsupported-oracle policy with #773 rather than introducing another
skip approach. No new upstream issue: UB-35 and UB-194 already cover these
conversion defects. No healer status or known-mismatch waiver was changed.
