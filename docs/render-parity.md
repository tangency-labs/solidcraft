# CPU and GPU renderer parity

This table describes the supported domains retained from published commit
`00edaf6a0e6b66acf509d877428163168d5e4fcd`, source tree
`38a7f0aff298cbe50368f367c327da9aeebb1ed7`, plus the Glass follow-up described below.
A control or saved field is
not proof that every producer accepts its geometry or effect combination. Raster
remains schematic; it is not physical Glass, GI or an alternate path tracer.

## Transport and authoring

| Capability | CPU ray tracing | GPU path tracing |
| --- | --- | --- |
| Progressive sampling | Explicit progressive camera samples; fixed jobs retain their authored AA. | Progressive fixed-SPP and independent-pilot adaptive jobs. |
| Adaptive sampling | Independent pilot seed/reset, fixed production allocation per pixel; optional, not a convergence guarantee. | Same bounded pilot/production policy, with its own producer implementation. |
| Opaque PBR and clear coat | Common stable saved opaque coats; direct and explicit Path consume them. Selected reflection/anisotropy overrides cannot share a coat target. | Common saved coats plus the existing raw GPU coat override. |
| Built-in surface maps and aging | Saved built-in base-colour/normal/roughness/metallic maps on opaque targets; ordered aging preserves the supported closure. | Same saved built-in roles and ordered aging, under GPU source/upload limits. |
| External map files | No published common portable importer or CPU engine/UI file-map capture route. Unsupported intent refuses; built-in IDs remain usable offline. | Existing bounded PNG/JPEG temporary-input route. Portable external import is separate unqualified work. |
| Lights and environment | Bounded directional, point/spot and rectangle/LED sources. Finite Sun/Sky and diffuse environment GI require explicit Path; HDR NEE uses uniform solid angle. | Bounded hard/finite directional, point/spot and area sources, Sun/Sky and HDR importance sampling under its own limits. |
| Mesh emission | Explicit body emission is visible and Path continuation can light neighbors; no mesh-emitter next-event sampler. | No authored mesh-emission consumer; analytic area lights are a separate source type. CPU emission intent cannot be silently dropped on a method switch. |
| Glass without CPU Path | Certified one/two-triangle planar sheets and axis-aligned boxes, including valid planar tilings up to 65,536 triangles. Saved closed_mesh intent retains the legacy box projection. | Smooth thin-sheet and captured closed-mesh transport; no CPU legacy route is inferred. |
| General closed Glass | Explicit Path, progressive `primary_samples: 1` and traced shadows. Exact captured-f64 manifold/vertex-link/contact/outward certificate; no welding or geometry repair. | Captured-f32 closed/oriented homogeneous topology certificate and its documented precision/domain limits. It does not prove all self-intersections absent. |
| Nested media and inside eye/pupil | Certified Path nesting with convex enclosing parents, exact origin/near-segment checks and bounded medium stack. Touching/crossing boundaries refuse. | Explicit nested-media mode, certified hierarchy and actual shader-origin query. Default mode retains outside/disjoint restrictions. |
| Transmitted thin shadows | Explicit Path flag; parallel-sheet Fresnel/Beer with sample-count MIS. Legacy route refuses. | Enabled by thin-sheet default in the raw adapter; exact slab response on a straight connection. |
| Transmitted closed shadows | Refused, including with Path. Physical sampled Glass paths still reach emitters/environment. | Explicit opt-in 64-interface straight-through NEE approximation; not focusing, refracted silhouettes or caustics. |
| Interior subsurface scattering | Selected certified closed Path medium; active scattering requires disabled surface Denoise. | Selected closed medium; active scattering requires Denoise off, one Fresnel stratum and no active external volume. |
| External Atmosphere with Glass | Refused. Active Atmosphere is an explicit opaque-Path route and also refuses surface Denoise. | Active bounded Atmosphere also refuses Glass boundaries and active selected-medium scattering. |
| Selected reflection/anisotropy overrides | Existing direct/legacy contracts remain. Selected overrides refuse with Path; no silent clearing or reinterpretation. | No equivalent CPU planar selected-anisotropy adapter. HG phase anisotropy is a different parameter. |
| Guided denoising | Complete displayed passes are filtered; Glass and sharp specular first hits bypass. Raw EXR remains unfiltered. Active medium restrictions above remain. | Same distinction: surface guide filter, not a Glass/DOF ML denoiser; raw accumulation stays intact. |
| Physical lens and output | Explicit lens; straight-alpha primary coverage PNG and compatible finishing; completed raw EXR. Glass is not a general transmissive compositing matte. | Explicit lens, compatible PNG/finishing and completed raw EXR under its own admission. |
| Rough Glass, dispersion and coating interference | Optional isotropic GGX reflection/transmission, bounded Abbe/Cauchy dispersion and one lossless coherent coating; explicit Path. Three representative RGB bands. | Same authored models with f32 arithmetic and its precision refusals; three representative RGB bands. |
| Hard-source caustics | Opt-in bounded photon paths and finite-radius density estimate. | Opt-in bounded photon paths and finite-radius density estimate; lazy seven-storage-buffer pipeline. |

Stable Glass, opaque coats and built-in maps save with the shot and resolve exact
definition/feature/component/occurrence targets. Glass cannot share a target with
opaque coat/maps. Different opaque targets retain their supported effects. Local
edits are prospective until explicit Save/replacement/Remove; painting never
saves. Unsupported combinations retain authored values and refuse.

Both routes retain their existing request/profile/job/host limits, fallible exact
capacity checks, cumulative work or bounded-dispatch cancellation and completed
frame ownership. A maximum in one dimension does not guarantee the other limits
will admit that request. No renderer or document-format cap is raised by this
capability summary. See [CPU profiles](render-cpu-resource-profiles.md),
[general Glass](render-cpu-general-glass.md), [GPU boundaries](render-glass-boundaries.md),
[shadow connections](render-glass-shadows.md) and [clear coat](render-clear-coat.md).
The [detailed Glass matrix](render-glass-complete.md) describes the current
dielectric contract, charged Path sheet tessellations, actual uploaded-GPU
geometry certification and reproducible CPU/GPU examples. The
[material-model guide](render-glass-material-models.md) covers rough transmission,
three-band dispersion, coherent single-layer films and finite-radius hard-source
caustics. These are not dense spectral integration or an unbiased general
specular-chain solver. Their new source and headless evidence are qualified
separately from the historical results below.

<a id="qualification"></a>

## Historical published qualification

Full CI7 on tree 38a completed all 86 native test binaries: 12,223 passed, 34 failed,
58 ignored; exit 1. The renderer library passed 744 tests (1 existing ignore),
GPU library 210 (3 existing ignores), and all 433 unique Render workspace tests
passed inside the full UI library (4,396 passed, 2 failed, 19 ignored). The doctest
side job and the 14-crate WASM check passed separately. The later CI offline-web stage
was not reached after the failed test gate.

All 34 failure-site files match selected upstream c553. That is source equality,
not proof of unchanged callgraphs or cause. Only two Boolean cases have an
independent original-input clean-c553 baseline run. See the public
[CI7 assessment](https://github.com/0xlillith/solidcraft/releases/download/material-finishes-demo-20261015/render-solidcore-main-20261103-v3-ci7-assessment.json)
and [433-test pass list](https://github.com/0xlillith/solidcraft/releases/download/material-finishes-demo-20261015/render-solidcore-main-20261103-v3-render-workspace-433.json).
No green full-CI, complete effect parity, default-quality or convergence claim
follows from the passing scoped tests.

Actual native CPU/GPU Check, Render and same-frame PNG/raw EXR, plus CPU Cancel,
save and fresh reopen evidence used tree 562 and binary 9b. Publication differs by one cfg(test)
JSON literal; production source is identical. The native scene is an opaque
Sample Plate. It does not qualify a native Glass/scattering/Atmosphere visual,
every progressive paint, every backend or every material combination.

## Glass follow-up (this change)

CPU general-Glass Check performs the full geometric certificate. Each strict
prepared submission retains its ready task and moves its own source/proof into
the renderer under its owning lease and cumulative work debit. A separate Render
click prepares fresh captured intent rather than reusing an earlier Check proof.
Advertised boundaries reflect
explicit CPU Path. The editor merges saved target A and new prospective target B
without an incompatible raw-override lane or implicit Save. A checked file HDR
keeps those stable targets through CPU and GPU runtime capture; stable bindings
cannot be mixed with ordinal optical overrides.

These are current source behaviors in this change. See
[PR #11087](https://github.com/tangency-labs/solidcraft/pull/11087) for follow-up
qualification by exact source and executed scope. The historical 00ed/CI7 results
and opaque native fixture above remain separate.

Closed CPU shadow approximation, complete Path selected reflection/anisotropy
parity, external-map import, dense spectral transport, general unbiased caustics
and a production Glass denoiser remain separate work. Unsupported guards cannot
be removed to manufacture those capabilities.
