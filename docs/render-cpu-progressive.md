# CPU progressive camera sampling

The limits below describe the compatibility **Compact** profile. Explicit Studio and Workstation envelopes are documented in [CPU resource profiles](render-cpu-resource-profiles.md); the published suites and remaining qualification limits are recorded in [renderer parity](render-parity.md).

CPU progressive rendering repeatedly traces fresh camera samples into a bounded
linear-light film. Every complete nonfinal image pass produces a checkpoint:
1, 2, 3, 4, and so on. The exact requested count produces the final PNG,
including non-powers of two such as 3 or 5. A one-sample job has no provisional
checkpoint. The UI can coalesce fast updates between physical screen repaints;
the renderer does not skip complete-pass work to follow a powers-of-two cadence.

This mode uses the native `cpu-raytrace` executor. With `raytrace.path` absent it
keeps the existing direct-light, selected-reflection and certified sheet/box
optical transport. Explicit [opaque path lighting](render-cpu-path.md) adds bounded
diffuse/specular continuation. Guided noise reduction is a separate saved option
applied to completed passes. Independent-pilot adaptive sampling, selected-medium
subsurface transport and general Glass are available under their explicit Path
and compatibility contracts. Optional [Glass models and photon caustics](render-glass-material-models.md)
extend that explicit Path route. Certified convergence remains unsupported; an
adaptive sample allocation or finite-radius photon estimate is not a convergence guarantee.

## Request and saved intent

The CPU option is nested inside `raytrace`. The top-level `progressive: true`
request continues to select the registered GPU path executor.

```json
{
  "integrator": "raytrace",
  "settings": { "width": 512, "height": 512 },
  "raytrace": {
    "progressive": { "samples_per_pixel": 32, "seed": 81 },
    "primary_samples": 1,
    "exposure_ev": 0,
    "shadows": true
  }
}
```

Both `render.preflight` and `render.start` use this request. Missing or null
`raytrace.progressive` preserves the existing fixed 1/4/16 spatial AA route.
An explicit empty progressive object defaults to 16 samples and seed 0. The
seed is an unsigned 32-bit integer. Unknown fields are rejected.

The corresponding portable setup is:

```json
{
  "name": "CPU film",
  "set": {
    "width": 512,
    "height": 512,
    "exposure_ev": 1,
    "job": {
      "method": "cpu_raytrace",
      "options": {
        "progressive": { "samples_per_pixel": 32, "seed": 81 },
        "primary_samples": 1
      }
    }
  }
}
```

This is a `render.settings.set` request. Exposure is stored once in the setup;
normalization transfers it into the CPU request. Film storage, checkpoint PNGs,
job identities, cancellation state and host reservations are never serialized
into the design. Saving validates pixel and camera-sample bounds even when no
render host is installed.

## Supported combinations and limits

| Control | CPU progressive contract |
| --- | --- |
| Dimensions | Each side 1..4096; at most 1,048,576 pixels |
| Samples per pixel | 1..64 |
| Total camera samples | At most 16,777,216 (`width × height × samples_per_pixel`) |
| Fixed spatial AA | `primary_samples` must be 1 |
| Physical depth of field | Optional `lens` uses the shared aperture radius / axial focus distance; positive aperture needs a bounded perspective camera and conservative pupil/glass admission |
| Selected mirror / isotropic rough reflection | Existing bounded depth; `reflections.samples` must be 1 when bodies are selected |
| Directional / point / spot sources | Existing CPU light contracts and limits |
| Constant / captured HDR environment | Existing CPU contracts; HDR plus rough reflections still requires certified fully metallic selected bodies and effective faces |
| Surface emission / isotropic surface effects | Existing CPU appearance contracts; emission is visible radiance |
| Area / LED sources | Existing rectangular emitter transport; at most 4 sources sharing 1/4/16/64 area samples per camera sample |
| Dielectric / anisotropic face overrides | Without Path: certified sheets/axis-aligned boxes and the separate planar anisotropic adapter. With Path: general Glass under its certificate; selected anisotropy/reflection overrides refuse. Geometry, coat, HDR and overlap guards remain. |
| Output | Opaque or straight-alpha coverage PNG; optional compatible shared tone/colour finishing and final-only native linear EXR via `output`; bloom/backdrop/photographic effects require opaque output |

An incompatible request fails; it does not remove settings or switch methods.
Progressive sampling alone retains the legacy sheet/box and planar-anisotropy
transport. Explicit Path selects [general Glass](render-cpu-general-glass.md);
these are different transport choices, not an implicit widening of fixed jobs.
At 1024 × 1024 the camera-sample cap allows at most 16 samples per pixel; at
512 × 512 it allows 64. Requests are also subject to the existing geometry,
384 MiB CPU job and 512 MiB shared host-pool admission limits. Traversal,
shading, film updates, resolve, PNG encoding and hashing share one cumulative
1,000,000,000-unit work ceiling. The count is not reset at a pass boundary.
The camera-sample limit is a cold upper bound, not a promise that every scene
will finish below the cumulative runtime work ceiling.

## Sampling, linear film and display

Camera jitter is a deterministic counter sequence keyed by the u32 seed, pixel
index and absolute sample ordinal. Reflection and HDR-direct variates use
separate domains of the same counter. Rectangular and continuous LED emitters use
a separate film-aware area-sample identity; their admitted area integral completes
before that camera sample enters the film. Target sample count, worker quantum and
checkpoint cadence are not keys. Consequently, for an unchanged captured
request and scene, a checkpoint at N samples has exactly the same sample prefix
and PNG bytes as a job whose final target is N.

Each pixel stores three f64 sums and a primary coverage count. Only complete,
uniform passes can be resolved. Camera misses add zero scene radiance and zero
coverage. The resolve computes the mean scene radiance, applies exposure and
the existing tone curve once to covered radiance, composites the display-linear
backplate by coverage, and encodes sRGB once. The background is not illuminated
or exposed. Averaging already tone-mapped sample pixels is not equivalent and
is not used.

`film::primary::PrimaryFilm` is a compact camera-specific accumulator alongside
the existing statistical `FilmAccumulator`. It has incremental initialization
and requires the exact next scanline pixel and absolute ordinal on every update.
The existing general `FilmAccumulator`/borrowed `FilmPngTask` APIs are retained;
the owned image job uses the shared `PngEncoding` pipeline without a
self-referential borrowed task or a second independent pool.

## Physical thin lens

Optional `raytrace.lens` and saved CPU `options.lens` use the shared physical
[`ThinLens`](render-thin-lens.md) contract. Positive aperture requires this
progressive mode; an existing fixed-AA shot needs explicit adoption in Sampling.
The independent pupil domain uses seed, pixel and absolute film ordinal, so
changing target samples or worker quantum does not alter a prefix. Each camera
sample still traces one primary ray. Pupil generation adds one charged scalar
continuation and no image buffer; the existing memory and cumulative work caps
remain in force. Omitting the lens or using radius zero preserves the original
fixed/progressive images and work count byte for byte.

## Presentation and raw masters

Optional [`output`](render-cpu-output.md) settings reuse the same tone, finishing,
bloom and photographic stages as GPU output. They receive scene-linear,
coverage-premultiplied mean radiance, never decoded PNG pixels. Every checkpoint
uses the same presentation as the final PNG. Only the exact final pass may retain
an EXR; provisional images remain nonexportable. Identity presentation keeps the
original f64 display calculation byte for byte, including when raw retention is
selected. All new stages share the same cumulative CPU work budget and admission.

## Preview publication and final export

`JobStatus.preview`, when present, contains:

```text
PreviewInfo {
    frame_sequence: u32,
    samples_per_pixel: u32,
    frame: FrameInfo
}
```

`JobHandle::preview()` returns an `Arc<RenderPreview>` whose `png()` is the
immutable encoded snapshot. The first checkpoint has sequence 1. The job's
`JobId { session, serial, generation }` and captured `scene_generation` remain
unchanged across every pass. `frame_sequence` is a separate monotonic counter.
Clients cache previews by `(JobId, frame_sequence)` and may retain an older
snapshot without mutating it.

The preview slot is separate from `JobStatus.frame`/`JobHandle::frame()`. A
preview cannot be exported through `render.export`, cannot become the session's
last completed image, and does not release the running task or film. The final
PNG is published only after its digest is complete; it clears the preview slot
and is the only new exportable result. `render.export` writes those exact PNG
bytes and does not render or re-encode them.

Cancellation clears the preview slot immediately. Failure and completion also
clear it. Publication checks the cancellation token while holding the same
record lock used for cancellation, including after the final digest chunk.
The worker cannot publish a late checkpoint into a cancelled or replaced job.
An externally retained preview remains an immutable snapshot of its original
job, but cannot replace a previous successfully completed final frame.

## Allocation and retained snapshots

Admission reserves the working film, existing RGBA/encoder/final-PNG capacity,
and at most two maximum-sized encoded checkpoint slots with bounded metadata.
The checkpoint allowance is independent of the requested sample count. Actual
film and PNG capacities are checked against their admitted capacities after
fallible allocation.

A completed checkpoint moves its PNG allocation into an immutable preview.
`Reservation::split` transfers a prepaid slot without increasing the pool total.
Dropping its last `Arc` returns the slot to the same running job; keeping an old
snapshot alive keeps that slot billed. Replacing the host's old snapshot needs
no consumer acknowledgement. If both slots are externally pinned, the job
records `preview_retention_skips`, omits that snapshot and continues sampling
and final export. A freed slot allows later complete passes to publish again;
`frame_sequence` counts published snapshots and `samples_per_pixel` identifies
the actual pass, so consumers must not equate the two after a retention skip.

Completion or cancellation closes the reusable pool and releases unused slots.
Externally retained snapshots keep their own leases until their last owner drops
them. The final PNG retains its separate reservation and never borrows a pinned
preview's slot. There is no full-film clone per checkpoint.

## Regression coverage

The source includes regressions for deterministic prefixes and worker quanta,
fresh primary rays, non-power-of-two targets, actual HDR/rough-reflection PNGs,
rectangular/LED PNG prefixes against numerical light integrals and area-shadow cancellation,
linear averaging before tone mapping, coverage and unexposed backgrounds,
bounded/cancelled film mutations, and the single cumulative work budget.

Host tests manually advance the worker to the digest/publication boundary. They
hold multiple preview `Arc`s through replacement, cancellation and completion,
check allocation identity and exact pool release, and reject an invalid
checkpoint sequence. Document tests cover offline limits and saved round trips.
Engine tests cover Check/Start equality, cold rejection before HDR capture,
session ownership, exact final export, and retention of the previous completed
image after a progressive job is cancelled. Execution evidence belongs to the
integrated CI receipts; this source handoff does not claim those gates passed.


`transparent_background: true` exports the existing primary coverage as straight
PNG alpha. Fixed AA and every complete progressive prefix use the same conditional
scene colour; clear pixels are zero RGBA and no display background is mixed into
edge colour. Opaque false/default pixels and work remain unchanged. Raw EXR
premultiplied radiance/coverage is independent of this display choice. See
[CPU output presentation](render-cpu-output.md#transparent-png-coverage).


The optional [CPU path lighting](render-cpu-path.md) consumer adds bounded
progressive diffuse/specular GI and its conditional general-Glass transport only
when `path` is authored. The restrictions above describe the absent-Path route.
Published source has document/engine/workspace regression coverage; unsupported
Glass/anisotropy/external-volume combinations and native Glass visual acceptance
remain explicit in [the parity matrix](render-parity.md).

## Optional independent-pilot allocation

CPU `raytrace.adaptive` (saved `CpuJob.options.adaptive`) uses the shared
`AdaptiveSampling` policy. It requires an explicit progressive production cap of
at least 16 samples. Pilots use an independent seed domain; their RGB and coverage
Welford statistics allocate each pixel a fixed 16..production-cap budget. Pilot
radiance is discarded through a cooperative reset. Production preserves the
ordinary CPU seed and zero-based sample ordinals, and stopped pixels consume no
more transport variates or rays. Every complete pilot and production sweep still
passes through the same two-slot checkpoint publication; the UI can coalesce
physical paints, and externally pinned slots can cause retention skips.

All existing CPU profile limits include **pilot_samples + samples_per_pixel**;
no cap or profile is raised automatically. At most 256 pilots and 4096 total
pilot/production passes are possible in the largest selected profile. Compact
retains its 64-pass and 16,777,216 camera-sample bounds. A 32-pilot/64-production
request therefore needs an explicit profile that admits 96 total passes. The
cumulative work budget also includes initialization, reset, skipped-pixel cursors,
finishing and publication. Statistical allocation can miss rare paths and does
not certify a noise level or completion time.

Per-pixel sums, coverage and sample counts remain f64/u16 until complete-pass
resolve. RGB and raw FLOAT32 EXR coverage are divided by each pixel's own
production count; no uniform denominator or PNG alpha quantization enters the
raw master. Shared finishing borrows the film's exact count plane and rounds
straight-alpha PNG bytes from the integer covered/sample ratio. It allocates no
additional count snapshot. The raw EXR remains final-only and unfiltered.
`JobStatus.sampling` records the actual production count range, evaluations and
pixels at the cap; `PreviewInfo.adaptive` distinguishes pilot and production
ordinals. A production sweep ordinal is not a promise of uniform per-pixel work.

The existing certified optical/aniso route, physical lens, direct lights/HDR,
opaque path lighting and completed-pass denoiser use the same adaptive camera
identity. Their existing geometry, coat, optical/HDR and path-combination guards
remain in force. Omission preserves the ordinary film allocation, random keys,
sample order and debits; actual new inline metadata is billed by size rather
than hidden behind the historical memory estimate.

The independent-pilot implementation and its numerical, host-ownership,
saved-request and UI regressions are included in the published CI7 source.
See [qualification](render-parity.md#qualification). That execution does not
certify adaptive convergence, every material combination or native Glass quality.
