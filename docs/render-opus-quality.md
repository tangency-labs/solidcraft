# OPUS image quality and qualification

This change composes the rendering workspace in #11087 and the optical material
models in #12200. It removes resource and shading-data barriers to the complete
OPUS IV still. Higher budgets do not establish convergence or guarantee an image
quality level.

## Explicit quality controls

| Control | CPU Path | GPU Path | Compatibility |
|---|---|---|---|
| Optical targets | 128 | 128 | Legacy CPU non-Path remains eight |
| Nested closed media | 16 | 16 | Count is independent of target and path-depth limits |
| Path depth | 1–64 | 1–64 | Existing authored/default depth remains unchanged |
| Camera samples | Workstation up to 65,536 | Up to 65,536 | Pilot plus production obey the profile's complete sample bound |
| Photon paths | Up to 1,048,576 | Up to 1,048,576 | Explicit authoring; memory and work remain bounded |
| Image profile | Preview/Image and Final/Image | Preview/Image and Final/Image | Same physical producer and exact authored options; raster remains Preview |
| Texture filtering | Explicit `texture_filtering: true` | Explicit `texture_filtering: true` | Absent/false keeps CPU isotropic mips and the existing GPU portable-mip or legacy LOD0 policy |
| Rough metal energy | Explicit `metal_energy_compensation: true` | Explicit `metal_energy_compensation: true` | Default false. CPU needs Indirect lighting. |
| Shading normals | Saved per triangle corner | Saved per triangle corner | Legacy meshes remain flat until explicitly generated/imported |
| Raw master | FLOAT32 linear EXR | FLOAT32 linear EXR | Exposure, display effects and denoise never alter transport master |

The public saved flags are `job.options.texture_filtering` for either image
method. Temporary CPU requests use `raytrace.texture_filtering`; temporary GPU
requests use top-level `texture_filtering`. Native sample editors expose the
expanded bounds without changing existing saved values.

Primary quality filtering uses the renderer's ray differentials. Secondary
filtering projects a circular ray cone onto the geometric receiver plane using
the camera pixel angle and accumulated path length. Mip selection uses the minor
footprint axis and at most eight anisotropic taps. The secondary estimate
approximates refracted footprints and has no measured temporal error bound.

## Rough metal reflectance

Enable **Recover rough metal reflectance** in the Render workspace or set
`job.options.metal_energy_compensation: true` on either path tracer. Temporary
CPU requests use `raytrace.metal_energy_compensation`. Temporary GPU requests
use top-level `metal_energy_compensation`.

Single-scattering GGX misses light that bounces between microfacets. The
optional reciprocal lobe redistributes part of that loss with Kulla and
Conty's corrected Fresnel factor. The base color tints each additional bounce.
The isotropic correction fades in between perceptual roughness 0.35 and 0.45.
Metallic mixtures scale it by the metal fraction. A clear coat attenuates
the compensated substrate. The existing cosine and visible-normal proposal covers both
lobes with the same PDF used for multiple importance sampling.
[Imageworks SIGGRAPH course, corrected appendix](https://blog.selfshadow.com/publications/s2017-shading-course/imageworks/s2017_pbs_imageworks_slides_v2.pdf)

The directional loss fit is specific to SolidCraft's separable Smith GGX.
Training integrates the unit-Fresnel BRDF over azimuth analytically and uses
128-point Gauss–Legendre integration on each side of the mirror peak in
incident cosine. Least squares uses 73 Chebyshev-spaced roughness values
and 110 squared-spaced cosine values. The fit uses degree five in perceptual
roughness and degree seven in square root of cosine. The cosine-weighted
moments normalize the same fitted curve. Its maximum sampled overestimate on a separate grid was
0.00219. Subtracting 0.003 makes the fit conservative across that grid.
Independent hemisphere quadrature and CPU and GPU white-furnace renders
check the complete BRDF.

## Resources and numerical contracts

Native 64-bit capture admits up to 512 MiB of unique normalized base-level
texture pixels. Wasm and 32-bit capture retain the 96 MiB envelope. Encoded
sources remain bounded at 64 MiB and bindings at 128. Role/hash identity and
capture-once deduplication remain authoritative.

The OPUS inventory of 74 distinct 512×512 maps requires 310,378,496 float4 base
bytes and 413,837,600 bytes with complete mip chains. With filtering enabled, GPU maps retain packed RGBA8 at LOD0 and use linear RGBA16_UNORM mip tails:
the complete chain requires 129,324,176 bytes before headers. This fits
a 128 MiB storage binding. Actual device buffer limits are checked independently.
LOD0 retains the original decoded eight-bit channels. Color texels decode sRGB
before filtering; data channels stay linear. CPU mip chains retain float4.
GPU mip levels quantize after linear-light reduction: per-channel quantization
error is at most 1/131070 (approximately 0.00000763) in linear color/data.
No recursive integer mip quantization is used. See
[texture resources](render-runtime-textures.md) for peak capture, decoder,
working/upload, residency and retained-result accounting.

Optical certification uses the source f64 CPU representation or exact materialized
camera-relative uploaded-f32 GPU coordinates. A cancellable Morton hierarchy
finds conservative triangle-contact candidates. Adaptive exact predicates decide
contact; geometry is never snapped or repaired by the hierarchy. Surface winding
classifies true containment independently of input order and convexity. Disjoint
nonconvex interlocking surfaces remain in air even when their boxes overlap.
Touching, crossing, coincident and self-intersecting optical boundaries refuse
with target identities. Thin sheets whose boxes overlap closed media still refuse
conservatively; the general disjointness extension targets closed solids. True nested GPU media still require `nested_media: true`
and disabled straight shadow transmission. CPU Path retains its physical-shadow
compatibility guards. Contact scratch and work count against admission.

[Mesh shading normals](render-mesh-normals.md) describes OBJ corner normals,
smoothing groups, explicit crease generation, persistence and inverse-transpose
transforms. Shading metadata does not change triangles or their optical proof.

## Reproduction and evidence

The engine example `opus_quality_fixture` prepares an explicitly named copy of an
OPUS document. The baseline stays unchanged. The copy retains the original
assembly, all surface bindings, optical objects, coats and aging. Curated curved
mesh sources receive an explicit crease policy; deliberately faceted sources
stay unchanged. Named CPU/GPU reference setups disable the saved straight-shadow
approximation. The original Master setup remains available.

Use the resulting named setup with the native CLI:

```sh
solidcraft-cli render OPUS-quality.sc --setup "OPUS Physical CPU 4K" --out cpu.png --timeout 3600
solidcraft-cli render OPUS-quality.sc --setup "OPUS Physical GPU 4K" --out gpu.png --timeout 3600
```

The exact generated setup names and commands are recorded by the fixture example.
Use small pilot setups first, then establish depth/sample convergence on fixed
material and glass regions before selecting production budgets. A completed
low-sample frame establishes a successful composed job, not a converged beauty.
Retain the same completed job's raw EXR with `render.export` and `format: "exr"`.

## Acceptance boundaries

Implemented contracts and targeted correctness tests are separate from scene
qualification. The PR evidence records the actual source tree, compiler, device,
requests, counts, resource estimates, outcomes and completed artifacts. Native
4K completion, seed/depth convergence, measured peak host/device memory,
responsiveness and temporal stability must be reported from actual execution.
An admission estimate is not measured GPU residency.

Inherited optical models remain three representative RGB wavelength bands, one
lossless thin film and a finite-radius biased photon estimate. Full spectral
reconstruction, general mesh-emitter sampling/MIS, glass plus external smoke,
heterogeneous media and motion blur remain outside the implemented domain.
These exclusions prevent calling the full advanced benchmark complete.

## Optical denoising qualification

Matched 3840 × 2160 OPUS runs use the frozen fixture with SHA-256
`a0a36102d6fc77278987ede9193e84013aeaf0d0950e5eb802984a8274020789`.
The first optical-guide comparison preserves all raw RGBA EXR values exactly
on both CPU and GPU. Display changes affect 46,102 CPU pixels and 94,723 GPU
pixels. These one-sample runs establish transport preservation, not converged
photorealism or universal denoising quality.

Review of that implementation found that mixed glass retained only the identity
of the reflected endpoint. The current filter retains both endpoint surfaces,
including their material, normal and depth edges. Coated mirrors conservatively
bypass optical filtering when the coating cannot share the base reflection guide.
See [denoising](render-denoise.md) for resource costs and supported paths.

The dual-endpoint GPU comparison at one sample changes 36,752 display pixels.
Raw R/G/A values remain exact; the largest B difference is 3.73e-9. A matched
four-sample GPU comparison against the earlier environment-caustics build changes
317,668 display pixels, with a largest raw channel difference of 3.73e-9 and exact
coverage. That earlier build predates the robust and optical filters, so the
four-sample display difference measures their cumulative effect, not only the
dual-endpoint fix. All raw values are finite in both comparisons.

The four-sample raw frame still has 221 pixels above luminance 100 and 12 above
1,000, with peak 3,168.93. The largest one-sample pixel falls from 11,820.48 to
2,955.75 at four samples. Other coherent highlights remain. This is sampling
evidence, not proof that the remaining bright pixels are all erroneous or that
additional display clamping would preserve the image.

Native 4K photon probes isolate the largest area-light outlier to the smooth
opaque photon gather: 4,919.75 of 4,920.72 luminance at the sampled pixel. A fourfold
photon budget reduces that pixel to 1,230.53. An independent photon seed removes
that spike but produces another elsewhere. Camera sampling alone cannot prove
photon-density convergence. The environment comparison at four camera samples
also retains substantial outliers. Area and environment photon estimators remain
opt-in, finite-radius approximations; their low-sample images are diagnostics.

Production qualification requires independent camera and photon seeds, higher
sample counts, photon-density/radius sweeps, and reference comparisons on optical
regions. Display smoothing and unchanged raw masters are not substitutes for
those transport checks. No claim of perfect behavior on arbitrary scenes follows
from these tests.
