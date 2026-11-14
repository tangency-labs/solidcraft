# solidcraft-viewport-egui

The shared viewport renderer in an egui window: `solidcraft-viewport-gpu`'s pipelines drawn in
egui-wgpu paint callbacks, on the host's device and render pass, over a background the host paints
first. Its workspace dependency is `solidcraft-viewport-gpu`.

The host passes what is its own: whether bodies may be built on the GPU and whether they must be
built on the CPU, the background's colours, and how to report a GPU error no error scope caught.

egui-wgpu must carry the fix in the repository's vendored copy (`vendor/egui-wgpu`, listed in
`vendor/README.md`): the window's drawable is acquired before paint callbacks are prepared, without
which the cached offscreen picture is never drawn. A host workspace patches egui-wgpu to that copy
with `[patch.crates-io]`, as the repository's root `Cargo.toml` does.

## API

| Item | What it is |
|---|---|
| `install`, `TessReady` | The pipelines put in egui-wgpu's callback resources, with the plan evaluation compiling beside the window |
| `ViewportCallback`, `ProductHighlightCallback` | The paint callbacks that draw a frame and its product highlights |
| `paint_background` | The view's background gradient from the host's colours |
| `log_uncaptured_errors` | GPU errors logged and handed to the host instead of panicking |
