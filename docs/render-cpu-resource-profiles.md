# Explicit CPU resource profiles

**The combined CPU-feature renderer build and profile/admission regressions have executed; current combined workflow and large-scene runtime acceptance remain pending.** These envelopes are admission limits, not measured performance or a final-quality certification.

## Intent

A CPU image must not inherit a small interactive-preview ceiling when the owner explicitly asks for a large still or a heavy assembly. Conversely, opening an old file must not silently authorize substantially more memory or work.

`job.options.budget` in a saved `cpu_raytrace` setup, or `raytrace.budget` in a runtime CPU request, selects one of three finite profiles. Omission means `compact`, preserving the previous policy. Unknown names, including `unlimited`, are rejected. The CPU dropdown preserves dimensions and sample count when changing profiles; an incompatible draft remains visible until the user corrects it or explicitly accepts proposed dimensions.

| Envelope | Compact | Studio | Workstation |
|---|---:|---:|---:|
| Maximum side | 4,096 px | 8,192 px | 16,384 px |
| Fixed-sampling internal pixels | 4,194,304 | 16,777,216 | 134,217,728 |
| Progressive internal pixels | 1,048,576 | 16,777,216 | 134,217,728 |
| Progressive samples/pixel | 64 | 1,024 | 65,536 |
| Total camera samples | 16,777,216 | 4,294,967,296 | 8,796,093,022,208 |
| Captured triangles | 262,144 | 2,000,000 | 8,000,000 |
| Visible bodies | 16,384 | 65,536 | 262,144 |
| Retained source geometry | 128 MiB | 1 GiB | 4 GiB |
| Image/preparation admission envelope | 384 MiB | 3 GiB | 128 GiB |
| Cumulative work units | 1,000,000,000 | 1,000,000,000,000 | 1,152,921,504,606,846,976 |

With [X2 supersampling](render-quality-controls.md), these dimensions and pixel
counts apply to the internal grid, while the selected output size remains
unchanged. The new output UI stops at 8K UHD and offers sample selections through
16,384; legacy native-size/API intent is preserved. Denoising, diagnostics and
shared output presentation have their own final 8192-side/33,554,432-pixel cap.

Every column is an intersection of limits: a scene that fits the triangle count may still exceed its image/preparation byte estimate. Similarly, a sample count within the per-pixel limit may exceed the total camera-sample or cumulative traversal-work limit. No claim is made that all maxima can be reached simultaneously or within a particular time.

Workstation permits up to 65,536 total pilot/production passes. Exact CPU coverage
and adaptive counts use 32-bit integers, with their larger count plane included
in admission. Its camera-sample ceiling is the product of the maximum internal
pixel count and the existing 65,536-pass ceiling: 2^43. Its work ceiling is 2^60.
The 128 GiB image/preparation admission envelope accommodates the combined UI quality controls,
including 16,384 production spp plus valid adaptive pilots, 8K final output,
X2, denoising, diagnostics, raw output and 64 workers. Available RAM under the
default policy, scene-dependent storage and cumulative work remain separate
admission limits.

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

The native CPU host has a finite 256 GiB software reservation pool on 64-bit
platforms; it does not allocate that amount at startup. Workstation's 128 GiB
image/preparation admission envelope remains smaller. Retained request and
resource metadata are additionally charged to that pool and the shared RAM
ledger; working images and retained results/previews also hold reservations
there. BVH preparation independently retains its existing 12 GiB ceiling, which
covers the admitted geometry cardinality; films, guides and output planes use
the remaining image/preparation envelope.

Native CPU and registered GPU hosts additionally share one process-wide
reservation ledger with its own 256 GiB software ceiling. Physical admission
uses available system RAM and the process's remaining Linux cgroup capacity.
The smaller supported observation funds an allocation epoch independently of
the software ceiling. Capture, prepared or queued work, running jobs and reusable
checkpoint credit keep that observation fixed while allocations remain possible.

Once no unsettled allocation promises or active work remain, the next admission
starts a new epoch with a fresh observation. Immutable retained images, previews
and metadata do not block that refresh. Their leases remain charged to software
limits, but the new OS observation already measures the machine with those
objects alive. Their old physical debits are therefore excluded from the new
epoch. This updates admission after other programs acquire or release RAM
without treating unused reservations as already allocated memory.

An unavailable query records a labeled fallback of at most 512 MiB, bounded by
any reported cgroup limit. It is an absolute allowance across all held leases;
retained objects do not earn a new fallback allowance. A genuine zero-available
result remains zero.

Preferences → Graphics → **Use previous render memory checks**
(`render_legacy_memory_admission`, default false) restores native software-pool
admission. The CLI equivalent is `--legacy-memory-admission`; embedders can set
`NativeMemoryPolicy::SoftwarePoolOnly` on an `ImageHost`. It skips only the
physical-capacity refusal and applies to future checks/reservations. Both modes
retain the shared 256 GiB software ceiling, each host's budget, device limits and
fallible allocation checks. All held leases stay in software accounting. Both
policies also update the physical epoch debit, so a legacy host's current-epoch
reservations reduce the allowance for another host using the RAM gate.

Native admission includes a `host_memory` receipt. `required_bytes` includes any
already-held `credited_bytes`; only their difference, `additional_bytes`, needs
new funding. `admitted_bytes` reports other held software leases.
`physical_debit_bytes` reports the reservations charged to the current RAM
observation, or all held leases when the query used the conservative fallback.
`capacity_bytes` records that observation, and `available_bytes` is its remaining
allowance for additional bytes, saturating at zero. The receipt also reports the
shared software limit, source and policy. See [the field definitions](render-quality-controls.md)
for the full contract.

A legacy admission can exceed the physical allowance while still reporting the
observed capacity, including zero. Capacity refusals report byte counts before
an image is allocated. Splits, transfers, shrinking, cancellation and release
preserve software accounting and the appropriate epoch debit. The snapshot is
not a promise against unrelated processes allocating memory afterward.
Cooperative hosts and GPU executors keep their own finite software limits, and
selecting a CPU profile does not change a GPU limit.

Fallible allocation, actual vector-capacity checks, checked arithmetic, bounded work quanta, cancellation checks and final-only publication remain required. A failed or cancelled job must release its working reservations and preserve the previous completed result. Every retained progressive checkpoint is still conservatively charged; this increment does not implement tiled/out-of-core rendering or unbounded preview retention.

## Compatibility and limits

The new pending-format descriptor is `render-cpu-budget`. Old intent has Compact semantics; larger intent is not discarded on reopen. The independent GPU sampling flag remains separate from CPU progressive sampling. Visual material edits still do not change engineering material properties.

Resource-profile selection alone does not enable CPU diffuse global illumination, adaptive sampling, arbitrary dielectric geometry, texture displacement or EXR output. Native CPU worker selection is described separately in [quality controls](render-quality-controls.md). Optional [opaque path lighting](render-cpu-path.md) and [output presentation](render-cpu-output.md) have their own explicit contracts. The existing optical-combination restrictions remain explicit. Raising an admission ceiling is not a performance optimization or a claim that every large scene is practical yet.

## Regression coverage

Tests cover explicit 4K/8K admission without allocating a large film, BVH
cardinality estimates for larger profiles, old/unknown profile handling,
offline saved round trips, preservation of incompatible UI drafts, finite
per-host limits, arithmetic overflow and linear film accumulation above
64 samples. Previously published renderer admission and above-64-sample film
regressions have executed. Additional regression cases exercise the combined
maximum UI settings, shared CPU/GPU native-capacity accounting, frozen worker
counts, cancellation and snapshot resets. Their current-source outcomes belong
to the corresponding validation receipts. Offline document/UI, host and platform
qualifications remain separate; admission tests without a large film do not
establish large-scene performance.

## Display is separate from output

The result canvas validates image dimensions against the captured job, not a subsequently edited profile. The exported PNG is never resized or re-encoded for display. If a viewport's texture-side limit or the finite display-cache pixel limit is smaller than the result, only its UI preview is reduced, with an explicit warning and native 100% inspection disabled. Full-resolution tiled inspection remains pending. One displayed completed result may be pinned while a newer job completes, preserving exact export and accounting for its retained memory.
