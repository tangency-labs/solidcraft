# geom.polar_box (green; no disagreements)

All 3030 default cases (30 fixture programs plus 3000 generated programs of 4 to ~30
commands on four box registers) agree bit for bit with upstream (`max_rel_err = 0`).
Upstream behaviour worth knowing:

- **`Union` reads an unset count.** When neither box is unbounded and the axis tests pass
  (`|b0 + b0'|^2 >= SM_EFF_ZERO_SQ`, `b0 . b0' >= SM_EFF_ZERO`) but one box is
  `SM_PS_UNINITIALIZED` (possible after `ReSet`, which keeps the stale basis),
  `GetBoundaryVectors` returns `SM_ERR_INVALID_INPUT` without setting its count and
  `Union` loops over that unset count (undefined behaviour). The Rust `union` /
  `union_assign` return `InvalidInput` there; the shim detects the case and skips the call.
- **`Union` aliasing.** `SmCompositeCurve.cpp:446` calls `p->Union(t, *p)`. The result
  is `ReSet` and seeded with the mid vector before `this`'s corners are read, so the
  original extent of `p` is dropped (only `t`'s corners and the mid vector remain).
  Ported as `PolarBox::union_assign`.
- **Region-to-arc relabel does not transpose.** In `AddVector3d`'s `SM_PS_REGION` case,
  when the x range collapses below `SM_EFF_ZERO_SQRT` the basis is rotated
  (`b1 <- b2`, `b2 <- -b1`) and the state becomes `SM_PS_ARC`, but the domain is re-set
  to its own bounds (`SetMinMax(UMin, VMin, UMax, VMax)`), so the arc's span stays in y
  while arc code reads x. Kept as is.
- **`Cross`/`Union` keep stale fields.** On their early `SM_PS_UNBOUNDED` exits (and when
  `Cross` adds nothing) the result box keeps its previous basis and domain. The port
  returns a fresh box; the difftest compares upstream through a fresh temporary plus
  `operator=`, which is what `PolarBox::assign` reproduces.
