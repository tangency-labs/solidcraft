Updates: WR-8

#### WR-8 (update, 2026-10-31). Opt-in exact box image for aligned `EvaluateBoundingBoxSimple` (`#2148`)

- **Location:** `source/SMLib/src/SmTransform.cpp:1224`-`1359` (`SmTransform::EvaluateBoundingBoxSimple`, aligned branch `:1283`-`1319`).
- **Confirmed:** yes, reproduced by unit tests in `crates/solidcore/src/geom/transform.rs`.
- **Defects in pinned `v1.0.1`:** For a simple affine transform $P(u, v, w) = o + u\mathbf{x} + v\mathbf{y} + w\mathbf{z}$ and an axis-aligned `pOptParamPseudoBox` (`sThisParamPseudoBox.AreAligned(*pOptParamPseudoBox, alAxisMap)`):
  1. **Reciprocal scale (`:1293`-`1295`):** scales `crParamBox`'s `u, v, w` intervals by `1 / |x|`, `1 / |y|`, `1 / |z|` instead of multiplying by `|x|`, `|y|`, `|z|`.
  2. **Projected-dot translation (`:1293`-`1295`):** translates by `o · x`, `o · y`, `o · z` instead of the origin's coordinates along the unitized transformed basis vectors `o_u * |x|`, `o_v * |y|`, `o_w * |z|` where `(o_u, o_v, o_w) = T_lin^-1(o)`. Under non-unit orthogonal scaling `o · x = o_u * |x|^2`, and under shear (`x, y, z` non-orthogonal) `o · x` mixes components from the other axes.
  3. **Inverted 3-cycle permutation (`:1298`-`1311`):** `sThisParamPseudoBox.AreAligned(*pOptParamPseudoBox, alAxisMap)` sets `alAxisMap[k]` (for parameter axis `k` in `{0, 1, 2}`) to the signed 1-based slot of `pOptParamPseudoBox`'s parallel basis vector, so `sIvls[k]` belongs in `sTmps[|alAxisMap[k]| - 1]`. Upstream writes `sTmps[k] = sIvls[|alAxisMap[k]| - 1]`, which agrees for swaps and sign flips but applies the inverse permutation on 3-cycles (`[Y, Z, X]`, `[-Y, Z, -X]`).
  4. **Coupled `Circumscribe` cycle (`:1318`, `WR-5`) and singular axes (`:1276`, `:1293`):** `pProjBox->Circumscribe(sThisProjPseudoBox)` hits `SmExtent3d::Circumscribe`'s 3-cycle axis-map bug when the transformed basis itself is a 3-cycle of `{X, Y, Z}`, and singular axes (`rank < 3`) leave `sThisProjPseudoBox`'s basis unprojected while dividing by `0` when an axis column vanishes.
- **Opt-in (`Kernel::fixes.transform_aligned_bbox_image`, `#2148`):** default `false` preserves pinned `v1.0.1` parity in `Transform::evaluate_bounding_box_simple` and `Transform::evaluate_bounding_box_simple_with_kernel`.
  - When enabled and `param_pseudo_box` is aligned with `[X, Y, Z]`, `this_proj_pb.set_basis(sx, sy, sz)` succeeds, and `inv_transform_vector(to_origin())` returns `o_param`, the aligned path scales parameter intervals by `(|x|, |y|, |z|)`, translates by `(o_param.x * |x|, o_param.y * |y|, o_param.z * |z|)`, places axis `k`'s interval into basis slot `|axis_map[k]| - 1` (negated when `axis_map[k] < 0`), and circumscribes `this_proj_pb` into `proj_box` with `circumscribe_axis_map_by_basis = true`.
  - When the transform's linear part is singular (`set_basis` or `inv_transform_vector` fails), it falls back to the general branch (unbounded outputs when `!param_box.is_bounded()`, or 8-corner `evaluate_point` accumulation when bounded) without `NaN` or division by zero.
- **Wired callers:** `Volume::evaluate_bounding_box_simple` (`crates/solidcore/src/surface/volume/p2.rs`) forwards `k: &Kernel` to `Transform::evaluate_bounding_box_simple_with_kernel` for `VolumeKind::Transform`, propagating the fix through `Volume::evaluate_bounding_box` (including `orient_map` projections) and `Volume::map_pseudo_box`. Direct callers of `Transform::evaluate_bounding_box_simple` without a `&Kernel` keep default behaviour unless migrated to `evaluate_bounding_box_simple_with_kernel`.
- **Regression tests (`geom::transform::tests`):**
  - `wr8_non_uniform_scale_and_translation`
  - `wr8_three_cycle_param_basis_and_three_cycle_transform`
  - `wr8_shear_with_translation`
  - `wr8_partially_unbounded_and_singular_and_volume_dispatch`

`addendum_options: Kernel::fixes.transform_aligned_bbox_image`
