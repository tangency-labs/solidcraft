# Matched GGX visible-normal proposal (#1658)

Recovered VNDF source port, connected to the CPU image consumer. The tests are
written but have not run on this combined CPU/GPU2 revision. No measured variance
reduction or visual acceptance follows from this document. The exact integration
baseline and recovered source identity are recorded in
[advanced CPU integration](render-cpu-advanced-integration.md).

This change does not diagnose the prior bright pixels as a PDF bug: the prior
projected-NDF sampler and its PDF were matched.

## Mathematical contract

Keep the existing opaque specular BSDF: Schlick F, GGX D, perceptual roughness
squared to alpha, and separable Smith G2=G1(v)G1(l). Let c_v=n.v>0 and
c_l=n.l>0, with h=normalize(v+l) and v.h>0. Define S(v)=G1(v)/c_v, evaluated
without dividing by c_v:

    S(v) = 2 / (v_z + sqrt(v_z² + (alpha_x v_x)² + (alpha_y v_y)²))
    p_visible(h|v) = D(h) S(v) max(v.h,0)
    p(l|v) = p_visible(h|v)/(4 v.h) = D(h) S(v)/4
    f_spec(v,l) = F(v.h) D(h) S(v) S(l)/4
    f_spec(v,l) c_l / p(l|v) = F(v.h) G1(l)

Thus no tiny view-cosine or half-vector-cosine division is needed in the weight.
This cancellation requires the same distribution, visibility convention and
Fresnel law in sampling, PDF and BSDF. It does not apply unchanged to a
height-correlated Smith G2, transmission or a diffuse/specular mixture.
For admitted reflectance and valid unit directions, each channel of F*G1(l)
is mathematically in [0,1]; it is not clamped. HDR radiance and remaining
lighting estimators can still cause strong variation.

## Original construction from the spherical-cap equations

In the visual frame let A=diag(alpha_x,alpha_y,1) and w=normalize(A v).
For two uniform variates (u,t) in [0,1), choose phi=2*pi*t and
z=1-u*(1+w_z). Uniform azimuth and uniform z give a uniform solid-angle
direction c on the cap z in [-w_z,1]. Its density is 1/(2*pi*(1+w_z)).
The visible unit-roughness normal is proportional to w+c. The desired normal
is therefore normalize(A(w+c)); intermediate normalization cancels.

The prototype computes the radial component as sqrt((1-z)*(1+z)) and the
vertical sum directly as (1-u)*(1+w_z). The latter avoids subtracting and
then re-adding w_z near the lower cap boundary. No disk-radicand clamp,
weight clamp, rejection loop, retry or conditional density is introduced.
Finite-precision support at arbitrary tilted frames remains a review and
execution requirement, not a universal numerical proof.

## Production route and controls

The proposed isotropic `rough_specular` wrapper and planar anisotropic adapter
call `sample_visible_reflection`; they use the matching visible-reflection PDF.
The shared anisotropic implementation covers equal axes too. The delta `mirror`
function is unchanged. Existing projected-NDF methods `sample_normal`,
`normal_pdf`, `reflection_pdf` and `sample_reflection` retain their contracts
and assertions as explicit legacy numerical controls. The previous isotropic
implementation is retained only as `rough_specular_ndf` under cfg(test).
Their earlier passes do not certify the new production VNDF route.

Below-shading/geometric-hemisphere reflections remain null events. Every null
attempt counts in R and the original final divisor. The receiver's variates,
R in 1..64, primary samples S in 1/4/16, bounce limits, captured selectors,
host pool, Record lease, work/cancel debits, geometry, environment partition
and radiance caps are unchanged. Default S=1 retains its deterministic-primary
reuse policy; S>1 completes all S*R attempts. Existing directional, punctual,
rectangle/LED and HDR continuations retain their reset and ownership policies.
Actual outgoing paths, null counts and traversal work can differ. No equal-work
or all-noise-resolved claim is made. No MIS, HDR importance sampling, filtering,
new primary anti-aliasing policy or new persistence is installed by this delta.

## New written checks and required review

New tests target the actual VNDF APIs and production wrapper: normal-incidence
mean 1-ln(2), second moment 3-4ln(2), half-null mass, unit-alpha grazing
cap density and acceptance, unchanged-BSDF throughput, anisotropic rotation,
finite open endpoints/subnormal grazing, PDF support and density normalization.
The original ten regressions remain, including cancellation and exhausted work
after a rough spawn across R=1/4/16/64. A spawn alone is not a second-body hit.
Five additional tests cover the previously identified source-coverage gaps:

- Rotation about an arbitrary three-component axis checks direction, PDF,
  throughput and geometric rejection away from unresolved boundaries. A
  separate tilted test covers both sides of a resolved 1e-6/1e-9 horizon and
  near-opposition cap endpoints, including u=next_down(1). It checks finite
  outcomes and explicit support; it does not certify signs of subnormal
  elevations lost when added to O(1) world coordinates.
- For unequal axes at normal incidence, acceptance has the independent value
  `1/sqrt((1+alpha_x^2)*(1+alpha_y^2))`. The radial acceptance threshold is
  `u < 1/(1+alpha_x^2*cos(phi)^2+alpha_y^2*sin(phi)^2)`; integrating azimuth
  gives that value. Solid-angle quadrature checks normal-density mass one and
  outgoing-density mass equal to acceptance, with nulls counted separately.
  A grazing unequal-axis case adds an independently tilted geometric rejection
  plane and compares outgoing PDF mass with accepted attempts. Its finite-grid
  0.01 agreement gate is not a universal quadrature-error bound. The sampled
  BSDF check and original moment assertions retain their prior tolerances.
- A real ImageTask fixture puts the camera and selected reflector inside a
  separate inward-facing box. Every accepted secondary must resolve body1;
  no spawn flag substitutes for that identity check. It records distinct
  primary attempts, actual secondary spawns and resolved second-body hits for
  S=1/4/16 and R=1/4/16/64, requires all S*R attempts before pixel publication,
  checks null attempts at unit roughness, and completes/decodes the PNG.

The current planar bridge rebuilds its frame from the oriented geometric normal
and captured visual tangent; that receiver behavior is preserved. Its existing
exact-equality regression now calls the visible-normal reference API, retaining
every equality/rotation/reciprocity assertion. Projected-NDF controls remain
explicit and unchanged. All fifteen VNDF tests are WRITTEN, NOT RUN.

Independent numerical review and actual execution must verify the cap measure/Jacobian, Smith
separability, no accidental NDF production path, geometric rejection, extreme
arithmetic and null accounting. Future separately allocated gates must compile
default/feature modes, run old controls and new VNDF tests, strict Clippy, wasm
and required full CI on the final receiver. Only then compare actual matched
images and measure error/work with appropriate references. The foundation
publication is not held hostage by this unqualified follow-up.

## Primary references and licence scope

- Eric Heitz, *Sampling the GGX Distribution of Visible Normals*, JCGT 7(4),
  2018, revised 2019: [paper](https://jcgt.org/published/0007/04/01/paper.pdf),
  equations 1–3 and 15–19. The paper states CC BY-ND 3.0 on its last page.
- Jonathan Dupuy and Anis Benyoub, *Sampling Visible GGX Normals with Spherical
  Caps*, 2023: [primary paper](https://arxiv.org/html/2306.05044v1), equations
  5–10 and the ellipsoid transformation. Its arXiv version states CC BY 4.0.

Paper equations and displayed listings were consulted. Rust code here is an
original implementation from the equations and the derivation above; no source
listing, figure or paper prose is copied into the repository. No supplemental
implementation repository, GPL-family source or Blender source was consulted.
No dependency is added. Original code remains MIT OR Apache-2.0 under the
project contribution licence; paper licences are not relicensed as project code.


## Landed numerical foundation versus this advanced candidate

The numerical foundation is already in the basic Render delivery through #1658.
The CPU consumer and the additional experimental work above remain part of #5494.
Historical validation notes retain their original source pins; the current combined
source requires its own acceptance. The two delivery scopes are not equivalent.
