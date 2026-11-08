# primitive.api_primitives: review fixes (PR #160, 2026-10-19; no disagreements)

- **Torus surface guard was too wide (AGENTS.md section 4 step 5 item 6, "no more").** The FFI guard refused every
  non-finite origin and every `+inf` radius, but `SmTorus::CreateCanonical` (`SmTorus.cpp:190`) returns
  `SM_ERR` for any radius not above `SM_EFF_ZERO` (NaN included) before it reads the origin or builds
  the NURB. The crash fences (UB-198) now apply only when both radii are above `SM_EFF_ZERO`. The
  reviewer's cases (origin `(NaN,0,0)` with radii `(3,0)` / `(0,1)`, origin `(0,inf,0)` with `(3,-1)`,
  radii `(inf,0)`, `(-2,inf)`, `(NaN,inf)`, `(0,DBL_MAX)`, origin `(-inf,0,0)` with `(1e-12,5)`) all
  return `SM_ERR` in the C++ and match Rust bit for bit.
- **New fixture** `fixtures/primitive.api_primitives/review_adversarial.json` (216 cases: zero, `-0`,
  negative, sub-normal, `SM_EFF_ZERO` and just above, `1e18`, `1e300`, `DBL_MAX` and non-finite
  sizes and origins for the plane/cylinder/cone/torus surfaces and the pyramid): 191 pass, 25 defer
  (pyramid at `topology.brep_p8`). Generated case `gen0732` (a `SmApiCreateConeEllipticEnds` input)
  now passes on its validation status instead of deferring at `primitive.primitive_creation_p1`,
  from main merged into the branch before this fix.
- **Pyramid coverage is the base-edge status only.** Line geometry of the pyramid base edges is
  never compared: every successful base defers at `topology.brep_p8`, so only the failing
  `Line::create_line_segment` statuses are compared.
- `SmApiCreateBlendPrimitive` is still not difftested: it needs a B-rep with faces, which needs the
  `SmBrep` construction units (`topology.brep_p3`/`p5`/`p8`) and `primitive.primitive_creation_*`.
