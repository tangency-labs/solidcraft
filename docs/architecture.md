# Architecture

<!-- ANCHOR: architecture -->
| Layer | Crates | Role |
|---|---|---|
| L0 | `geom` | vectors, planes, profiles, meshes and their measures |
| L1 | `sketch`, `kernel`, `render`, `dynamics` | constraint solver and profiles; the B-rep kernel boundary (the vendored `solidcore`); product rendering (render jobs and their backends) on the shared viewport renderer; multibody dynamics (rigid bodies, joints, forces, time integration) |
| L2 | `doc` | parameters, expressions, the feature timeline and its incremental evaluation |
| L3 | `io` | design files, STL/OBJ/STEP/3MF export, STEP and 3MF/STL/OBJ/glTF import features |
| L4 | `engine` | the session and the command registry (everything is a command) |
| L5 | `gpu`, `ui-egui`, `mcp` | the GPU path tracer and SolidCraft's GPU environment over the shared viewport renderer; the swappable desktop front end; SolidCraft's MCP tools (headless or bridged to the app) over the shared `mcp-server` |
| shared | `color`, `ui-registry`, `ui-text`, `theme`, `ui-kit`, `look`, `mcp-server`, `control`, `control-egui`, `viewport`, `viewport-gpu`, `viewport-egui` | crates other front ends use: they depend only on other shared crates, solidcore and external crates (UI toolkits included), any crate may use them, and `cargo xtask shared` builds and tests each one on its own. The viewport renderer is three: `viewport` (camera, pictures, CPU rasterizer, shading), `viewport-gpu` (its wgpu pipelines and plan evaluation) and `viewport-egui` (the viewport in an egui window). The UI kit (`ui-kit`) holds the tokenised controls and the components built from them (ribbon, app bar, history and navigation bars, view compass, preferences window, start page, window chrome, theme files), drawn from data the host installs or passes. SolidCraft's look (`look`: its icon masters and colour roles, code-drawn glyphs, IBM Plex faces and built-in themes) is a shared crate too, so SolidCraft and other hosts install the one copy. A crate below L5 reaches no UI toolkit or wgpu through them, or any other way: `cargo xtask layers` follows workspace dependencies to check it |
| apps | `solidcraft`, `solidcraft-cli` | desktop app, headless CLI |
<!-- ANCHOR_END: architecture -->

How the layers, the shared crates and the solidcore seam fit together, with what enforces each rule: [layers.md](layers.md).
