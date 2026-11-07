# Procedural material patterns (evaluator)

`solidcraft_render::procedural` is the shared reference for procedural surface detail (#2163):
wood grain, textured plastic, brushed metal. It's a pure function from a validated recipe and a
validated point to surface values. Nothing is stored and no image is sampled. Patterns are solid
(3D), so they need no UV unwrap and have no seams.

This is the **evaluator only**. No renderer consumes it yet, and no file format stores recipes.
Consumers (#1656, the starter pack #1722) and the library/design formats that will carry recipes
(#1062, #1086) own those steps.

## Contract

- **Inputs.** A `Recipe` validated with `Recipe::validate` and a `Point` built with `Point::new`:
  - The point is in the body's **definition-local** frame, in millimetres. Each coordinate must be
    finite and within ±1 km (a per-axis limit, not a radius).
  - Every recipe parameter has a documented range. NaN, infinities and out-of-range values are
    refused with the parameter's name.
  - `evaluate` itself can't fail.
- **Outputs** (`Sample`) are independent:
  - linear RGB colour in 0..1, or `None` to keep the material's colour;
  - roughness in 0.02..1, or `None` to keep the material's roughness;
  - a height in mm, and its analytic gradient in the definition-local frame.
- **Shading only.**
  - The consumer applies the definition-local frame once.
  - It projects the height gradient onto the shading tangent plane to tilt the *shading* normal.
  - Geometric normals, silhouettes, shadow-side tests, physical properties and saved geometry
    don't change.
- **Zero amplitude is the plain material.** Every recipe returns `Sample::PLAIN` (`None`, `None`,
  0, `[0; 3]`) at amplitude 0, exactly.
- **Deterministic.**
  - Integer lattice hashing (splitmix64), quintic fades, and IEEE `+ − × ÷ √ floor` only, with no
    platform `sin`, `cos` or `exp`.
  - The same inputs give bit-identical outputs on every platform. A test pins a digest of
    1000 evaluations per family.
- **Bounded cost.** At most 4 octaves per noise sum (`procedural::MAX_OCTAVES`) and a fixed
  number of sums per family: oak uses two (3 and 4 octaves), textured ABS and brushed one each.
  No loop's length depends on the input.
- **Gradients** are exact derivatives of the height except where the height is not
  differentiable: at oak's ring boundaries (a step by design) and within 1e-9 mm of oak's grain
  axis, where the radial term is set to zero. Consumers that cap or admit gradients agree that
  with their owner; this evaluator doesn't inherit an existing consumer's gradient cap.
- **Unfiltered.**
  - This is a point reference, with no footprint or level of detail.
  - A renderer that installs it needs its own footprint/filtering contract for primary and
    secondary rays. It mustn't treat this as level 0 of a filtered texture.
- **Capability.** Recipes need `appearance.procedural/draft`
  (`procedural::PROVISIONAL_CAPABILITY`). The name is provisional: the formats that carry recipes
  own the final name and versioning. A library or design without the capability must refuse such
  an appearance, not keep only a colour.

## Families

| Family | Varies | Not modelled (`Recipe::limitations`) |
|---|---|---|
| **Oak** (side grain) | Colour (earlywood/latewood rings with figure, darker pores), roughness (raised in pores), height (pores) | End grain: the grain runs along local +X, and faces across X show rings without end-grain pores |
| **Textured ABS** | Roughness (valleys to peaks) and height, at a stipple feature size (`grade_mm`) | No colour change |
| **Brushed** | Colour (around a tint), roughness and shallow grooves, streaked along local +X | Anisotropic reflection: this is an isotropic stand-in, so streaked colour and roughness must not be presented as anisotropic rendering |

## Testing

`crates/render/src/procedural_tests.rs` has seven tests, covering:
- zero-amplitude identity;
- refusal of hostile parameters and points;
- output ranges and per-family outputs;
- analytic height gradients against central differences, away from oak's ring boundaries (a step
  by design); the test points stay clear of the grain axis;
- the determinism digest;
- seeds and streak direction;
- ring periodicity.

A future WGSL port must be tested against this reference at the same evaluation points, within a
stated tolerance, before any GPU consumption is claimed.
