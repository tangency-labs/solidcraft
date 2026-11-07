# Optional spatial denoising

CPU ray-traced requests accept `raytrace.denoise: true`; saved CPU jobs store
`job.options.denoise`. GPU path requests keep their existing `denoise` flag and
saved GPU field. Omission and explicit `false` preserve the corresponding raw
image path and allocate no denoising buffers. CPU filtering uses only the native
Rust renderer: no GPU device, WGSL runtime or software-GPU fallback is involved.
Raster cannot consume raytrace options. Offline editing does not imply that an
image host is installed.

Every complete published pass uses the selected filter, including nonfinal CPU
and fixed/adaptive GPU feedback. Fast updates can coalesce at screen repaint;
the producer does not omit an effect merely to obtain an intermediate image.
Filtering uses normalized linear coverage-premultiplied radiance before bloom,
exposure, tone mapping, sRGB encoding and photographic finishing. It never
changes the raw film, sample keys or random stream. Radiance and transport
failures are checked before filtering. Final-only raw FLOAT32 EXR comes from the
unfiltered pass, before denoising; no raw companion is retained per preview.

The original implementation uses three edge-aware a-trous passes with pixel
steps 1, 2 and 4 and the separable five-tap kernel [1,4,6,4,1]. It is inspired by
the published method of [Dammertz, Sewtz, Hanika and Lensch, HPG 2010](https://holger.dammertz.org/publications/dammertz10hpg_bibtex.html).
No third-party filtering source or weights are copied. Deterministic pixel-centre
rays supply effective albedo, shading normal, body identity, position, depth and
projected pixel footprint. CPU guides reuse its actual prepared hit and supported
surface-effect evaluation; positions are made camera-relative in f64 before
conversion to f32. Guides are captured once per immutable CPU job and never
consume an absolute camera-sample ordinal.

Bilateral weights use albedo difference, normal agreement, distance from the
local tangent planes, axial depth difference and radiance contrast. Spatial
thresholds scale with the projected pixel footprint. Every pass preserves the
original coverage. Online convex averaging avoids multiplying HDR radiance by
large kernel weights.

First-hit guides cannot describe an image seen through refraction or sharp
reflection. Pixels first hitting glass, metals with roughness below 0.25,
near-mirror coats with roughness below 0.05, light emitters, background or partial
coverage bypass the filter. CPU's explicit Mirror override bypasses it as the
corresponding sharp reflective route, including on a dielectric base colour.
Glossier coats can be filtered; reflections of fine detail may soften. First-hit
guides also do not reconstruct depth-of-field correspondences: the centre lens
ray is used, so out-of-focus boundaries can retain noise or acquire spatial
bias. Rough glossy effects, texture detail and soft shadows can lose detail.
This is a biased, optional spatial filter, not a temporal or learned denoiser.
It cannot guarantee preservation of every lighting or texture feature.

CPU admission prepays 48 bytes per pixel for guides plus two 16-byte colour
buffers. If shared presentation or EXR already admits one linear buffer, the
additional pixel storage is 64 bytes rather than 80. Bounded guide cursor and
output-task storage is also admitted; there is no retained image per pass.
The existing CPU resource profile and shared host pool still apply. The shared
float4 presentation envelope limits filtering to 8192 pixels per side and
16,777,216 pixels total, even when EXR retention is off. No size is reduced and no
profile is raised automatically. Each filter continuation processes at most 256
pixels with a fixed 25-tap gather per pixel and cancellation checks between
pixels. Work units count these bounded pixel visits, as for existing finishing
kernels: three visits per image pixel per complete published pass. Guide rays
retain the existing incremental BVH node/triangle charges, using the same
cumulative job budget without resetting it between output and transport.

GPU admission keeps 80 additional bytes per pixel for three guide vec4s and two
colour buffers. Adaptive intermediate normalization has its separate admitted
input. Current GPU job/storage-binding limits apply before allocation. GPU
guides reuse production intersections and material evaluation; cancellation is
checked between bounded dispatch tiles. CPU and GPU filters share the numerical
policy, not bit-identical transport or a common device pipeline.

Preflight names the enabled method, passes, guide/bypass policy, work convention
and limitations. Saved settings preserve the explicit flag. Retained-master
regrading has no geometry guides and does not reapply denoising; its metadata
reports `denoise_applied: false`, and the original raw EXR remains unchanged.
Submit otherwise identical unfiltered and filtered jobs to retain both PNGs.

Executed CPU renderer regressions include an independent direct f64 gather
oracle, HDR/constant/coverage identity, noise reduction, body/albedo/normal/depth
edges, production centre-ray guides and bypasses, translated geometry, lens-guide
identity, false-path byte/work/allocation identity, every filtered prefix,
quantum independence, raw film/EXR preservation, cancellation and full scratch
admission. Document/engine/UI tests for offline persistence, explicit CPU wire
routing, native request metadata and one-undo controls still need qualification
on the combined source. Executed renderer checks do not establish native
image-quality, performance or full-project acceptance.
