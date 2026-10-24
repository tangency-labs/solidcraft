# Explicit CPU resource profiles

**Implementation batch: source prepared; consolidated compilation and large-scene runtime acceptance are pending.** These envelopes are admission limits, not measured performance or a final-quality certification.

## Intent

A CPU image must not inherit a small interactive-preview ceiling when the owner explicitly asks for a large still or a heavy assembly. Conversely, opening an old file must not silently authorize substantially more memory or work.

`job.options.budget` in a saved `cpu_raytrace` setup, or `raytrace.budget` in a runtime CPU request, selects one of three finite profiles. Omission means `compact`, preserving the previous policy. Unknown names, including `unlimited`, are rejected. The CPU dropdown preserves dimensions and sample count when changing profiles; an incompatible draft remains visible until the user corrects it or explicitly accepts proposed dimensions.

| Envelope | Compact | Studio | Workstation |
|---|---:|---:|---:|
| Maximum side | 4,096 px | 8,192 px | 16,384 px |
| Fixed-sampling pixels | 4,194,304 | 16,777,216 | 67,108,864 |
| Progressive pixels | 1,048,576 | 16,777,216 | 67,108,864 |
| Progressive samples/pixel | 64 | 1,024 | 4,096 |
| Total camera samples | 16,777,216 | 4,294,967,296 | 68,719,476,736 |
| Captured triangles | 262,144 | 2,000,000 | 8,000,000 |
| Visible bodies | 16,384 | 65,536 | 262,144 |
| Retained source geometry | 128 MiB | 1 GiB | 4 GiB |
| Complete job admission | 384 MiB | 3 GiB | 12 GiB |
| Cumulative work units | 1,000,000,000 | 1,000,000,000,000 | 16,000,000,000,000 |

Every column is an intersection of limits: a scene that fits the triangle count may still exceed its complete byte estimate. Similarly, a sample count within the per-pixel limit may exceed the total camera-sample or cumulative traversal-work limit. No claim is made that all maxima can be reached simultaneously or within a particular time.

Examples of cardinality admission, before geometry and memory are considered: Studio permits 3840 × 2160 at 128 samples/pixel; Workstation permits 7680 × 4320 at 128 samples/pixel. Compact rejects both instead of silently resizing them.

## Example saved intent

```json
{
  "name": "CPU studio still",
  "set": {
    "width": 3840,
    "height": 2160,
    "job": {
      "method": "cpu_raytrace",
      "options": {
        "budget": "studio",
        "primary_samples": 1,
        "progressive": { "samples_per_pixel": 128, "seed": 82 }
      }
    }
  }
}
```

Submit this to `render.settings.set`. `render.preflight` and `render.start` accept the resulting named setup. The preflight reply identifies the CPU budget, all of its limits, and the estimated job admission. `render.backends` also lists the profiles without starting a device or allocating an image.

Large intent can be saved and reopened without a render host. Executing Studio or Workstation requires a native 64-bit host. A missing feature, unavailable host, unsupported optical combination or exceeded profile is an error, never a switch to another executor.

## Accounting and responsiveness

The source-capture gate, dimensions, camera-sample bound, BVH construction, film, encoded checkpoints, environment input, retained request and host reservation consume the same selected policy. Ray tracing no longer adds raster projected-triangle lists to its estimate: its own BVH, captured looks and film are charged instead, alongside a conservative pixel/codec allowance.

The native CPU host has a finite 16 GiB logical reservation pool on 64-bit platforms, including retained results and previews. It does not allocate that amount at startup, promise that physical RAM is available, or account for the rest of the CAD application or other processes. Per-job profiles remain smaller. Cooperative and registered GPU host pools retain their original 512 MiB limit; this change is not a GPU-memory expansion.

Fallible allocation, actual vector-capacity checks, checked arithmetic, bounded work quanta, cancellation checks and final-only publication remain required. A failed or cancelled job must release its working reservations and preserve the previous completed result. Every retained progressive checkpoint is still conservatively charged; this increment does not implement tiled/out-of-core rendering or unbounded preview retention.

## Compatibility and limits

The new pending-format descriptor is `render-cpu-budget`. Old intent has Compact semantics; larger intent is not discarded on reopen. The independent GPU sampling flag remains separate from CPU progressive sampling. Visual material edits still do not change engineering material properties.

This increment does not introduce CPU diffuse global illumination, adaptive sampling, multithreaded transport, arbitrary dielectric geometry, texture displacement or EXR output. The existing optical-combination restrictions remain explicit. Raising an admission ceiling is not a performance optimization or a claim that every large scene is practical yet.

## Prepared regression coverage

Tests cover explicit 4K/8K admission without allocating a large film, BVH cardinality estimates for larger profiles, old/unknown profile handling, offline saved round trips, preservation of incompatible UI drafts, host-pool isolation, arithmetic overflow and linear film accumulation above 64 samples. Their execution belongs to the consolidated batch gate; formatting or static source checks alone do not constitute a passing test run.

## Display is separate from output

The result canvas validates image dimensions against the captured job, not a subsequently edited profile. The exported PNG is never resized or re-encoded for display. If a viewport's texture-side limit or the finite display-cache pixel limit is smaller than the result, only its UI preview is reduced, with an explicit warning and native 100% inspection disabled. Full-resolution tiled inspection remains pending. One displayed completed result may be pinned while a newer job completes, preserving exact export and accounting for its retained memory.
