# boolean.api_brep: #2538's quarter-round notch on a tilted edge fails only when the slanted plane misses the notch's axis (2026-11-03)

- **Unit.** `boolean.api_brep` (`boolean_difference`), with `primitive.api_primitives`
  and `sweep.api_brep` building the operands.
- **The case.** tangency-labs/solidcore#2538: the box 198 x 105.5 x 71.5 with its lower
  left corner at (-99, -6.5, -55), clipped by a half space that leaves an edge along the
  line through the issue's sketch origin (-12.07395, -6.5, 0.84551) along the unit of its
  sketch `y_dir` [0.069857, 0, 0.997557] (4.005773878 degrees from z), then cut by the
  cylinder of radius 8 about that same line. That line lies in the box's own y = -6.5
  face, so the slanted face and that face meet along it at a right angle and the cut is
  a quarter-round notch. The design's tool (`v5`) has both cap centres exactly on the
  box's z = 16.5 and z = -55 face planes, and its seam along world +y.
- **What the issue reports.** Through SolidCraft (`solidcraft-cli mcp --headless`,
  `SOLIDCRAFT_KERNEL=solidcore`, pinned at 8ddce9016) the design's cut fails with
  `boolean::merge::p2::make_faceuse: upstream status 1001`; the tool 2 mm longer
  (`v13`) is refused with `SmFace::Test #36` ("Face Surface Seam has suspicious Edge
  near seam"); 0.5 mm shorter (`v14`) adds `Bad Gap` and two `SmEllipse` approximation
  failures at 1.5888501e-08 against 5.550122e-09. The C++ fork refuses all three. The
  mirror image about x = 0 (`r1_pos`) passes on both, at 641498.909.
- **What reproduces here, and what does not.** Both. With the slanted plane built so
  that it holds the notch's axis exactly, `boolean_difference` gives one closed manifold
  solid that passes `topology::integrity::check`, `is_manifold_solid` and `check_brep`
  at Level2 (which runs face test 36), for the design, for the tool 2 mm past both faces
  and for the mirror, with the tool built either as a placed `create_cylinder` or as the
  issue's own full-turn `create_rotational_sweep`, under `UpstreamFixes::default()` and
  `UpstreamFixes::all()` alike. Built the way the issue's repro builds it, by turning a
  2500 cube about [-0.590342071131, 0.590342071131, -0.550447525296] by
  122.33904750051076 degrees and moving it, the same cut leaves a ninth, sliver face and
  a result that is **not** a closed manifold solid, and Level2 reports exactly the
  issue's `SmFace::Test #36`, together with `SmEdge::Test #9` ("An Edge->Edgeuse/
  RadialEdgeuse pair of radial angles do not increase monotonically") and the two
  `SmEllipse` tests at the issue's own 1.5888500731855886e-08.
- **The cause: the seam lands within tolerance of its own edge, but not on it.** The
  tool's axis is the edge being notched, so the cylinder meets the slanted face in the
  two lines a radius to either side of the axis, and the design turns the seam onto
  world +y, which is the line on the material side. With the exact plane the seam **is**
  that edge of the result, and nothing is reported. The repro's plane is tilted
  4.4721e-07 rad from the exact one, and along the seam the gap to it runs from
  -2.3540e-05 mm at the lower cap through +1.4954e-06 mm at the sketch origin to
  +8.5134e-06 mm at the upper cap: the seam crosses its edge instead of lying on it.
  That is inside the band where `CurveClassification::has_near_misses`
  (`intersect/curve_class/p7.rs`) calls an interval a near miss -- gap at least
  `near_miss_dist_3d = EFF_ZERO * 1000` = 1e-09 mm -- while the seam still classifies as
  coincident with the edge at the bodies' 2e-05 tolerance. `assert_valid::face`
  (`topology/brep/validation/assert_valid/face.rs`, test 36 from
  `has_seam_problem_in_brep_mut`'s `near_miss`) then fires.
- **The band, measured.** Perturbing the exact plane on its own, by a shift along its
  normal or a tilt about world y (scratch probe, `runs/2026-11-03-2538-tilted-notch/`
  `logs/40-probe3.log` at `e8397421e`):

  | slant plane | faces | closed solid | Level2 |
  |---|---|---|---|
  | exact | 8 | yes | ok |
  | tilt 3e-11 rad, shift 1e-09 mm | 8 | yes | ok |
  | tilt 1e-10 to -2e-07 rad, shift 3e-09 mm | 8 | yes | face 36 |
  | tilt -3e-07 rad | 8 | yes | face 36, four `Bad Gap` |
  | tilt -4e-07 to -1e-06 rad, the repro's rotation | 9 | **no** | face 36, edge 9 |
  | tilt -1e-05 rad and past it | 9 | yes | ok |

  So the defect is a band, not a threshold: below 1e-09 mm of gap the seam is the edge,
  above about 2e-05 mm it is a separate edge and the kernel builds the sliver face
  properly, and in between -- which is where any construction from rounded numbers lands
  -- the seam is a near miss. The sign matters inside the band: only the direction the
  repro happens to take loses manifoldness.
- **The volume.** The closed form of the family: the clipped box is a prism whose cross
  section in x at height z runs from -99 to -12.07395 + k (z - 0.84551) with
  k = tan(t) = 0.07002807859601005, so V_prism = 645089.6884492004; the notch is the
  quarter disc p <= 0, q >= 0 of radius 8 (p along the slanted face's outward normal,
  q along world +y from the y = -6.5 face, s along the axis) times the length in s the
  box and the tool both keep, which is L + k p with L = 71.5/cos(t) = 71.67510115307044
  for the design and L throughout for the tool 2 mm past the faces:
  V_notch = 16 pi L - (512/3) k = 3590.83208089761 and 16 pi L = 3602.7835396446626. The
  design's result is 641498.8563683027 and the 2 mm one 641486.9049095557; the corpus
  entry `booleans/2538-slant-edge-minus-tilted-notch-caps-on-faces` carries the
  derivation. Measured against those: 8.0e-12 and 8.1e-13 relative for the design
  (primitive and revolve tool, all fixes), 5.9e-11 and 1.7e-09 with default fixes,
  1.1e-12 for the 2 mm one, 6.2e-12 and 1.3e-11 for the mirror.
- **The issue's own figure.** The repro's plane is offset as well as tilted, and the
  offset is affine in (y, z), so it adds exactly 105.5 * 71.5 * 7.532e-06 = 0.056814 mm3
  to the prism; the notch's own change is second order. That gives 641498.913182 mm3 for
  the repro's construction, and the kernel measures 641498.908378 on the (invalid)
  result it returns -- the issue's 641498.909 for the mirror of the same geometry.
- **Not changed here.** Nothing in `crates/`. Which of the two the fix belongs to is a
  maintainer's call: healing could snap an edge that classifies as coincident with a
  seam onto the seam, or `make_faceuse` could put the face's seam on that edge. Both
  touch the old merge path, so the generic blend's plan (#2243) may decide it instead.
- **Evidence.** `crates/solidcore/tests/issue_2538_tilted_notch_on_a_slant_edge.rs`
  (three passing tests for the exact plane, one known-failing test for the repro's
  rounded one) and `runs/2026-11-03-2538-tilted-notch/logs/`.
