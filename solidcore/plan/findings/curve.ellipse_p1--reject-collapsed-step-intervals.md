# curve.ellipse_p1: reject collapsed ellipse STEP intervals (#428, PR #686) (2026-10-25)

- **Root reproduced.** The existing `step_interval_past_a_turn_collapses_as_upstream`
  regression already encoded the defect: a circle over `[0,360]` or `[0,180]` adjusted
  to `[360,540]` has both STEP ends clamped to one old NURBS parameter. A following
  NURBS reparameterization divides by that zero-length range and creates NaN knots.
  The replacement regression failed before this fix, then checks rejection, exact
  preservation of the NURB/domain/polar angles, and a subsequent finite edit.
- **Actual unchanged C++ confirmation (WR-211).** A standalone public-API probe linked
  against the existing pinned `bin/cpp/lib/libSMLib.a`, with the harness flags and no
  reference edits, produced:

  ```text
  old=[0,360] requested=[360,540] status=1000 step=[360,540] nurbs=[1,1]
  subsequent NURBS knots: 360 360 360 nan nan 540 540 540
  old=[0,180] requested=[360,540] status=1000 step=[360,540] nurbs=[1,1]
  subsequent NURBS knots: 360 360 360 nan nan 540 540 540
  old=[0,360] requested=[90,90] status=1000 step=[90,450] nurbs=[0.25,0.25]
  subsequent NURBS knots: 360 360 360 nan nan nan nan nan nan 540 540 540
  ```

  Native `1000` is `SM_SUCCESS` (normalized to `0` by difftest). Equal-angle input
  is expanded by `MakeNurb` into a full-turn analytic interval while its NURBS range
  is collapsed; accepting it as a harmless point arc would also preserve this defect.
  The probe deliberately stops before any surface clamp consuming the NaNs (#427).
- **Fix = safe refusal, as permitted by #428.** `Ellipse::adjust_step_interval`
  validates finite, ordered STEP endpoints and rejects a zero/non-finite converted
  NURBS span before storing the new domain or rebuilding any geometry. There is no
  new angular or size cutoff, no epsilon for span collapse, and no periodic extension
  algorithm. Supported edits retain their existing parameter conversions and numerics.
  An already-built shifted arc over `[360,540]` still accepts an unchanged STEP domain;
  four native controls cover that case in both orientations and circle/ellipse forms.
- **Actual revolution caller.** A circle-generator revolution rejects the shifted
  generator range in both UV orientations. Its STEP domain, generator and base NURBS
  stay unchanged, and a supported retry succeeds. `adjust_step_uv_domain` now stores
  its analytic domain only after the generator accepts its interval. This guarantee
  covers this rejection path; it is not a general transaction promise for all surface
  edits. The separate p3/extrusion `UpdateAnalyticalDomain` bodies can already have
  written their analytic domain before the new generator error; they are not rewritten
  here and their differing error results remain explicit below.
- **Coverage.** Three Rust regressions (one replacing the intentional-corruption test)
  cover the original full/half-circle inputs, invalid/overflowing/NaN endpoints,
  post-error finite editing, and the real revolution caller/retry. Twenty native
  fixtures add eight collapsing edits and twelve passing controls (quarter trims and
  already-shifted arcs), with proper constructors for both traversal orientations.
- **Local gates on main `94abd96b5` plus this fix:** fmt, default/native workspace
  Clippy `-D warnings`, 2075 kernel tests (two existing primitive integration tests
  ignored), wasm32, and 46 difftest tests plus its float-roundtrip regression pass.
  All twelve healthy native controls detect deliberate perturbations. Full macOS
  `--green` is not claimed because the previously recorded reference failures/hangs
  #448–450 remain; canonical fork CI requires maintainer approval.
- **Revalidated on main `94824377f` after PR #679 landed:** all Rust gates above
  pass again with 2075 kernel tests. A freshly rebuilt native runner retains every
  unit count in the following table (5 pass, 3 partial, 111 explicit known differences,
  no unexpected failure/deferred cases), and all twelve perturb controls are
  sensitive again. The rebase had no conflicts and preserved this fix's source.
- **Native comparisons on main `94abd96b5` plus this fix:**

  | Unit | Cases | Exact passes | Explicit known differences |
  |---|---:|---:|---:|
  | `curve.ellipse_p1` | 2051 | 2001 | 50 |
  | `curve.ellipse_p2` | 2107 | 2107 | 0 |
  | `surface.sphere` | 816 | 816 | 0 |
  | `surface.surf_of_extrusion_p1` | 2071 | 2057 | 14 |
  | `surface.surf_of_revolution_p1` | 894 | 894 | 0 |
  | `surface.surf_of_revolution_p2` | 1167 | 1167 | 0 |
  | `surface.surf_of_revolution_p3` | 26731 | 26684 | 47 |
  | `surface.torus` | 596 | 596 | 0 |

  There are 111 intentional reference differences: 50 ellipse programs (8 new
  fixtures, 42 existing generated programs), 47 existing revolution-p3 updates and
  14 existing extrusion-p1 updates. The caller cases include empty reconstructed
  generator intervals, degenerate domains, clipped out-of-arc ranges and subnormal
  ellipse geometry. They are **not 111 independently observed C++ NaN crashes**:
  the direct C++ probe above confirms the mechanism, while these are individual
  comparison differences caused by rejecting the empty/collapsed edit. All are
  recorded by exact case name; the three affected units stay partial. Five other
  native units pass all their cases exactly. No tolerance, reference implementation,
  input guard or generated case was changed or removed to classify them.

  Each original input is reproduced by its fixture source or generated seed below.
  Ellipse programs continue after a rejected operation, so their final state and
  later values can differ as well as the first error status. Surface caller records
  report C++ status `0` versus Rust `1007` (`InvalidInput`).

### `curve.ellipse_p1`

| Case | Reproduction source | First differing field | C++ | Rust |
|---|---|---|---|---|
| `collapsed_step_circle_180_reverse_0` | fixture:fixtures/curve.ellipse_p1/upstream.json#28 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `90` | `92` |
| `collapsed_step_circle_180_reverse_1` | fixture:fixtures/curve.ellipse_p1/upstream.json#30 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `90` | `92` |
| `collapsed_step_circle_360_reverse_0` | fixture:fixtures/curve.ellipse_p1/upstream.json#32 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `90` | `118` |
| `collapsed_step_circle_360_reverse_1` | fixture:fixtures/curve.ellipse_p1/upstream.json#34 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `90` | `118` |
| `collapsed_step_ellipse_180_reverse_0` | fixture:fixtures/curve.ellipse_p1/upstream.json#36 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `92` | `94` |
| `collapsed_step_ellipse_180_reverse_1` | fixture:fixtures/curve.ellipse_p1/upstream.json#38 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `92` | `94` |
| `collapsed_step_ellipse_360_reverse_0` | fixture:fixtures/curve.ellipse_p1/upstream.json#40 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `92` | `120` |
| `collapsed_step_ellipse_360_reverse_1` | fixture:fixtures/curve.ellipse_p1/upstream.json#42 (upstream issue #428 / WR-211: polar clamping must not create a zero-length NURBS interval) | `values.rec.len` | `92` | `120` |
| `gen0000` | generated:seed=2282642883389604638 | `values.rec.len` | `89` | `67` |
| `gen0015` | generated:seed=2282642883389604653 | `values.rec.len` | `69` | `58` |
| `gen0158` | generated:seed=2282642883389604796 | `values.rec.len` | `202` | `167` |
| `gen0345` | generated:seed=2282642883389604983 | `values.rec.len` | `169` | `134` |
| `gen0402` | generated:seed=2282642883389605040 | `values.rec.len` | `96` | `124` |
| `gen0470` | generated:seed=2282642883389605108 | `values.rec.len` | `122` | `60` |
| `gen0490` | generated:seed=2282642883389605128 | `values.rec.len` | `75` | `52` |
| `gen0494` | generated:seed=2282642883389605132 | `values.rec.len` | `51` | `52` |
| `gen0565` | generated:seed=2282642883389605203 | `values.rec.len` | `75` | `52` |
| `gen0660` | generated:seed=2282642883389605298 | `values.rec.len` | `55` | `56` |
| `gen0792` | generated:seed=2282642883389605430 | `values.rec.len` | `96` | `57` |
| `gen0921` | generated:seed=2282642883389605559 | `values.rec.len` | `93` | `58` |
| `gen0945` | generated:seed=2282642883389605583 | `values.rec.len` | `57` | `63` |
| `gen1028` | generated:seed=2282642883389605666 | `values.rec.len` | `69` | `58` |
| `gen1035` | generated:seed=2282642883389605673 | `values.rec.len` | `63` | `78` |
| `gen1039` | generated:seed=2282642883389605677 | `values.rec.len` | `55` | `56` |
| `gen1149` | generated:seed=2282642883389605787 | `values.rec.len` | `75` | `89` |
| `gen1173` | generated:seed=2282642883389605811 | `values.rec.len` | `57` | `58` |
| `gen1189` | generated:seed=2282642883389605827 | `values.rec.len` | `51` | `52` |
| `gen1259` | generated:seed=2282642883389605897 | `values.rec.len` | `56` | `57` |
| `gen1298` | generated:seed=2282642883389605936 | `values.rec.len` | `91` | `69` |
| `gen1303` | generated:seed=2282642883389605941 | `values.rec.len` | `91` | `69` |
| `gen1307` | generated:seed=2282642883389605945 | `values.rec.len` | `120` | `134` |
| `gen1308` | generated:seed=2282642883389605946 | `values.rec.len` | `56` | `57` |
| `gen1335` | generated:seed=2282642883389605973 | `values.rec.len` | `59` | `99` |
| `gen1372` | generated:seed=2282642883389606010 | `values.rec.len` | `142` | `132` |
| `gen1387` | generated:seed=2282642883389606025 | `values.rec.len` | `97` | `113` |
| `gen1397` | generated:seed=2282642883389606035 | `values.rec.len` | `104` | `183` |
| `gen1414` | generated:seed=2282642883389606052 | `values.rec.len` | `51` | `52` |
| `gen1438` | generated:seed=2282642883389606076 | `values.rec.len` | `55` | `56` |
| `gen1502` | generated:seed=2282642883389606140 | `values.rec.len` | `65` | `93` |
| `gen1543` | generated:seed=2282642883389606181 | `values.rec.len` | `55` | `56` |
| `gen1569` | generated:seed=2282642883389606207 | `values.rec.len` | `51` | `65` |
| `gen1585` | generated:seed=2282642883389606223 | `values.rec.len` | `68` | `96` |
| `gen1603` | generated:seed=2282642883389606241 | `values.rec.len` | `83` | `73` |
| `gen1651` | generated:seed=2282642883389606289 | `values.rec.len` | `53` | `80` |
| `gen1735` | generated:seed=2282642883389606373 | `values.rec.len` | `65` | `79` |
| `gen1796` | generated:seed=2282642883389606434 | `values.rec.len` | `51` | `65` |
| `gen1921` | generated:seed=2282642883389606559 | `values.rec.len` | `91` | `82` |
| `gen1943` | generated:seed=2282642883389606581 | `values.rec.len` | `57` | `58` |
| `gen1953` | generated:seed=2282642883389606591 | `values.rec.len` | `57` | `63` |
| `gen1989` | generated:seed=2282642883389606627 | `values.rec.len` | `62` | `61` |

### `surface.surf_of_extrusion_p1`

| Case | Reproduction source | First differing field | C++ | Rust |
|---|---|---|---|---|
| `review3_circle_0_360_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#237 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_circle_0_360_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#239 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_circle_-360_0_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#285 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_circle_-360_0_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#287 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_circle_-180_180_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#381 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_circle_-180_180_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#383 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_0_360_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#429 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_0_360_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#431 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_-360_0_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#477 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_-360_0_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#479 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_-180_180_infinite_sw1_upd1` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#573 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review3_ellipse_-180_180_infinite_sw1_upd2` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#575 (upstream review: ellipse generator UpdateAnalyticalDomain) | `values.out.len` | `188` | `0` |
| `review6_ell_5e-324_canonical_update` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#1072 (upstream review: extreme tiny generators) | `values.out.len` | `152` | `0` |
| `review6_ell_5e-324_infinite_update` | fixture:fixtures/surface.surf_of_extrusion_p1/review2_extra_cases.json#1080 (upstream review: extreme tiny generators) | `values.out.len` | `152` | `0` |

### `surface.surf_of_revolution_p3`

| Case | Reproduction source | First differing field | C++ | Rust |
|---|---|---|---|---|
| `r2_update_torus_1_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#362 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_1_[0, 0, 0, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#467 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_1_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#468 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_sphere_1_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#574 (upstream review 2 adversarial) | `values.out.len` | `351` | `0` |
| `r2_update_sphere_part_1_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#679 (upstream review 2 adversarial) | `values.out.len` | `299` | `0` |
| `r2_update_torus_0.0001_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#1914 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_0.0001_[0, 0, 0, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#2018 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_0.0001_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#2019 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_sphere_0.0001_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#2125 (upstream review 2 adversarial) | `values.out.len` | `351` | `0` |
| `r2_update_sphere_part_0.0001_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#2231 (upstream review 2 adversarial) | `values.out.len` | `299` | `0` |
| `r2_update_torus_100000_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#3465 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_100000_[0, 0, 0, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#3570 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_torus_sw_100000_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#3571 (upstream review 2 adversarial) | `values.out.len` | `499` | `0` |
| `r2_update_sphere_100000_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#3676 (upstream review 2 adversarial) | `values.out.len` | `351` | `0` |
| `r2_update_sphere_part_100000_[0.3, 0.3, 0.3, 0.3]` | fixture:fixtures/surface.surf_of_revolution_p3/review2_extra_adversarial.json#3782 (upstream review 2 adversarial) | `values.out.len` | `299` | `0` |
| `r3_upd_torus_False_[0.5, 0.5, 0.5, 0.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5585 (upstream review 3 adversarial) | `values.out.len` | `499` | `0` |
| `r3_upd_torus_False_[-0.5, -0.5, 1.5, 1.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5586 (upstream review 3 adversarial) | `values.out.len` | `499` | `0` |
| `r3_upd_torus_True_[0.5, 0.5, 0.5, 0.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5647 (upstream review 3 adversarial) | `values.out.len` | `499` | `0` |
| `r3_upd_torus_True_[-0.5, -0.5, 1.5, 1.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5648 (upstream review 3 adversarial) | `values.out.len` | `499` | `0` |
| `r3_upd_sph_False_[0.5, 0.5, 0.5, 0.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5709 (upstream review 3 adversarial) | `values.out.len` | `351` | `0` |
| `r3_upd_sph_True_[0.5, 0.5, 0.5, 0.5]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_adversarial.json#5771 (upstream review 3 adversarial) | `values.out.len` | `351` | `0` |
| `r3b_upd_torus_0_False_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#181 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_torus_0_True_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#184 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_torus_0_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#185 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_torus_1_False_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#187 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_1_True_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#190 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_1_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#191 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_2_False_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#193 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_2_True_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#196 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_2_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#197 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_3_False_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#199 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_3_True_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#202 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_3_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#203 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_torus_4_False_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#205 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_torus_4_True_[0.1, 0.2, 0.6, 0.7]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#208 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_torus_4_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#209 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_arc_0_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#215 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `r3b_upd_arc_1_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#221 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_arc_2_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#227 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_arc_3_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#233 (upstream review 3 adversarial) | `values.out.len` | `169` | `0` |
| `r3b_upd_arc_4_True_[0.5, 0, 0.5, 1]` | fixture:fixtures/surface.surf_of_revolution_p3/review3_update_bspline_generator.json#239 (upstream review 3 adversarial) | `values.out.len` | `277` | `0` |
| `rev2_update_torus_1` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#9 (upstream review adversarial (zero-width domains)) | `values.out.len` | `499` | `0` |
| `rev2_update_torus_2` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#15 (upstream review adversarial (zero-width domains)) | `values.out.len` | `499` | `0` |
| `rev2_update_torus_5` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#33 (upstream review adversarial (zero-width domains)) | `values.out.len` | `499` | `0` |
| `rev2_update_sphere_1` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#117 (upstream review adversarial (zero-width domains)) | `values.out.len` | `351` | `0` |
| `rev2_update_sphere_2` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#123 (upstream review adversarial (zero-width domains)) | `values.out.len` | `351` | `0` |
| `rev2_update_sphere_5` | fixture:fixtures/surface.surf_of_revolution_p3/review_adversarial_zero_width.json#141 (upstream review adversarial (zero-width domains)) | `values.out.len` | `351` | `0` |

## Review revalidation (2026-10-27)

Rebased onto main `7e6ef6679`. The conflict in the revolution tests preserves both
main's polar-curve identity regression and this change's rejected-generator-edit
regression. All 2,363 kernel tests, formatting, plain/native workspace Clippy with
warnings denied, wasm32, infrastructure tests and native registry tests pass.
The findings, upstream confirmation and all 111 known differences remain in
per-entry/per-unit files; the frozen archives are not reintroduced.

The prescribed `--profile difftest` run of all eight affected units has five
passing units and three partial units, retaining the existing 50 ellipse,
47 revolution p3 and 14 extrusion reference differences, with zero unexpected
failures or deferrals. No input, comparison tolerance, FFI guard or known entry
was changed for this revalidation.

A separate unoptimized Rust run reported two near-zero angle fields in
`surface.surf_of_revolution_p3/gen0598` (seed `4440996037211322594`): C++
`3.4905207278410467e-15`, Rust `1.893000648790168e-15`, in `out[30]` and
`out[34]`. The same case compares all 249 values and passes in the prescribed
optimized profile. The debug result is retained as a profile-dependent diagnostic,
not hidden by a new known entry or a relaxed tolerance. No numerical source change
was made to obtain the optimized result.
