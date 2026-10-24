# CPU progressive camera sampling

The limits below describe the compatibility **Compact** profile. Explicit Studio and Workstation envelopes are documented in [CPU resource profiles](render-cpu-resource-profiles.md); their implementation batch still requires consolidated validation.

CPU progressive rendering repeatedly traces fresh camera samples into a bounded
linear-light film. Checkpoints are complete image passes, at 1, 2, 4, 8, 16 and
32 samples per pixel when below the requested final sample count. The exact
requested count produces the final PNG, including non-powers of two such as 3
or 5. A one-sample job has no provisional checkpoint.

This mode uses the native `cpu-raytrace` executor and its existing direct-light
and selected-reflection transport. It does not add diffuse global illumination,
caustics, adaptive convergence, a denoiser, or general glass path tracing.

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

| Control | Initial CPU progressive contract |
| --- | --- |
| Dimensions | Each side 1..4096; at most 1,048,576 pixels |
| Samples per pixel | 1..64 |
| Total camera samples | At most 16,777,216 (`width × height × samples_per_pixel`) |
| Fixed spatial AA | `primary_samples` must be 1 |
| Selected mirror / isotropic rough reflection | Existing bounded depth; `reflections.samples` must be 1 when bodies are selected |
| Directional / point / spot sources | Existing CPU light contracts and limits |
| Constant / captured HDR environment | Existing CPU contracts; HDR plus rough reflections still requires certified fully metallic selected bodies and effective faces |
| Surface emission / isotropic surface effects | Existing CPU appearance contracts; emission is visible radiance |
| Area / LED sources | Explicitly rejected in this initial progressive mode |
| Dielectric / anisotropic face overrides | Explicitly rejected in this initial progressive mode |
| Output | Opaque PNG with the existing display background |

An incompatible request fails; it does not remove settings or switch methods.
The fixed CPU route retains its existing area, dielectric and anisotropy support.
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
separate domains of the same counter. Target sample count, worker quantum and
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
and the maximum encoded capacity plus metadata for every possible nonfinal
checkpoint. This conservative upper bound covers a consumer retaining all
checkpoints at once. Actual film and PNG capacities are checked against their
admitted capacities after fallible allocation.

A completed checkpoint moves its PNG allocation into the preview object.
`Reservation::split` transfers that allocation's share of the running lease
without increasing or decreasing the pool total. Replacing or clearing the host
preview drops only the host's `Arc`; any external `Arc` keeps its memory billed.
At final completion the task releases its working buffers before the remaining
lease shrinks to the final PNG. Dropping each last external snapshot releases
only that snapshot's reservation. There is no full-film clone per checkpoint.

## Regression coverage

The source includes regressions for deterministic prefixes and worker quanta,
fresh primary rays, non-power-of-two targets, actual HDR/rough-reflection PNGs,
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
