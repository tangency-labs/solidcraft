# Architecture

SolidCraft is a Cargo workspace in layers. A crate only depends on crates in lower layers, and
`cargo xtask layers` fails the build otherwise.

| Layer | Crates | Role |
|---|---|---|
| L0 | `geom` | vectors, planes, profiles, meshes and their measures |
| L1 | `sketch`, `kernel`, `render` | constraint solver and profiles; the B-rep kernel boundary (truck); view math and the headless scene description |
| L2 | `doc` | parameters, expressions, the feature timeline and its incremental evaluation |
| L3 | `io` | design files, STL/OBJ/STEP/3MF export, STEP and 3MF/STL/OBJ/glTF import features |
| L4 | `engine` | the session and the command registry (everything is a command) |
| L5 | `gpu`, `ui-egui`, `mcp` | compute-shader evaluation of display tessellation plans; the swappable desktop front end; the MCP server (headless or bridged to the app) |
| apps | `solidcraft`, `solidcraft-cli` | desktop app, headless CLI |

The web app, `apps/solidcraft-web`, is the desktop front end compiled to wasm with
[trunk](https://trunkrs.dev). `xtask` holds the repository's own tooling: `cargo xtask ci`,
`layers`, `assets`, `features`, `oracle` and `book`.

A few rules follow from the layering:

- **Only `crates/kernel` touches truck.** Everything above it sees SolidCraft's own types:
  bodies, faces, edges, meshes and measures. See [The kernel boundary](kernel.md).
- **The document knows nothing about commands or the UI.** `crates/doc` holds the parameters,
  the feature timeline and the component tree, and evaluates them into a model.
- **Everything the user can do is a command** in `crates/engine`. See
  [The command engine](commands.md).
- **The front end is swappable.** Nothing below `ui-egui` depends on egui. The CLI and the MCP
  server run the same engine without a window. Headless pictures (`solidcraft-cli snapshot`, MCP
  screenshots, `ui.render`, thumbnails) are drawn on the GPU with the viewport's own pipelines
  (`crates/gpu`, `capture`): the engine describes the scene (`view::scene`) and the host installs
  the renderer (`engine::capture`). The tiers, in order: a hardware GPU, a software adapter (Mesa
  lavapipe or llvmpipe, WARP), and last `crates/render`'s CPU rasterizer, which draws no surface
  analysis, highlights or legends. `SOLIDCRAFT_GPU=auto` walks all three, `hardware` and `software`
  stay on their tier (an error without one), `cpu` forces the rasterizer; every picture's reply
  names its `renderer` (`gpu`, `gpu-software`, `cpu`) and a CPU picture carries a warning.
  `solidcraft-cli gpu` lists the adapters and the tier the policy lands on.
