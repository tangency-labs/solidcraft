# nlib.srf_offset: extrusion recognition and sliver-span conditioning (issue #471)

Reproduced the SolidCraft shell/3576 geometry using the pinned C++ reference:
`SmApiCreateTaperExtrude` of the centered 120x80 rectangle, height 40, draft 5 degrees,
then r4 on all four sloping edges (`SmFilletExecutive`, circular/constant radius,
default tolerances), and `SmApiShellBrepFull` with distance -2 and the top removed.
The standalone `fixtures/nlib.srf_offset/issue471-repro.cpp` uses the linker's
`--wrap=N_SrfOffset` to save the real inputs without changing reference code.
`issue471.json` preserves the four rounds, including the failing (-x,+y) round.

Both Rust and C++ return success from the offset of that round, despite control
points reaching approximately 4125.906 mm from the origin (the part is 120 mm wide).
The C++ shell returns 1001 here (the issue reported a later 1006 failure). With
`eps=1e-4`, `tol=1e-5`, `d=-2`, the surface is classified as an extrusion, but
`|AA x NN|=2.1064044121150683e-6` rejects the shortcut; dividing AA by its length
makes this 5.490920700622231e-8, below `MODEL_TOL=1e-7`.

The free-form path also needs conditioning independently of recognition. It shares
one sample count between u and v, irrespective of the patch aspect ratio. On this
captured input the 2.115e-4 sliver is below the existing 5e-5 relative-width cutoff,
so it initially receives one step, not the roughly 120 described in the issue.
However, the later total-grid cap raises it back to degree 3. Nearby v extension
spans cause the same clustering. Thus normalizing the extrusion test alone does
not address general surfaces or the other rounds whose angular discrepancy is
above `MODEL_TOL` even after normalization.

Documented opt-in correction (defaults remain the pinned C++ algorithm):

- `OffsetOptions::normalize_extrusion_direction` compares unit directions, and
  restores the inherited parameter domain on the extrusion result. The original
  extrusion constructor resets the linear parameter to [0,1], which is unsuitable
  for evaluating against inherited trim parameters on the captured [19.97,58.34]
  domain.
- `OffsetOptions::condition_sampling` computes independent directional sample
  counts. Its minimum density scales by each span's fraction of the full domain;
  directional second-derivative estimates retain sampling where a short parameter
  interval really contains curved geometry. Mixed terms are apportioned by the
  u/v span ratio. Derivative bounds use `abs(d)` for inward offsets. Grid reduction
  keeps a one-step sliver at one step and errors if spans/degrees cannot fit the
  10000-point budget. Inherited fits are checked at the grid points, cell-edge
  midpoints and centers, refining failing intervals for at most 24 iterations.
  They reserve a quarter of the tolerance for knot removal and use half the
  tolerance as the refinement threshold; exhausted budgets return an error.
  Computed parametrizations reserve half the tolerance for knot removal. No
  differential-test tolerance is changed.
- `Surface::offset_with` and `BSplineSurface::approximate_offset_surface_with`
  expose both flags. The original entry points pass `OffsetOptions::default()`.

Tests exercise synthetic slivers, both parameter directions and offset signs,
length-independent extrusion recognition and rejection of oblique axes. The
captured-round tests evaluate interior points of every original span, including
the tiny extensions and sliver, against `S(u,v) + d*N(u,v)`, require error at most
1e-5 mm and bounded control nets, and exercise the B-spline wrapper. All four actual
inputs remain in the ordinary Rust/C++ parity runner with the original defaults.

Limit: the end-to-end shell cannot yet run in Rust: `replace_implicit_offsets`
still defers on `SmBrep::ReplaceSurface` (`topology.brep_p2`). Its future continuation
can select these options when calling the B-spline approximation. This change
corrects the offset approximation; it does not claim that missing shell topology
has been implemented. Upstream defect: WR-257.

On a 21x21-per-span check of the captured inward offsets, the corrected general
path has worst errors 2.48e-6, 2.50e-6, 2.48e-6 and 3.21e-6 mm; with extrusion
recognition enabled the largest is 8.02e-6 mm. The defective default reaches
33.55 mm error inside the last round's spans. Re-running the standalone reproducer
preserves all four control nets and u knots exactly, and reproduces the last
round's complete input and 4125.906 mm net; the other rounds' tiny lower v extension
bounds can differ in their last bits (at most 7e-20 in the checked run). The saved
JSON inputs fix every bit for differential testing.
