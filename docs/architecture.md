# Architecture

<!-- ANCHOR: architecture -->
| Layer | Crates | Role |
|---|---|---|
| L0 | `geom`, `laser` | vectors, planes, profiles, meshes and their measures; laser device profiles and LightBurn handoff packages |
| L1 | `sketch`, `kernel`, `render` | constraint solver and profiles; the B-rep kernel boundary (truck); view math and the headless scene description |
| L2 | `doc` | parameters, expressions, the feature timeline and its incremental evaluation |
| L3 | `io` | design files, STL/OBJ/STEP/3MF export, STEP and 3MF/STL/OBJ/glTF import features |
| L4 | `engine` | the session and the command registry (everything is a command) |
| L5 | `gpu`, `ui-egui`, `mcp` | compute-shader evaluation of display tessellation plans; the swappable desktop front end; the MCP server (headless or bridged to the app) |
| apps | `solidcraft`, `solidcraft-cli` | desktop app, headless CLI |
<!-- ANCHOR_END: architecture -->
