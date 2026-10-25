# geom.extent2d (green; no disagreements)

3069 default cases (69 fixture from `my_test_extent2d` + edge cases, 3000 generated)
agree bit for bit with upstream (`max_rel_err = 0`); a 30000-seed run is clean.
Upstream behaviour worth knowing:

- **`ContainsPeriodicPoint2d` can hang.** It calls `SmExtent1d::ContainsPeriodicValue`,
  whose `PeriodicWrap` steps one period per iteration; a period that is tiny relative
  to the point's distance from the interval (e.g. period `1e-9`, value `1e15`) runs
  for ~1e24 iterations in both C++ and the port. The generator only uses periods with
  `|p - min| / period < 1e4`.
- `Inversion` may write a partial normalised point (`-1` components) before returning
  `SM_ERR`; the port returns only the error. `IntersectWithInfiniteLine` always
  returns `SM_SUCCESS` and leaves the interval untouched when nothing is found (port:
  `None`).
- `GetU/VLeftEval` return TRUE unless `value > mid` (TRUE for NaN), kept as is.
- `SmTol::GetScaledZero(const SmExtent2d&)` is implemented in `geom/extent2d.rs`
  (wired from the `UNPORTED(geom.extent2d)` marker in `geom/tol.rs`) and covered by
  the `geom.tol` fixtures `fixtures/geom.tol/extent2d.json` (object kind 5).
