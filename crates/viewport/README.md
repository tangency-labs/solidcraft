# solidcraft-viewport

The viewport renderer the Craft apps share, so each of them frames, projects, shades and draws a
picture the same way. It takes solidcore-tess's display meshes and plain data (colours, boxes,
lines) and knows no app's geometry or document types, no GPU API and no UI toolkit, so an app's
engine and command line can use it. Its only workspace dependency is `solidcraft-color`.

It holds the view maths, the description of a picture and its CPU rasterizer, the surface shading
models and the section hatches. Two crates build on it: `solidcraft-viewport-gpu` (the wgpu
pipelines, offscreen pictures and plan evaluation) and `solidcraft-viewport-egui` (the viewport in
an egui window). Product rendering (render jobs, the path tracer, studio, film, textures) stays in
the app.

## API

| Item | What it is |
|---|---|
| `Camera`, `UpAxis`, `StandardView`, `ViewAngles`, `CameraAnim`, `ease_in_out` | The orbit camera: perspective or orthographic projection, fit to a box, screen ↔ world mapping and picking rays, standard views and their animation |
| `Mat4`, `Quat` | The camera's matrices and rotations |
| `nearest_drawn_point`, `DrawnPoint` | Picking drawn lines: of the host's polylines, the point drawn nearest the pointer, with its line id, segment, parameter on the segment and gap in pixels |
| `Scene`, `SceneMesh`, `SceneLine`, `Rgb`, `MIN_ALPHA`, `SEE_THROUGH`, `SECTION_CAP`, `SECTION_HATCH`, `png` | A picture as data: shaded meshes, lines, section cap and hatch colours, and PNG encoding |
| `AnalysisMode`, `SceneAnalysis` | Surface analysis shading (zebra, draft, curvature, environment, access) and its palette |
| `render`, `render_png`, `render_transparent`, `Canvas`, `shade` | The CPU rasterizer (z-buffer, shaded triangles, depth-tested lines) for pictures without a GPU, and its shading |
| `surface::OpaqueSurface`, `clear_coat::ClearCoat` | The opaque metallic-roughness shading model and its clear coat |
| `hatch::Hatch`, `hatch::HATCH_COLOURS` | Section hatches by part |
| `vec::Bounds`, `vec::Vec3Ops`, `vec::Vec2Ops` | The renderer's box and vector maths on solidcore-record's vectors |
