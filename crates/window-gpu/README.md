# solidcraft-window-gpu

SolidCraft's own window painter on a stock egui-wgpu. It is a copy of egui-wgpu 0.37's
`winit::Painter`, drawn only through egui-wgpu's public API, with four targeted changes:

- a window other than the main one presents without waiting for vsync where its surface can;
- a whole frame can be handed to a frame compositor (the blur behind panels);
- the frame is acquired before paint callbacks are prepared;
- a window larger than the device's largest texture gets a clamped surface.

It has no workspace dependencies. A host draws its windows with it instead of eframe's painter.

## API

| Item | What it is |
|---|---|
| `painter::Painter` | The painter: surfaces per window, the device, egui's renderer, paint and present |
| `FrameCompositor`, `Compositor` | A hook that takes over whole frames, and the callback resource it is found in |
| `render_run` | Draws a run of an uploaded frame from a mesh offset with stock `Renderer::render` |
| `viewport_present_mode` | The present mode a window's surface is configured with |
| `clamp_surface_extent` | The surface size and draw scale for a window larger than the largest texture |
