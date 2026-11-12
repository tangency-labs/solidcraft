# Simulation on GPUs and every platform

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

## 1. The honest premise

The mathematics is portable. The *performance* is not. Writing "mostly mathematical code" gets you
a correct solver on every platform; it does not get you the same speed, the same precision or the
same numerical behaviour. The non-portable parts are:

- **Shader precision.** WGSL has no portable f64. Apple GPUs have no double precision at all.
  Anything that needs f64 must stay on the CPU or use compensated f32.
- **Memory.** An iGPU shares system RAM and may have low per-buffer limits; a dGPU has fast VRAM but
  pays for transfers; a browser tab has a hard memory ceiling.
- **Watchdogs.** Windows TDR and macOS/browser GPU timeouts kill long command buffers. Kernels must
  be short and batched, not one heroic dispatch.
- **Drivers.** Metal-only on macOS, Vulkan/DX12 on Windows, Vulkan on Linux, WebGPU in the browser.
  DX12 shader compilation can take minutes on first run; GL has no compute.
- **Threads.** Native has threads; wasm has none unless we add Web Workers and JS glue, which the
  repo deliberately keeps minimal.

So the plan is not "GPU everywhere". It is: **one correct CPU f64 path everywhere, plus a GPU path
that is selected only where it is faster and verified.**

## 2. What "works everywhere" means

| Platform | Primary path | Fallback | Guarantee |
|---|---|---|---|
| macOS, Apple Silicon | Metal compute (f32) | CPU f64 | Every feature; GPU used where it helps |
| macOS, Intel + AMD/Intel GPU | Metal compute | CPU f64 | Every feature; GPU when compute is available |
| Windows, NVIDIA/AMD/Intel | Vulkan or DX12 compute | CPU f64 | Every feature |
| Linux, NVIDIA/AMD/Intel | Vulkan compute | CPU f64 | Every feature |
| Browser with WebGPU | WebGPU compute | Cooperative CPU (wasm) | Small studies only, capped |
| Browser without WebGPU | — | Cooperative CPU (wasm) | Small studies, may be slow |
| Headless/CI, no GPU | CPU f64 | software adapter for shader tests | Full correctness |

The guarantee is about **correctness and feature availability**, not speed. Every command, study
type and result view works on every platform. GPU acceleration is never required.

## 3. Precision policy

| Layer | Type | Why |
|---|---|---|
| Element assembly | f64 on CPU | Conditioning; cheap relative to the solve |
| Iterative solve (SpMV, dot, axpy) | f32 on GPU, f64 on CPU | The hot loop |
| Convergence decision | f64 residual on CPU | Never trust an f32 residual to declare victory |
| Stress/strain recovery | f64 on CPU | Cheap O(n) and the quantity users read |
| Fields on device | f32 for display | Display precision is enough |
| LBM | f32 in lattice units | Standard; lengths converted carefully |

Rules:

- A GPU result is labelled `mixed-f32-f64` and marked **verified** only when the f64 residual meets
  the requested tolerance and the CPU recovery completed.
- Ill-conditioned problems (thin shells, high aspect ratio, near-incompressible materials) route to
  CPU f64. The preflight reports why.
- Dot products use deterministic trees, not atomics. If f32 dot error is marginal, use
  double-single (two f32s) for that reduction; this is a measured decision per kernel.
- Modal results use f64 on the CPU for small models and GPU Lanczos with f64 residual checks for
  large ones; near-degenerate modes trigger a CPU re-solve.

## 4. Device selection

Reuse what already exists: `crates/viewport-gpu/src/select.rs` surveys adapters, classifies
hardware vs software, ranks discrete over integrated over virtual over unknown over CPU, exposes
`Policy::{Auto, Hardware, Software, Cpu}`, `Survey`, `pick`, `pick_among`, and the
`SOLIDCRAFT_GPU` environment variable. `native_backends()` is Metal on macOS, DX12|Vulkan on
Windows, Vulkan|GL elsewhere.

Add one shared capability check for compute:

```text
usable(adapter) =
    adapter.get_downlevel_capabilities().flags.contains(COMPUTE_SHADERS)
    && adapter.limits().max_storage_buffers_per_shader_stage >= 4
    && adapter.limits().max_compute_invocations_per_workgroup >= 64
    && adapter.limits().max_buffer_size >= problem.min_buffer_bytes()
```

`GL` (WebGL2) fails the compute check and falls through to CPU, as the viewport already does.

Selection order:

1. The user's preference: `Auto`, `Hardware`, `Software`, `Cpu`, or a named adapter.
2. Per-study override (`sim.run { backend: ... }`) for reproducibility.
3. Auto-tune: on first use per adapter/driver, run a short calibration (a ~1 s SpMV and a small
   LBM step) and cache the result. Small problems go to CPU when transfer overhead dominates; large
   ones go to the fastest device that fits in memory.
4. `sim.devices` reports what was surveyed, chosen and why. `sim.selftest` proves it works.

Preferences (new rows in `preference_settings!`, old behaviour preserved, `Changes a default: no`):

- Simulation backend: Auto (default), CPU only, a specific adapter.
- Memory budget: a fraction of physical memory, default conservative.
- Allow GPU while the viewport is busy: off by default on integrated GPUs.

## 5. Kernel inventory

Each kernel is a small WGSL entry point with a CPU twin. Every kernel must pass the same gates:
CPU agreement, determinism on the same device, watchdog-safe dispatch size, cancellable, bounded
memory, no panic on device loss.

| Study | GPU kernels | Notes |
|---|---|---|
| Static | assembly (f32 optional), SpMV, dot, axpy, Jacobi/Chebyshev preconditioner, residual | Assembly may stay f64 CPU until profiling proves otherwise |
| Modal | SpMV, dot, axpy, Lanczos basis, M-orthogonalisation | f64 residual checks |
| Buckling | Same as modal with `K_g` | Reuses modal kernels |
| Thermal | Assembly, SpMV, dot, axpy | Same skeleton as static |
| Topology | Sensitivity, density filter (convolution), projection, volume reduction | The filter is the GPU win |
| Flow | Voxelise, collide, stream, boundary conditions, reductions, drag/lift integrate | LBM is naturally GPU-friendly |
| Display | Field colour mapping, deformation | Reuse viewport pipelines; do not duplicate |

Shared primitives live in `sim-gpu`: buffers, chunking, deterministic reductions, pipeline cache,
device-loss handling, progress/cancel. No solver-specific policy leaks into `fea`.

## 6. Memory, unified memory and discrete GPUs

- **Preflight estimate** before allocation: nodes, DOFs, nonzeros, vectors, factor fill-in (if
  direct), fields, plus 20% headroom. Refuse before allocating, naming the limit and the required
  amount.
- **Device limits are law.** `max_buffer_size` can be small on downlevel adapters. Chunk large
  arrays into several buffers, as `viewport-gpu` already does for vertex buffers.
- **Unified memory (Apple Silicon, AMD APUs).** No discrete copy cost, but bandwidth is shared with
  the display, the CPU and the browser. Avoid unnecessary readbacks; keep vectors resident; do not
  run a heavy solve while the user is orbiting if the calibration says it hurts frame rate.
- **Discrete GPUs.** Upload the mesh and materials once, keep them resident for all iterations,
  read back only scalars during the loop and fields at the end. Never re-upload per iteration.
- **Browser.** Practical ceiling is well under 2 GB; the wasm build caps problem size accordingly
  and refuses with an estimate rather than crashing the tab.

## 7. Multi-GPU and shared compute

Two different things, deliberately separated:

**A. Job-level parallelism (v1).** One problem per device. Independent studies, parameter sweeps,
independent topology studies or candidates, and flow cases run concurrently across adapters. This is the useful multi-GPU
feature for normal users, scales near-linearly across independent work, and needs no halo exchange.
The engine's queue assigns a device per job, respects per-device memory budgets and merges results
by study id. A laptop with an iGPU and a dGPU can use both.

**B. Single-problem domain decomposition (later, experimental).** Partition the mesh across devices,
exchange halos each iteration. On discrete GPUs the exchange is host-staged and expensive; on
unified memory it is cheaper but still a copy. Expect 1.3–1.8× on two GPUs, not 2×. Gate behind a
preference, off by default, and only offer it when one device cannot hold the problem. Do not
promise linear scaling.

**Shared compute with the viewport.** The simulation device is separate from the window's device,
which keeps the solve out of the viewport's command stream. On the same adapter it still competes for
execution and memory bandwidth, so frames can be delayed; chunked submissions reduce the impact but do
not guarantee isolation. The scheduling policy is validated by contention tests. On integrated GPUs,
a "pause heavy solve while interacting" preference keeps navigation smooth.

## 8. Watchdogs, cancellation and progress

- No single dispatch or command buffer longer than ~50 ms. Iterations are batched: submit N
  iterations, poll with a timeout, check cancel, update progress, submit again.
- Windows TDR defaults to about 2 s; macOS and browsers also kill long GPU work. Treat 2 s as a
  hard ceiling, 50 ms as the design target.
- Cancellation is a flag read between batches, so it takes effect within one batch (< 500 ms).
- Device loss is an error (`DeviceLost`), not a crash: mark the run failed, keep the study, offer
  CPU retry, log adapter and driver in the run summary.
- Never `poll(Wait)` for the whole run; poll with a timeout so progress and cancel stay live.

## 9. Browser specifics

- wgpu on wasm uses WebGPU. Compute is available where the browser exposes it; WebGL2 is not a
  compute path, so it falls back to CPU.
- No rayon. The CPU solver is cooperative: solve in chunks, yield, let the tab paint.
- Caps: problem size, memory, and a wall-clock budget; refuse with a clear message.
- Results render through the same field viewer; no second UI.
- A Web Worker is a later, separate change; it needs JS glue, which the repo keeps to the service
  worker and small inline scripts.

## 10. Platform quirks to encode, not discover

| Quirk | Handling |
|---|---|
| macOS has only Metal, no software adapter | CPU fallback is the only fallback |
| DX12 compiles shaders slowly (repo note: minutes, #3217) | Prefer Vulkan on Windows for compute; cache pipelines; allow DX12 opt-in |
| GL/WebGL2 has no compute | Capability check excludes it |
| Software adapters (llvmpipe, lavapipe, WARP) exist | Use them for shader/correctness tests, never as the default |
| Some adapters report `IntegratedGpu` but use system memory | Detect via limits and calibration, not the device type name |
| Laptop thermal throttling | Long runs report sustained throughput; avoid time-based assertions in tests |
| Multiple adapters can be the same physical GPU | Use a stable adapter index or PCI bus id when available; never collapse two adapters that share a name, backend and driver |

## 11. Accepting a GPU path

A kernel is accepted only when all of these are recorded in the PR:

1. CPU vs GPU agreement on the benchmark set, with the measured relative difference.
2. Determinism on the same device, same driver, same run count.
3. Dispatch-size and watchdog test (a forced long loop still progresses and cancels).
4. Memory estimate vs actual peak, and the refusal path when over budget.
5. Device-loss injection: the app survives and the CPU retry works.
6. `sim.selftest --json` output from at least one Apple Silicon machine and one other vendor.
7. A note in the PR on what was *not* verified (hardware the contributor did not have).

## 12. Non-guarantees

- No claim that GPU and CPU produce identical bits, or identical results across vendors.
- No claim of a specific speed-up on hardware we have not measured.
- No claim that every adapter behaves; the capability gate and CPU fallback are the contract.
- No claim that the browser can solve the same problem sizes as desktop.
