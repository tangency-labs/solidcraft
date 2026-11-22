Updates: WR-737

#### WR-737 (update, 2026-11-04). The five sweep cases, measured against the bound; confirmed on the C++ reference
- **Severity:** Wrong result
- **Area:** Surfaces and volumes

- **Why:** main's difftest sweep has reported five `surface.surface_assert_valid` cases on `values.out.len` since solidcore#2642 made the rounding bound the default: `gen0528`, `gen0812`, `gen1016`, `ovf0160`, `ovf0276`. The base record said whether any of the unit's cases now differ was unmeasured; this is that measurement, asked for in solidcore#2675.
- **Confirmed:** yes, for three of the five, on the difftest's own C++ reference: `gen0528`, `ovf0160` and `ovf0276` of `surface.surface_assert_valid`. There the C++ emits `SmBSplineSurface` test #5 on a collapsed row whose steps are one ulp of the coordinate and inside the rounding of their own two points, with the numbers below. The base record's `- **Confirmed:** no` stands as written: a merged record is not edited (AGENTS.md section 4b), so this line is the confirmation. Note that `check_cpp_defect` (`crates/difftest/src/known.rs`) looks for `- **Confirmed:** yes` in the base record only and skips any file whose name contains `--addendum-`, so this line does not make a `cpp_defect = "WR-737"` entry pass its id check; see **Not parked** below, where that is not the binding constraint anyway.
- **How:** main at `aa4968f24`, `cargo run --profile difftest -p difftest --features cpp -- --unit surface.surface_assert_valid --filter <case>`, one run per case against the built C++ reference. Each case reproduces on its own, so the disagreement does not depend on the other 2365 cases.
- **What the lengths are:** the runner flattens its result to `[ok, reports, then per report 13 numbers and 5 length-prefixed texts, last_add]`. A report's size is therefore **not fixed** — each of its five texts contributes its own length plus one — so a length alone does not give a report count. The counts below for the three measured cases are read from the vector's own `reports` field, not from its length. For the two unmeasured cases, `149` equals the length of the one-report results whose texts are the test #5 labels, which is why one report is the reading, but it is a reading of the length and not a count that was observed.

| case | surface | coordinates | `out.len` C++ | `out.len` solidcore |
|---|---|---|---|---|
| gen0528 | cone, top radius 0 | origin 1.6e4 | 290 | 146 |
| gen0812 | sphere r 147309 | centre 1.2e5 | 149 | 3 |
| gen1016 | sphere r 1127997 | centre 9e5 | 149 | 3 |
| ovf0160 | sphere r 2134351 | centre 1.5e6 | 149 | 3 |
| ovf0276 | sphere r 25118 | centre 7.6e3 | 149 | 3 |

- **Three cases measured per report and per step.** Their `reports` fields were read directly: the C++ emits one report more than solidcore, and the extra one is `SmBSplineSurface` test #5, `Bad NonMonotonic CPts`. The step it reads as a direction is one ulp of the coordinate, far inside the rounding of the two points it lies between:

| case | collapsed row | step | bound | distance from origin | bound / step |
|---|---|---|---|---|---|
| gen0528 | apex column, 8 steps | 3.638e-12 | 3.864e-11 | 3.480e4 | 10.6 |
| ovf0276 | pole row, 8 steps | 3.638e-12 | 3.046e-11 | 2.743e4 | 8.4 |
| ovf0160 | pole column, 8 steps | 1.164e-10 | 2.723e-9 | 2.453e6 | 23 |

  `gen0528` is the clearest: the C++ emits two reports, test #5 and `Bad Cone Radius` (`15012`, test 0, value `-1.8189894035458565e-12`); solidcore emits one, `Bad Cone Radius`, with the same value. The single difference is test #5. So these three are this entry's defect, reached from surfaces the case generator builds rather than from a moved record: a cone whose apex row and spheres whose pole rows are collapsed, placed far enough out that one ulp of a coordinate is above an absolute `SM_EFF_ZERO`.
- **Two cases not measured that far.** For `gen0812` and `gen1016` an isolated harness (`ffi::eval` on the case's block, then `assert_valid` on the same surface) reports **no** report from either side and finds no step inside the bound, where the difftest run over the same block gives `cpp=149 / rust=3`. That harness therefore does not reproduce the difftest's C++ answer for these two, and the reason is not known; a setup step the harness makes per case is the candidate (`crates/difftest/README.md` names latched C++ process state as the reason every unit gets its own child process). For these two the lengths above are measured and **which report is missing, and how long the step is, is not**. It is not inferred from the other three.
- **Not parked, and why.** `crates/difftest/README.md` ("Regression gate"): "A baseline pass may not become `known_mismatch`, `cpp_defect` or parked, so marking a passing case never waives it." All five passed before solidcore#2642, so they are baseline passes and a `cpp_defect = "WR-737"` entry would not make the sweep green whatever the id check said. No `known_mismatches` entry is added here. How the sweep is brought back to green — a new baseline, or the rule itself, which solidcore#2647 changed for the kernel but not for this gate — is a maintainer decision and is not anticipated by this record.
