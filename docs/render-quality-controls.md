# Render quality, CPU workers and guide diagnostics

The Render workspace exposes sample presets through **16,384 spp**, an **8K UHD
(7680 × 4320)** output preset, optional **X2 supersampling**, CPU worker selection,
and explicit denoising settings. These controls are saved with the selected
render setup. Check and Render consume the same captured values; reopening a
file, changing an unrelated control or applying Undo does not clamp a legacy job.

## Controls and saved values

| Control | Saved CPU option | Saved GPU option | Meaning |
|---|---|---|---|
| Samples | `progressive.samples_per_pixel` | `samples` | New UI selections from 1 through 16,384; power-of-two presets from 1 through 16,384 |
| Supersampling | `supersampling` | `supersampling` | `1` keeps native size; `2` traces twice the output width and height |
| CPU workers | `threads` | Not applicable | `0` Auto, `1` Single, or an explicit count from 2 through 64 |
| Denoising | `denoise` | `denoise` | Enable the existing geometry-aware spatial display filter |
| Filter passes | `denoise_settings.passes` | Same | 1 through 3, using steps 1, 2 and 4 in order |
| Filter strength | `denoise_settings.strength` | Same | Finite per-pass blend from 0 (raw image) through 1 (full selected filter) |
| Firefly suppression | `denoise_settings.firefly_suppression` | Same | Optional first-pass isolated-bright-pixel heuristic on ordinary surfaces |
| Filter mirror and glass | `denoise_settings.optical_filter` | Same | Enabled by default for eligible optical guides; disable to restore opaque-only denoising |
| Guide diagnostics | `denoise_diagnostics` | Same | Capture guide eligibility, bypass counts and a categorical mask |

CPU options live under `job.options` for a saved `cpu_raytrace` setup and under
`raytrace` for a runtime CPU request. GPU options live under `job.options` for a
saved `gpu_path` setup and at the top level of an explicit progressive runtime
request. Width and height continue to describe the **exported image**.

The defaults are native resolution, no diagnostic capture, and three-pass,
full-strength denoising with firefly suppression off when denoising is selected.
Opaque filter weights remain unchanged. Eligible mirror and glass pixels are now
filtered by default; `denoise_settings.optical_filter: false` restores the earlier
opaque-only policy without disabling opaque denoising. Omitted CPU worker selection
uses Auto. Single retains the serial
execution path. Existing API and saved sample counts above 16,384 remain supported
within their original backend/profile limits; the UI preserves them and explains
that newly authored selections stop at 16,384. Canonical GPU quality presets retain
64, 512 and 2,048 spp for Draft, Balanced and High, together with their authored
sizes, depths, seeds and denoising choices. CPU quality budgets retain 16, 32 and
64 spp. All three quality buttons remain available within the UI sample limit.

## Resolution and linear X2 reconstruction

The output controls accept sides up to 8192 and at most 33,554,432 pixels. The 8K
preset is 7680 × 4320. X2 at that size traces a 15360 × 8640 grid and returns a
7680 × 4320 image. It costs four times as many camera samples at the same spp;
it does not change the spp value or create four independent photon-map rebuilds.

Each 2 × 2 block is averaged in **scene-linear, coverage-premultiplied RGBA**,
using f64 accumulation and one f32 rounding. Alpha is resolved with radiance.
There is no averaging of encoded sRGB values, tone-mapped highlights or straight
RGB at transparent edges. For filtered output, denoising operates on the internal
grid first; reconstruction precedes exposure, tone mapping and finishing.
Consequently the filter footprint measured in final pixels changes with X2.

A retained raw FLOAT32 EXR is separately reconstructed from the unfiltered
internal radiance and coverage. It has the selected final dimensions and excludes
denoising, exposure and finishing. PNGs and progressive previews have the same
selected dimensions. The trace grid and actual camera-sample totals remain
visible in internal sampling/diagnostic metadata.

X2 is a distinct experiment from four times the native spp. For example, 8 spp
with X2 and 32 spp at native resolution have equal camera-sample counts, but
sample placement, photon-map refresh frequency and the denoiser footprint differ.
Compare both the raw masters and the finished images at the same output size.

## CPU execution and resource profiles

Native CPU workers share the immutable prepared scene, BVH, optical certification,
texture storage and completed photon map. Each worker owns only bounded transport
state, texture/effect scratch and a small result tile. Camera results are merged
in pixel/sample order, and photon deposits in photon ordinal order, so worker
scheduling does not alter the random keys or film accumulation order. A photon
map is completed before the corresponding camera pass uses it.

Auto selects the host's available parallelism once during admission, capped at
64. The admission receipt records that count in `cpu_workers`; task creation and
every camera/photon batch retain it even if available parallelism later changes.
Single admits one worker and creates no parallel pool. An explicit count from
2 through 64 is retained exactly rather than reduced to the available core count.
Admission prepays the selected workers' stacks and scratch before the pool is
created, and the image job reuses that pool. Pool creation failure is an error.
Workers share the cumulative work budget and cancellation token.

The wasm build retains cooperative serial execution for Auto and Single; the
native worker selector is unavailable there. A saved explicit count above one
is preserved but refused for browser execution.

Resource profiles remain explicit. Compact keeps its 64-spp progressive ceiling,
and Studio keeps 1,024. Select Workstation for a new 16,384-spp request. Workstation
retains its 65,536-pass API ceiling, including any independent adaptive pilots.
With X2, the profile pays for the internal grid and all camera samples.

| Workstation envelope | Maximum |
|---|---:|
| Internal side | 16,384 pixels |
| Internal pixels | 134,217,728 |
| Total camera samples, including adaptive pilots | 8,796,093,022,208 (2^43) |
| Image/preparation admission envelope | 128 GiB |
| Cumulative work units | 1,152,921,504,606,846,976 (2^60) |

The native CPU host's software reservation pool is 256 GiB across working
images and retained frames/previews. Retained request and resource metadata are
charged separately to that pool and the shared RAM ledger. These bounds allow
the combined UI quality request—16,384 spp, 8K final output, X2, 64 workers,
three full-strength denoise passes, firefly suppression, guide diagnostics and
raw output—to reach
the hardware-capacity check. Valid adaptive/photon options retain their own
checked counts and compatibility rules. The maximum-size regressions calculate
admission without constructing a film; they do not establish completion time or
claim that a maximum-size render has run.

Legacy native-size CPU Workstation requests retain their 16,384-side contract
when denoising, guide diagnostics, raw EXR and nonidentity output presentation are
disabled. Their profile pixel, memory and work limits still apply. New UI size
selections and CPU jobs using X2 or those output features use the 8K output
envelope; the UI does not silently resize legacy jobs on load.

Native CPU and registered GPU hosts also share one process-wide reservation
ledger with a 256 GiB software ceiling. Physical admission uses a separate
available-RAM snapshot and, on Linux, the process cgroup's remaining capacity.
Each allocation epoch funds work from one such observation. The snapshot stays
fixed while capture, prepared or queued jobs, running work, or reusable
checkpoint credit can still require allocations.

The next admission refreshes the observation once no unsettled allocation
promises or active work remain. Immutable completed images, previews and
metadata may stay alive across that refresh. Their leases still count against
software limits; the new OS observation already measures the machine with those
objects alive, so their old physical debits are not subtracted again.

If the native RAM query is unavailable, it records a labeled fallback of at most
512 MiB, bounded by any reported cgroup limit. That fallback is an absolute
allowance across all held leases; retaining results cannot renew it. A measured
zero remains zero.

By default, native admission also requires each additional reservation to fit
the physical allowance left in this epoch. Preferences → Graphics → **Use
previous render memory checks** (`render_legacy_memory_admission`, default false) restores
software-pool admission when selected. The machine preference applies to future
CPU and GPU checks and reservations; it does not change held leases or saved
image quality. The CLI equivalent is `solidcraft-cli render
--legacy-memory-admission`. Embedders can select
`ImageHost::set_native_memory_policy(NativeMemoryPolicy::SoftwarePoolOnly)`;
the default is `AvailableRam`.

Both policies retain the shared 256 GiB ceiling, each host's software/job limits,
device checks, checked arithmetic and fallible allocations. Legacy admissions
remain debited in the shared ledger, including when another host uses the default
RAM policy. Thus selecting the legacy preference neither grants unbounded memory
nor makes an allocation succeed.

The optional `host_memory` receipt separates the software ledger from the
physical allowance:

| Field | Meaning |
|---|---|
| `required_bytes` | This request's full debit, including its already-held credit. |
| `credited_bytes` | The part of this request already reserved. |
| `additional_bytes` | New bytes to reserve: `required_bytes - credited_bytes`. |
| `admitted_bytes` | Other held software leases: the total software debit minus this request's credit. |
| `physical_debit_bytes` | Reservations charged to this epoch's observation, before adding this request's new bytes. With an unavailable-RAM fallback, all held leases remain charged. |
| `capacity_bytes` | The available-RAM/cgroup observation for this epoch, or the explicit fallback allowance. |
| `available_bytes` | Capacity minus `physical_debit_bytes`, saturating at zero. This is the allowance for `additional_bytes`. |
| `software_limit_bytes` | The shared software ceiling, enforced under either policy. |
| `source` | Whether capacity came from the system, system plus cgroup, or conservative fallback. |
| `policy` | `available_ram` or `software_pool_only`. |

Held credit cannot exceed either the request's full debit or the total software
reservation. Legacy admission may exceed the physical allowance; the receipt
still reports the observation, including zero. A capacity refusal reports the
needed and available byte counts before allocating the image. Preparation,
preview retention, completed-result shrinking, cancellation and final release
update the same ledger. An OS snapshot cannot promise memory against unrelated
processes that allocate later; fallible allocation and cancellation remain
necessary.

GPU admission additionally checks actual storage-binding and buffer limits and
its finite 128 GiB software envelope. It does not allocate that maximum at
startup. CPU admission includes film, guides, output scratch, diagnostics,
retained metadata and worker storage; BVH preparation keeps its separate 12 GiB
ceiling within the whole-job budget. An exceeded profile, actual-capacity or
compatibility limit preserves the requested values. No profile, resolution,
sample count, worker count or executor is changed automatically.

## Denoising controls and optical policy

Pass count, strength, firefly suppression and optical filtering affect display filtering only.
They do not change transport, the sample stream or the raw EXR. Original coverage
is preserved. Strength zero returns the original linear RGBA. The firefly toggle
controls isolated-bright-pixel capping and high-contrast tap attenuation. Disabling
it restores the previous opaque filter weights; ordinary bilateral contrast
weighting remains active.

The **Filter mirror and glass** control saves `denoise_settings.optical_filter`
for both CPU and GPU setups, including when denoising is temporarily disabled.
The default true retains the improved optical filter; false keeps all optical
pixels raw while ordinary opaque surfaces continue to use the selected filter.
Guide diagnostics retain their eligibility classifications in either mode.

Optical guides keep their conservative policy: they follow supported sharp
reflection and smooth-glass branches to compatible opaque endpoints, require
both contributing endpoint identities to agree, and use at most the first two
spatial steps. Isolated-outlier capping remains disabled on optical guides; the
selected high-contrast tap attenuation still applies. Raising
pass count or strength does not make unsupported optics eligible. Fine glints,
soft shadows and difficult glass still require sample convergence.

See [spatial denoising](render-denoise.md) for the guide and numerical contracts.

## Guide mask, counts and export

Diagnostics are opt-in and can be captured with filtering either enabled or
disabled. They record the actual reason selected during guide generation rather
than inferring optical eligibility from the final image. Surface and Optical
identify eligible guides; the other categories identify bypass conditions:
background, emitter, unsupported material, nested medium, depth of field, volume,
path depth, degenerate path, incompatible exit, total internal reflection,
further transmission, reflective endpoint, identity limit, partial coverage or
unavailable guidance.

The Inspector reports counts over the **internal trace grid**. The result viewer
can switch between the completed image and the guide mask. The mask is a separate
opaque PNG with fixed category colors and nearest-neighbor viewing; it never
replaces or modifies the color image or linear master. X2 reduces classes
conservatively: a bypass in a 2 × 2 block remains a bypass, and mixed eligible
Surface/Optical classes become Unavailable. Category numbers and palette colors
are never averaged.

Explicit mask export uses the existing atomic export boundary:

```json
{"command":"render.export","params":{"job":{"session":3,"serial":1,"generation":1},"path":"guide-mask.png","format":"denoise_mask"}}
```

`render.result.view {"view":"denoise_mask"}` selects the mask for a completed job
that captured it. Export replies label the file as `categorical_srgb` and opaque.
The `denoise_mask` metadata has its own dimensions, byte length and SHA-256;
`frame.png_sha256` still identifies the color PNG. Each retained file is hashed
independently and charged to the host lease. Cancellation or invalid output
cannot publish a partial mask.

Regrading a retained raw master preserves access to its source mask and original
diagnostic report. It does not regenerate geometry guides or reapply denoising.
The derivative's `denoise_applied` metadata remains false. A completed job that
did not capture diagnostics cannot acquire them from a later editor change.

## Example saved GPU setup

```json
{
  "command": "render.settings.set",
  "params": {
    "name": "Quality comparison",
    "set": {
      "width": 1920,
      "height": 1080,
      "job": {
        "method": "gpu_path",
        "options": {
          "samples": 32,
          "supersampling": 2,
          "denoise": true,
          "denoise_settings": {
            "passes": 2,
            "strength": 0.75,
            "firefly_suppression": false
          },
          "denoise_diagnostics": true,
          "linear_exr": true
        }
      }
    }
  }
}
```

Preflight this setup on the intended device before executing it. New saved fields
are documented by the pending `render-quality-controls` format note. Existing
qualification renders in [OPUS qualification](render-opus-quality.md) retain their
original source/binary identities; they are not evidence for a later build.
