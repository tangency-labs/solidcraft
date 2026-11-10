# Layers: how SolidCraft's crates fit together

SolidCraft is a Cargo workspace in layers. A crate depends only on crates below it, on the shared crates, on solidcore's three public crates and on external crates. `cargo xtask layers` fails the build on any other edge. The crates and their roles are listed in [architecture.md](architecture.md); this page is how they fit together and what each rule is enforced by. Where a job belongs between SolidCraft and solidcore is [solidcore/docs/layers.md](../solidcore/docs/layers.md).

![One click through the layers: (1) the host in L5 runs the command the ribbon component returned, in the engine; (2) the engine adds the feature to the document's timeline; (3) the document builds it through solidcore-ops; (4) the document plans the body's display mesh through solidcore-tess; (5) L5 hands the plan to the shared viewport-gpu crate, which evaluates it on the GPU; (6) the shared viewport-egui crate draws it in L5's window.](layers/layers.svg)

*One click on Fillet. The layers L5 to L0 are on the left, the shared crates on the right, and solidcore below the dashed line. egui and wgpu stop at the dotted line under L5: the shared crates may use them, the layers below may not, at any depth. The solidcore line is crossed only through `solidcore-ops`, `solidcore-tess` and `solidcore-record`.*

1. The user clicks Fillet. The ribbon is a component: SolidCraft's toolbar (in `ui-egui`, L5) hands it plain data (tabs, groups, labels, icons) and it returns the button clicked. The host runs that button's command, `solid.fillet`, in the engine (L4).
2. The engine adds the feature to the document (L2), which rebuilds the timeline from the first feature that changed.
3. The document builds the fillet through `solidcore-ops`. It computes no geometry itself.
4. The document plans the new body's display mesh through `solidcore-tess`, once per plan cell.
5. L5 hands the plan to the shared `viewport-gpu` crate, which evaluates it in a compute shader.
6. The shared `viewport-egui` crate draws the result in L5's window.

## 1. The rules

| Rule | What it means | Enforced by |
|---|---|---|
| Downward only | A layered crate (L0 to L5) depends only on lower layers. Within a layer, only the orderings in `INTRA_LAYER_ORDER` (`xtask/src/layers.rs`) are allowed, for example `gpu` before `ui-egui`. | `cargo xtask layers` |
| Shared crates stand alone | A shared crate depends only on other shared crates, solidcore's public crates and external crates, UI toolkits included. Any crate may depend on a shared crate. | `cargo xtask layers`; `cargo xtask shared` builds and tests each shared crate in a workspace of its own, which fails on any other dependency |
| UI toolkits stay at the top | egui, eframe, egui-wgpu, wgpu, winit and the other UI crates appear only in L5, the apps and the shared crates. The check follows workspace dependencies, so a crate below L5 cannot reach a toolkit through a shared crate either. | `cargo xtask layers` |
| One seam to solidcore | SolidCraft uses `solidcore-ops` (modelling), `solidcore-tess` (display meshes and plans) and `solidcore-record` (geometry as data). `solidcore` and `solidcore-convert` are solidcore's own; what a client needs from them is re-exported by a public crate (for example `solidcore_tess::{EvalSurface, vec3}`). | `cargo xtask layers` against `xtask/solidcore-public.txt`; `xtask/solidcore-seam.txt` lists the old direct uses and only shrinks |
| Test tooling | `testkit` may use anything up to L5; other crates use it only as a dev-dependency. | `cargo xtask layers` |
| The browser | Every crate below the apps, and the web app, builds for wasm32. | `cargo xtask wasm` |

## 2. Components: what makes a piece shared

The interface is built from components that another front end could use as they are. A component:

- **takes plain data**: labels, icons, entries, values and the host's prefix for its names. It reads no app state;
- **returns what the user chose**: an action, a row, a button. The host runs it as a command. A component that must record several requests in a frame returns them as a list for the host to run after drawing, in order;
- **draws with the shared theme**: every colour, size and space from `solidcraft_theme::Tokens`; text through `solidcraft_ui_text`; every outer window or popup through `ui_kit::Container` (`cargo xtask ui-containers`);
- **publishes its handles** into `solidcraft_ui_registry` under the host's prefix, so the control channel, the scenarios and the tests find it. SolidCraft's names stay byte-identical when a piece becomes a component;
- **is tested on plain data**, without the app. The host's own tests still pass unchanged.

A piece moves into a shared crate once it depends only on what a shared crate may: the viewport crates, the theme, the text catalogs and the control channel have. The rest of the kit lives in `ui-egui` until it does. When a piece becomes a component, before/after screenshots of SolidCraft show no differing pixels.

## 3. Adding or moving a crate

1. Give it a row in `TABLE` (`xtask/src/layers.rs`) with its class and a comment saying what it holds, and a row in [architecture.md](architecture.md).
2. A shared crate also needs `[lints] workspace = true`, a licence and a README.md beside it (`cargo xtask shared` checks all three).
3. Run `cargo xtask layers`, `cargo xtask shared` and `cargo xtask wasm`.
