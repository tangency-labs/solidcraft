# Optional spatial denoising

Native progressive GPU image requests accept `"denoise": true`. Omission and
explicit `false` preserve the existing raw image path and allocate no denoising
buffers or pipelines. This runtime flag is not stored in designs or appearances.
The request is rejected on non-progressive or non-GPU executors.

The optional final pass runs after all requested path samples, on linear
premultiplied radiance, before the existing single exposure/tone-map/sRGB output
step. It never changes the raw linear accumulator or the path random-number
stream. Raw radiance and medium diagnostics are checked first, so denoising
cannot conceal a failed glass path. To retain both raw and filtered PNGs, submit
two otherwise identical jobs with the flag false and true respectively.

The original implementation uses three edge-aware a-trous passes with pixel
steps 1, 2 and 4 and the separable five-tap kernel [1,4,6,4,1]. It is inspired by
the published method of [Dammertz, Sewtz, Hanika and Lensch, HPG 2010](https://holger.dammertz.org/publications/dammertz10hpg_bibtex.html).
No third-party filtering source or weights are copied. A deterministic pixel-centre
ray supplies textured albedo, shading normal, body identity, world position and
depth. Bilateral weights use albedo difference, normal agreement, distance from
the local tangent planes, axial depth difference, and radiance contrast. Spatial thresholds scale with
the projected pixel footprint. Every pass retains the original alpha; the
online convex average avoids overflowing radiance multiplied by kernel weights.

First-hit guides cannot describe an image seen through refraction or a sharp
reflection. Pixels first hitting glass, metals with roughness below 0.25,
near-mirror coats with roughness below 0.05, light emitters, background or
partial coverage bypass the filter. Glossier coats are filtered: on the studio
coffee scenes their broad reflections of the cyclorama and area lights keep
their shape, while the grain from choosing between coat and base layers is
removed. A coat reflecting fine detail can lose some of it. First-hit guides also do not model depth-of-field correspondences:
the centre lens ray is used, so out-of-focus boundaries may retain noise or
acquire spatial bias. Rough glossy effects and soft shadows can lose detail.
This is a biased, optional spatial filter, not a temporal or learned denoiser,
and it cannot guarantee preservation of every lighting or texture feature.

Preflight includes the enabled method, three passes, guide policy, bypass cases
and these limitations. The job options retain the explicit flag. A job admits
80 additional GPU bytes per pixel only when enabled: 48 for guides and 32 for
two colour buffers. The 512 MiB job estimate and each device storage-binding
limit apply. At 1920x1080 the guide buffer is 99,532,800 bytes; larger dimensions
may be rejected before allocation rather than silently reducing resolution.
The guide pass reuses the existing six-storage-buffer path layout; the filter
needs three storage buffers. Cancellation is checked between bounded tiles.

Regression coverage includes an independent f64 gather oracle, constant-image
and coverage identity, checker noise reduction, albedo/normal/depth/body edge
preservation, synthetic GPU/oracle agreement, production centre-ray guides,
default/explicit-false image identity, raw-buffer preservation, admission and
runtime preflight behaviour. GPU checks must be run on the target backends;
source-level and shader validation alone do not establish rendered quality.
