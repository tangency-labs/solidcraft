# Kernel-aware common Surface integration (issue #76, PR #133)

`SurfaceKind::Cone` retains the native cone/cylinder distinction and the existing
`surface.cone_p1` implementation. Common reads, copies, NURB/STEP evaluation,
normal/derivative helpers, UV conversion, STEP inversion (including the caller's
subdomain), adjustment and transform dispatch through the concrete object.
The common evaluation traits and callbacks borrow the caller's `Kernel`; implicit
iso curves also carry it through `CurveEvaluate` and the curve solver callers.
Production dispatch does not create a default kernel. The offset API returns
`Result<Option<Surface>>`, preserving the representability of successful null
analytic offsets; no cone offset implementation is imported or certified here.

Remaining integration boundaries are explicit:

- `surface.cone_p2` owns offsets and rebuilding analytic parameters after reverse /
  UV swap. PR #71 remains independent. Its head `c9aa85de` has 26 reported failures;
  older passing results do not certify that head.
- PR #131 has merged. Its concrete intersection numerics are retained, with
  caller Kernel propagation at the newly merged call sites. Common plane/cone,
  boundary and global cone intersection adapters remain `surface.surface_p2`
  dispatch gaps; numerical bodies and dependency integration stay with #131's owner.
- Cone trimming and fast point dropping still need inherited revolution bodies
  with the cone's virtual callbacks. These are dispatch gaps, not replacements by
  a plain revolution or a B-spline. Their errors name the owning common unit.
- Part 9 iso/tessellation helpers still store B-spline objects and cannot retain
  Cone/Cylinder iso/evaluation overrides. These now report an explicit
  `surface.surface_p9` dispatch gap before constructing stand-ins. The existing
  part 9 revolution fixtures do not certify real Cone/Cylinder dispatch; a core
  contract assertion covers refusal for both concrete types.
- Sphere (PR #100), torus and extrusion adapters are not imported.
  PR #120 has merged; its numerical implementation and subdivision bound are
  retained, with caller Kernel propagation through its new shared solver paths. Existing concrete readers may still defer when no SurfaceKind
  adapter exists. Curve-on-surface and cache storage that specifically owns a
  B-spline remains a separate adapter boundary.

Resource handling: common polymorphic surface reads balance the reader's bounded
nesting counter on ordinary errors. Fallible evaluation and copy entrypoints
preflight owned geometry against the existing 64-level limit; derivative orders
are checked by the concrete evaluator before rectangular output allocation.
Inherited `SplitAt` returns newly constructed B-spline results, as explicitly
specified and implemented by `SmBSplineSurface::SplitAt` (`SmBSplineSurface.cpp`,
5016–5076); the input's Cone/Cylinder kind is retained.
Constructor failures remain native statuses. Safe native database coverage writes
records from guarded real Cone/Cylinder constructors before reading them through
`SmSurface::ReadFromDB`; it does not parse arbitrary caller-controlled records in
C++. Existing crash/undefined-input guards are retained, and guarded `SC_ERR_ARG`
recipes are excluded from the newly added common numerical coverage.

Native common tests reuse the cone part 1 constructor/operation corpus and add
ASCII/binary version-30 round trips, lazy `SmSurfaceEval` derivatives, inversion
subdomains, degeneracy, normals and bounding boxes. Common fallible inversion
compares UV/location only on success; direct cone part 1 retains its native
failure-output comparisons. No comparison tolerance or absolute floor is changed.
The seven `topology.brep_data` fixtures previously deferred on cone/cylinder
records now compare: the five Defeature LShape I/II, Platte, defillet and
flathead-screw-top files, plus OffsetBoxCylOnCorner edited/healed. That unit
retains 48 independent offset/extrusion deferrals.
An additional 32 manual lazy-evaluator apex probes pass bit-identically: canonical
cone bot=2/top=0/height=3, identity placement, both swap/inside flags, angular
parameters 0, 0.25, 0.5, 1 and axial parameters 1 and its preceding float
0.9999999999999999 (exchange coordinates when swapped). These probes use the
common lazy adapter and are separate from the committed default case totals.


Validation configuration for #133: Apple Clang uses the shared Release/O2 kernel
and O2 shims. The full regression gate uses the standard Ubuntu CI workflow on
Karl's fork, with Release/O2 kernel, O2 shims, the difftest profile and unchanged
`-ffp-contract=off -fno-fast-math`. A local 4 GB Linux VM could not complete the
full runner build under memory pressure (including an unoptimized driver/shim
attempt); no full-suite pass is claimed for that abandoned local attempt. Its
validation container was removed without changing other containers or the VM's
configuration. Canonical CI requires maintainer approval, which this account
cannot grant; the fork workflow adds report upload, full sensitivity and the native contract
tests. Default and sensitivity checks run in independent processes after one
standard optimized build, with unchanged per-process input/operation order and
comparison policy. Its crate/harness sources match PR #133's source commit
`ce56aae6`.
After merging #120, the local C++ Clippy rerun could not write its query cache
because the shared disk filled; only this worktree's disposable caches were
removed. The hosted rerun passes C++ Clippy on the merged source. The merged
local core tests (1,193) and wasm check pass.

Final recertification: [GNU CI](https://github.com/Karl-VonRichter/solidcore/actions/runs/38870779906) validates code
`ce56aae6`, including merged #120 and the explicit part 9 Cone/Cylinder refusal.
The validation tree changes only the workflow. `--green` compares 452,754 of
465,289 cases across 182 units (99 green, 83 partial), with 12,535 deferred and
zero failed. Sensitivity detects all 452,672 perturbations; 82 empty outputs
remain unperturbed (`geom.assert_array` 1, `nlib.srf_basic` 1,
`geom.local_solve1d` 7, `nlib.basis_adv` 73), and deferrals are not perturbed.
The two primary reports have max relative error zero and no absolute floors;
all units retain their previous tolerance/floor policy. Workspace tests total
1,215 passes (1,193 core); the two native contract tests pass with 27 filtered.
Formatting, plain/C++ Clippy, wasm, deny and locked publish dry-run all pass.

The topology solver's 12 previous `curve.iso_curve` deferrals now reach the
explicit `SmSurface::GetNaturalUVDomain` gap in `surface.surface_p2`; 2,396 cases
still compare and 33 defer. No additional unit is promoted by this integration.
