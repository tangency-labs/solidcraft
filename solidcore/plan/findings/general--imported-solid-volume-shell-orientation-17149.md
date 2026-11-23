# Imported solid volume: shell orientation (#17149)

The two public KiCad DSUB models in #17149 expose inconsistent shell orientation.
The divergence-theorem volume integral requires an oriented closed boundary.
Repair that boundary without changing the volume integrator.

## Shared-edge orientation

The two DSUB files contain adjacent faces that traverse their shared edge in the
same direction. Normalizing a clockwise UV loop and reversing its shell side
preserves that physical traversal. Counting uses alone therefore accepted a
closed shell whose oriented boundary did not cancel.

Raw STEP topology gives these exact counts. Every listed edge has two uses.

| Model and shell | Faces | Edges | Equal-direction pairs | Relative face flips | Parity conflicts |
|---|---:|---:|---:|---:|---:|
| DSUB-9 outer | 160 | 324 | 26 | 13 | 0 |
| DSUB-9 cavity | 19 | 51 | 0 | 0 | 0 |
| DSUB-44 outer | 388 | 760 | 92 | 46 | 0 |
| DSUB-44 cavity | 10 | 24 | 0 | 0 | 0 |

Each shell is one connected component. The 13 and 46 relative flips match the
reported warnings. The ferrite's edge directions already cancel.

For an edge with physical traversal directions `a` and `b`, the repair solves
`flip_a XOR flip_b = (a == b)`. A traversal propagates these exact Boolean
constraints; a conflicting cycle refuses the shell. An inconsistent shell with a
nonmanifold edge or disconnected faces is also refused. No arbitrary pairing of
more than two edge uses is made. Already balanced inputs retain their existing
orientation behavior.

Parity determines relative orientation only. Its starting face can itself be
wrong, so keeping that face's side is insufficient. For a repaired solid, each
shell is extracted through the record's canonical-part operation and passed to
`mass::enclosure::volume_sign`. The outer shell must have positive volume and
cavities negative volume. An unsupported or undecided certificate refuses the
repair. The numerical `face_flux` estimate used by the existing cavity path does
not decide the repaired shells' sides. The final record is canonicalized again
because shell sides determine the edge and vertex walk.

This follows the oriented-edge invariants in `docs/validity-and-tolerances.md`.
The sign construction and its validated-quadrature references are documented in
`mass/enclosure.rs`; no new accuracy threshold is introduced.

An independent parametric integral on the unrepaired DSUB-9 record illustrates
the error: `integral n_z dA = 4.3429376112 mm^2`, rather than zero. Its z-flux at
origin zero is about `1353.24229696 mm^3`. Moving the origin to the bounding-box
midpoint, `z0 = 4.3 mm`, subtracts `4.3 * 4.3429376112`, giving
`1334.56766523 mm^3`, the reported result. Reorienting the 13 faces gives about
`1363.39414826 mm^3`. Area is unsigned, so it did not expose the defect.

## Reported models on the current baseline

The local KiCad library supplies the three public models named in the report.
The installed 0.44.1 CLI still reproduces both DSUB volume errors. The diagnostic
CLI with shell repair gives the reported reference volumes:

| Model | 0.44.1 volume (mm^3) | Repaired volume (mm^3) | Reported FreeCAD volume (mm^3) |
|---|---:|---:|---:|
| DSUB-9 | 1334.567665486977 | 1363.394148851036 | 1363.394 |
| DSUB-44 | 10369.232656756969 | 10523.244123797416 | 10523.244 |
| LairdTech ferrite | 309.174835892360 | 309.174835892360 | 309.187 |

The diagnostic build also contained circular-seam seating changes, subsequently
excluded. They are no-ops on DSUB-44. On DSUB-9, nine tube angles at `pi/2`
translate already aligned quarter grids from `1..5` to `0..4`, preserving the
trimmed geometry; four longitude-zero candidates are no-ops. Each face has one
candidate, and neither input needs added seams. These discarded parameter
translations can affect rounding but do not explain the volume correction.

Both DSUB areas are unchanged: 1938.419682203252 and 5112.942814840802 mm^2.
All three imports remain closed and use exact B-rep measurement (`smlib`).
Before/after snapshots were captured from the same view outside the repository.

The ferrite's reported 312.317 mm^3 result is already absent in 0.44.1.
Its current volume differs from the reported reference by about 0.004%.
Its topology is already balanced, so this repair leaves it unchanged.
This closeness does not establish that every ferrite discrepancy is resolved.
The rebuilt CLI's current STL exports give:

| Quality | Triangles | Signed mesh volume (mm^3) | Mesh area (mm^2) |
|---|---:|---:|---:|
| fine | 649654 | 309.092646768570 | 344.982400541897 |
| finest | 657440 | 309.101811479578 | 344.983920297391 |

These come from `solidcraft-cli run --in <ferrite.step> --out <mesh.stl>
--quality <quality>`. The binary-STL sum is `sum(a dot (b cross c))/6`;
triangle areas are `norm((b-a) cross (c-a))/2`. The exact query's area remains
342.510442066981 mm^2. The remaining ferrite mesh and area discrepancies are not
fixed by shell orientation. No mesh result decides the orientation repair.

The ferrite has five periodic torus seams represented by native degree-8
non-rational splines, not circles. Their projected parameters cross the ends of
the surface forms. Replacing these seams with circles would alter the input
geometry. A synthetic off-equator elbow exposes a related circular-seam coverage
defect, but repairing that path does not change the ferrite. That unrelated
construction is excluded from this shell-orientation patch.

This is a remaining import limitation, not evidence that the current public
volume API varies with origin. That API includes gap-closure strips and corners;
a bare-face flux integral omits them and is not the same construction.

The existing form API cannot request a five-quarter torus cover with an explicit
four-quarter period. `eval/form.rs::face_quarters` fixes a full period at four
quarters; `wound_surface` only supports cylinder/cone longitude. Own arcs only
apply below one full turn. Low-level arc/tensor-product pieces can represent five
quarters, but pcurve seam lifting derives periodicity from whether the spline
closes, and a five-quarter spline does not close. A correct ferrite coverage fix
therefore needs periodic torus-cover and pcurve-period support, or an exact split
into supported charts. Changing the quarter cap or relabelling spline seams as
circles is insufficient. No such support extension is forced into this patch.

The affected native seams are STEP edges `#276`, `#302`, `#719`, `#743`, and
`#769` on faces `#270`, `#296`, `#714`, `#739`, and `#764` respectively. Each is
a degree-8 polynomial spline with nine control points and end multiplicities
`(9, 9)`. This identifies the remaining construction for separate review.

## Analytic regression expectations

The existing signed-volume test file contains the regressions.
Their expected values are derived directly:

- A `2 x 3 x 5` box has volume `30` and area
  `2 * (2*3 + 2*5 + 3*5) = 62`. With base `(1, -2, 4)`, its centroid is
  `(2, -0.5, 6.5)`.
- Removing a centred unit cube gives volume `30 - 1 = 29`, area `62 + 6 = 68`,
  and the same centroid. Reverse each outer and cavity bound independently.

The box cases also require opposite traversals at every shared edge. A reversed
vertical face can leave z-flux volume unchanged, so volume alone is insufficient.
Both mass-query origins must return the same volume and world-space centroid.
The existing incorrect-`same_sense` regression must continue to face outward.
No golden or kernel-case expectation is blessed for these corrections.

## Validation

The CLI unittest `lane17149_dsub_regression.py` fails both reported DSUB cases
on installed 0.44.1 and passes both with the rebuilt CLI. Its absolute tolerance
is 0.0005 mm^3 because the report rounds reference volumes to three decimals.
The regression script and public fixture copies stay outside the repository.

The signed-volume regressions assert opposite shared-edge traversals,
hand-derived volume, area and centroid, and invariance under changing the
mass-query origin. Full crate tests, clippy and formatting run through buildq.
The combined diagnostic patch passed 145 tests (129 unit, 3 panic-fuzz,
13 signed-volume), including both retained box/cavity regressions; two optional
corpus tests were ignored. Clippy with `--all-targets -- -D warnings` passed.
Both root and `solidcore-step` formatting passed again after the unrelated seam
construction was removed. The retained shell repair and box/cavity tests are
unchanged from that passing run; the discarded seam path does not apply to boxes.

The final shell-only full-crate and clippy reruns waited more than 30 minutes in
buildq without starting. They were cancelled under the updated lane rule; wider
validation is left to CI. The retained regressions compiled and passed in the
completed run. No main merge or file copy is needed.
No golden or kernel-case expectation is changed.
