# SolidCraft

SolidCraft is an open-source parametric 3D CAD application written in pure Rust.
Sketch on a plane, constrain and dimension the sketch, turn profiles
into solids with features on a timeline, and change a parameter to watch the whole timeline
rebuild.

![The sample plate in SolidCraft](generated/sample-plate.png)

It runs as:

- a **desktop app** for Windows and Linux (egui + wgpu);
- a **web app** in the browser (wasm, WebGPU or WebGL2), [try it here](app/);
- a **headless CLI**, `solidcraft-cli`, that runs command scripts, measures and exports;
- an **MCP server** that AI agents model with, headless or driving the running app.

Everything the user can do is a *command* with JSON parameters. The toolbar, the dialogs, scripts,
the CLI, the control channel and MCP all run the same commands, so anything you can click you can
also script, test or hand to an agent.

SolidCraft is early. Sketching, solid modelling, parameters, components with joints, sheet metal
and a plastic-enclosure subset work; surfaces, drawings, CAM and simulation are deferred. The
[roadmap](https://github.com/bherbruck/solidcraft/blob/main/ROADMAP.md) has the details, and
the [feature list](dev/features.md) lists every command by area, and the
[reference tests](dev/oracle.md) check modelling results against analytic and FreeCAD values.

Existing `.solidcraft` designs and `solidcraft-cli` commands keep their names and behavior.

This book has two halves: the **user guide** for people modelling with SolidCraft, and the
**developer guide** for people working on it. The [command reference](reference/commands.md) is
generated from the code on every build.

SolidCraft is licensed MIT OR Apache-2.0. It is independent: behaviour is designed from public
documentation, general CAD conventions and user feedback, and reference tests use analytic
values and open-source tools. SolidCraft is not affiliated with Autodesk, Dassault Systèmes,
PTC or any other CAD vendor; product names such as Fusion, SolidWorks and Onshape are trademarks of
their respective owners, named only to describe compatibility.
