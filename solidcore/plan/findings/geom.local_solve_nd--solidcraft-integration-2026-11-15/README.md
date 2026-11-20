# November 15 integration evidence

These compact artifacts support the accompanying finding and SolidCraft PR #14102.
They archive measurements from baseline `be6ecb06420d501a8362d6de3d0de7ef324dafb9`
and the paired correction, with Rust 1.99.1. Numeric values and image bytes were
preserved; local absolute paths were replaced by relative provenance identifiers.
No helper program, complete B-rep Debug snapshot or build output is bundled.
Paths beginning `unbundled-validation/` identify historical private-run artifacts,
not files distributed in this directory. Source paths beginning `crates/` are
relative to the `solidcore/` subtree.

* `issue948-root-cause.{md,json}`: exact endpoint event, root orientation,
  unchanged `0.002` guard, operand/control measurements, and WR614 option matrix.
* `wr383-goldens.{md,json}`: five P1 endpoint-policy cases and two P8 saved-tail
  cases, with exact oriented fractions and a rational whole-interval Bernstein
  bound `0.333 < 0.5`. The two P8 rows exercise identical geometry; the bound
  certifies approximate coincidence throughout the added range, not exact equality
  or completeness.
* `wr383-propagation.{md,json}`: independent plane-trim certificate and matrix
  transform validity audit. Full UV-support Debug strings were removed; domains,
  endpoints, the input matrix and all failed-report values remain.
* `plane0599-certificate.json`, `plane0599-{before,after}.png`: exact-dyadic
  affine clipping and whole-span distance bounds. The V axis is explicitly
  expanded in the scientific plots; the two plots have identical axes.
* `mat0076-main-ci-{off-off,both-old-options-on-on}.png` and
  `mat0076-render-provenance.json`: actual body meshes with a common frame,
  viewed along the normal derived from the nearly flattening matrix. These
  pictures retain baseline-CI option provenance. All typed vertices, supports,
  UV trims/endpoints and complete Level-2 checks agree exactly with the
  corresponding main/paired goldens measurements. Similar images and successful
  meshing do not establish validity. Valid input transforms into invalid output;
  this remains tracked in [#17271](https://github.com/tangency-labs/solidcraft/issues/17271).
* `wr383-schematic/`: official VectorCraft 0.24.1 editable source, live-text SVG,
  inspected PNG, drawing command data and provenance. It explains the oriented
  range-end guard; it is not an additional body correctness certificate.

The plane result is bounded by `0.4923024273024897 mm < 0.5 mm` including the
evaluation envelope, and retains the exact projected inside intervals to a
geometric endpoint gap below `8.275e-15 mm`. Its isolated point is an approximate
contact, not an exact boundary intersection. The matrix result is deliberately
not blessed as a correct body: main/paired have 16/17 physical AssertReports,
or 17/18 `BrepCheck::failure_count()` lines after the synthetic skipped-check line.

`manifest.json` records the selected files, exact archived sizes/hashes and
sanitized source identifiers. It omits its own hash. All five geometric/diagram
PNGs were opened for visual inspection; SVG/source preserve the declared font,
with the documented preview fallback in the schematic README.

* `validation.json` and `golden-delta.txt`: completed paired checks and the exact
  per-unit comparison against main. The application failure remains explicit;
  successful kernel checks do not clear that application gate.

* `gpu-fixture-audit.md`: clean-main comparison for the separate cache-assertion
  failure tracked by #17280. No passing gate is inferred from the diagnostic control.
