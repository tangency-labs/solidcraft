# solidcraft-viewport

The viewport renderer the Craft apps share, so each of them frames, projects, shades and draws a
picture the same way. It takes solidcore-tess's display meshes and plain data (colours, boxes,
lines) and knows no app's geometry or document types. Its only workspace dependency is
`solidcraft-color`.

It holds the view maths, the description of a picture and its CPU rasterizer, the surface
shading models and the section hatches, and the GPU work ([`gpu`]): the viewport's wgpu pipelines,
offscreen pictures, adapter selection and display plans evaluated in a compute shader. Product
rendering (render jobs, the path tracer, studio, film, textures) stays in the app.

The crate reads no environment of an app's: the host chooses the adapter policy and whether bodies
are built on the CPU, and passes them in. wgpu's own `WGPU_BACKEND` and `WGPU_DX12_COMPILER` still
apply. The shaders are in `shaders/`.

## Tests

The GPU tests open their devices under `VIEWPORT_TEST_GPU` (`auto`, `hardware`, `software`, `cpu`;
`auto` when unset) and skip without an adapter, unless `VIEWPORT_TEST_REQUIRE_GPU` is set, when
they fail. CI sets `VIEWPORT_TEST_GPU=software` and `VIEWPORT_TEST_REQUIRE_GPU=1`.

## API

| Item | What it is |
|---|---|
| `Camera`, `UpAxis`, `StandardView`, `ViewAngles`, `CameraAnim`, `ease_in_out` | The orbit camera: perspective or orthographic projection, fit to a box, screen ↔ world mapping and picking rays, standard views and their animation |
| `Mat4`, `Quat` | The camera's matrices and rotations |
| `Scene`, `SceneMesh`, `SceneLine`, `Rgb`, `MIN_ALPHA`, `SEE_THROUGH`, `SECTION_CAP`, `SECTION_HATCH`, `png` | A picture as data: shaded meshes, lines, section cap and hatch colours, and PNG encoding |
| `AnalysisMode`, `SceneAnalysis` | Surface analysis shading (zebra, draft, curvature, environment, access) and its palette |
| `render`, `render_png`, `render_transparent`, `Canvas`, `shade` | The CPU rasterizer (z-buffer, shaded triangles, depth-tested lines) for pictures without a GPU, and its shading |
| `surface::OpaqueSurface`, `clear_coat::ClearCoat` | The opaque metallic-roughness shading model and its clear coat |
| `hatch::Hatch`, `hatch::HATCH_COLOURS` | Section hatches by part |
| `vec::Bounds`, `vec::Vec3Ops`, `vec::Vec2Ops` | The renderer's box and vector maths on solidcore-record's vectors |
| `gpu::view` (`Renderer`, `Frame`, `GpuScene`, `GpuTarget`, `GroundAo`, ...) | The viewport's pipelines: what a frame draws, the scene blocks, palette, ambient occlusion, transparency |
| `gpu::capture` (`Capturer`, `shared`, `headless`, `Headless`) | Pictures drawn offscreen with the same pipelines, or by the CPU rasterizer where the policy lands there |
| `gpu::select` (`Policy`, `pick`, `survey`, `Tier`) | Adapter selection for headless devices under a policy the host passes |
| `gpu::TessGpu`, `gpu::GpuMesh`, `gpu::PackedPlans`, `gpu::SHADER` | Display plans (solidcore-tess's) evaluated in a compute shader, and the shader's input as data |
| `gpu::open_headless`, `gpu::depth_features`, `gpu::depth_format`, `gpu::instance_descriptor_from_env`, `gpu::GpuError` | Devices, depth formats, wgpu instance settings and errors |
