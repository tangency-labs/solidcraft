# solidcraft-viewport-gpu

The shared viewport renderer's GPU work, on wgpu: the viewport's render pipelines, pictures drawn
offscreen with them, adapter selection for headless devices, and display plans (solidcore-tess's)
evaluated in a compute shader. It draws what `solidcraft-viewport` describes (cameras, scenes,
shading models) and is the same in a window and in a headless picture, so the two shade alike. Its
workspace dependencies are `solidcraft-viewport` and `solidcraft-color`. The shaders are in
`shaders/`.

The crate reads no environment of an app's: the host chooses the adapter policy and whether bodies
are built on the CPU, and passes them in. wgpu's own `WGPU_BACKEND` and `WGPU_DX12_COMPILER` still
apply.

## Tests

The GPU tests open their devices under `VIEWPORT_TEST_GPU` (`auto`, `hardware`, `software`, `cpu`;
`auto` when unset) and skip without an adapter, unless `VIEWPORT_TEST_REQUIRE_GPU` is set, when
they fail. CI sets `VIEWPORT_TEST_GPU=software` and `VIEWPORT_TEST_REQUIRE_GPU=1`.

## API

| Item | What it is |
|---|---|
| `view` (`Renderer`, `Frame`, `GpuScene`, `GpuTarget`, `GroundAo`, `TessLater`, `tessellation`, ...) | The viewport's pipelines: what a frame draws, the scene blocks, palette, ambient occlusion, transparency, and the plan evaluation compiled beside the window |
| `capture` (`Capturer`, `shared`, `headless`, `Headless`) | Pictures drawn offscreen with the same pipelines, or by the CPU rasterizer where the policy lands there |
| `select` (`Policy`, `pick`, `survey`, `Tier`) | Adapter selection for headless devices under a policy the host passes |
| `TessGpu`, `GpuMesh`, `PackedPlans`, `SHADER` | Display plans evaluated in a compute shader, and the shader's input as data |
| `open_headless`, `depth_features`, `depth_format`, `instance_descriptor_from_env`, `GpuError` | Devices, depth formats, wgpu instance settings and errors |
