# boolean.api_brep: #2537's tilted cylinder cap on a box face does not reproduce from solidcore's own primitives (2026-11-03)

- **Unit.** `boolean.api_brep` (`boolean_difference`), with `primitive.api_primitives`
  and `sweep.api_brep` building the operands.
- **The case.** tangency-labs/solidcore#2537: the box 198 x 105.5 x 71.5 with its lower
  left corner at (-99, -6.5, -55), minus a cylinder of radius 8 tilted 4.005773878
  degrees from z (the unit of the issue's sketch `y_dir` [0.069857, 0, 0.997557]), seam
  along world +y, whose top end cap is centred on the box's top face plane z = 16.5 so
  that face bisects the cap and the seam's end lies on cylinder, cap plane and box face
  at once. The axis runs in the plane y = -6.5, which is the box's own y_min face, so
  exactly half the tool is inside the box; the bottom cap centre sits 1.995 mm below
  z = -55, its whole disc 1.436 mm below it, so the box's bottom face does the cutting
  there.
- **What the issue reports.** Through SolidCraft (`solidcraft-cli mcp --headless`,
  `SOLIDCRAFT_KERNEL=solidcore`, solidcore pinned at 2dbd88b and still at 8ddce90) the
  result check refuses the cut with `SmFace::Test #36` ("Face Surface Seam has
  suspicious Edge near seam") twice; the C++ fork refuses it with eight `Bad Gap`
  failures (9.0232843e-06 and 1e-05 against a 7.5e-06 tolerance) and the same two #36.
  1 um of clearance, the mirror image, and the same degeneracy at the bottom face all
  pass.
- **What reproduces here.** Nothing of the refusal. Built from solidcore's own
  primitives (`create_box`, and the tool either as `create_cylinder` placed by
  `brep::p5::transform` or the issue's own path, a full-turn `create_rotational_sweep`
  of the 8 x 73.675097 rectangle about the sketch axis), `boolean_difference` returns
  one closed manifold solid of 10 faces that passes `topology::integrity::check`,
  `is_manifold_solid` and `check_brep` at Level0, Level1 and Level2, in all four
  combinations of those two tools with `UpstreamFixes::default()` and
  `UpstreamFixes::all()` (SolidCraft runs all but its held-back ones). The volumes are
  1486369.884197 and 1486369.884304 (primitive, no fixes and all fixes) and
  1486369.887650 and 1486369.884296 (revolve, the same two) against the closed form's
  1486369.8843794577: 1.2e-10, 5.1e-11, 2.2e-9 and 5.6e-11 relative. The volume is
  right, not merely plausible.
- **Why the difference is not the geometry.** The closed form for the family (cap centre
  `offset` mm up the axis from the face) is
  `198*105.5*71.5 - 32 pi L + ungula(offset)` with `L = 71.5/cos(t)` and the ungula of
  the tilted cap, derived in the corpus entry's `expected.method`. It reproduces all
  three variants the issue measured on both kernels, each a different pair of cutting
  caps: both caps clear gives 1486357.9329 (the issue's v10 measured 1486357.933); the
  bottom cap on z = -55 gives 1486369.8844 (v16 measured 1486369.885, the C++ fork
  1486369.884); both caps 1 um outside, where each still dips past its face, gives
  1486381.7354 (v12 measured 1486381.736). So SolidCraft and this reconstruction are
  building the same solid; what differs is how the operand bodies and the result check
  get there.
- **The refused check is ported and does not fire.** `SmFace::Test #36` is the
  "Face Near missing Seam" label, index 36 of `geom::assert_array::labels::FACE`, and
  `validation::assert_valid::face` raises it at `CheckLevel::Level2` from
  `face::p4::has_seam_problem_in_brep_mut`'s `near_miss`
  (`assert_valid/face.rs`, "36: FaceSeams must not have near miss"). So the check that
  refuses the cut in SolidCraft runs here too, on the same result, and passes. What
  differs is therefore the result it is run on, not the check.
- **What is therefore still open**, and was not measured here: the operand bodies as
  SolidCraft hands them over, and SolidCraft's own composition around the kernel call
  (its revolve, which asserts its result check runs and at which level, its `fixes()`
  preset minus `HELD_BACK`, and the pick/retry path). Narrowing it further wants the
  case driven through SolidCraft, or SolidCraft's traced operand records for it;
  neither is available here. The entry named below holds the case as data so that a
  re-export, or the corpus runner when it lands with `solidcore-ops`, can be compared
  against it directly.
- **Pinned.** The case is in the kernel-rig corpus as
  `crates/solidcore/tests/kernel_cases/booleans/2537-box-minus-tilted-cylinder-cap-on-top-face`
  with both operands as hand-written records and the closed-form volume, and in
  `crates/solidcore/tests/issue_2537_tilted_cylinder_cap_on_face.rs`, which grades three
  cap offsets against that closed form.
- **PR.** tangency-labs/solidcore#2616.
