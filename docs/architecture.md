# Architecture

<!-- ANCHOR: architecture -->
| Layer | Crates | Role |
|---|---|---|
| L0 | `geom` | vectors, planes, profiles, meshes and their measures |
| L1 | `sketch`, `kernel`, `render`, `dynamics` | constraint solver and profiles; the B-rep kernel boundary (truck); view math and the headless scene description; multibody dynamics (rigid bodies, joints, forces, time integration) |
| L2 | `doc` | parameters, expressions, the feature timeline and its incremental evaluation |
| L3 | `io` | design files, STL/OBJ/STEP/3MF export, STEP and 3MF/STL/OBJ/glTF import features |
| L4 | `engine` | the session and the command registry (everything is a command) |
| L5 | `gpu`, `ui-egui`, `mcp` | compute-shader evaluation of display tessellation plans; the swappable desktop front end; SolidCraft's MCP tools (headless or bridged to the app) over the shared `mcp-server` |
| shared | `color`, `ui-registry`, `ui-text`, `theme`, `mcp-server`, `control`, `control-egui` | crates other front ends use: they depend only on other shared crates, solidcore and external crates (UI toolkits included), any crate may use them, and `cargo xtask shared` builds and tests each one on its own |
| apps | `solidcraft`, `solidcraft-cli` | desktop app, headless CLI |
<!-- ANCHOR_END: architecture -->
