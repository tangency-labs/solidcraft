# Anisotropic GGX numerical core

`render::anisotropic_surface` supplies the independent optical core tracked in
#1694/#1699. This constituent extracts the original source and analytic tests
already published in #1698 at `463a3e0daaa6b84f169ba5f8f215fba4642c194a`.
It adds no renderer registration, saved field, command, device dependency or
engineering-material assignment. Runtime consumers remain separate work.

## Contract

`VisualFrame::new` admits finite, nonzero, perpendicular normal/tangent inputs,
normalizes them, and constructs a right-handed visual frame. This is not a physical
material-axis assignment. An original geometric normal is supplied separately;
response and reflected samples reject either hemisphere when appropriate.

Both perceptual roughness axes use the opaque surface range `[0.045, 1]`; alpha is
roughness squared. Linear-sRGB reflectance, metal fraction and dielectric IOR use
the existing `OpaqueSurface` admission. Invalid data is refused, not clamped.
Equal roughness axes preserve the isotropic opaque BRDF contract exactly.

Sampling uses the projected **normal distribution (NDF)**, not a visible-normal
sampler. `normal_pdf` is `D(h) * n.h`; the unconditional reflected-direction PDF
includes the `1 / (4 * v.h)` Jacobian. A rejected microfacet is a null event that
must remain in the sample denominator. Never retry until a sample succeeds.
Reflection weights contain the specular component only. Diffuse/environment
mixtures, visibility, ray spawning, image budgets and multiple scattering belong
to the consumer; this module does not claim to implement them.

The implementation is original Rust using elementary GGX/Smith/Schlick equations.
The public mathematical reference is Heitz, JCGT 7(4), 2018, equations 1 and 2:
https://jcgt.org/published/0007/04/01/. No external implementation was adapted.

## Reproduce the focused checks

```sh
cargo test --profile ci -p solidcraft-render --lib anisotropic_surface
cargo clippy --profile ci -p solidcraft-render --all-targets -- -D warnings
cargo check --target wasm32-unknown-unknown -p solidcraft-render
```

Analytic and deterministic quadrature tests cover hostile inputs, orthonormality,
normal-incidence peaks, isotropic equivalence, reciprocity, 90-degree visual-axis
rotation, geometric/shading hemisphere separation, grazing directions, inverse-CDF
sampling, PDF/throughput agreement and retained null-event mass. Broad-lobe energy
checks are scoped consistency tests, not universal energy certification, measured
material data or evidence of an integrated rendered image.
