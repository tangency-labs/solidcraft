# surface.cone_p2 review fixes (PR #153, 2026-10-16)

- CI clippy (`--features difftest/cpp`) failed on three runner lints; fixed without
  changing generated cases (`trim_defined` returns `bool`; the bounded-mode pick is a
  `match` on the same single `rng.below(3)` draw; the test helper is an associated fn).
- `BSplineSurface::copy_and_add_analytics`: a recognised cone is a DISPATCH GAP (no
  `SurfaceKind::Cone` until issue #76 / PR #133), not an `UNPORTED(surface.surface_p2)`
  stub, since surface.surface_p2 is partial.
- Review adversarial subset added as `fixtures/surface.cone_p2/adversarial.json`
  (308 cases: all swap/inside combinations for partial-sweep, apex, cylinder, 89/-60
  degree semi-angle and unbounded cones in skewed frames; offsets at the truncation
  boundaries with tolerance 0/negative; recognition with scale 0, -1, 1e6; twist,
  tilt and shear edits at tolerance limits, negative/zero weights, 1e7 translations,
  1e-6/1e6 scales; trims with zero/negative radii, non-orthogonal axes, start > end,
  ellipses through the boundary circles at 1e-9 height fractions). All 308 are
  bit-identical with the C++ (each goes through the shim's guards for inputs that
  are undefined in C++). The reviewer's full 18724-case set (from its `gen_adv.py`)
  is not committed.
