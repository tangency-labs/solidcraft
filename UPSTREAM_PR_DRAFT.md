# Project a hole rim when the click is in the opening

## Summary

Sketch Project ignored a click in a hole. The opening is not a face, and it is too far from the rim for edge picking, so the tool returned before `sketch.project` and showed no error.

A ray through the opening now names the rim edges. Projecting them adds the linked purple circle (two semicircle arcs). A click on nothing reports `Project: nothing under the cursor`.

Not in this change: occurrence-aware Project (#47 / #2523), silhouettes, or merging the two semicircles into one circle entity.

## Cause

`crates/ui-egui/src/sketch_tools.rs` (`on_click`, `Mode::ModelRef`) only ran the command when `model_ref` could see a hit. A click in the opening produced none. `SolveApp::run` already surfaces command errors; this click never called it.

`crates/doc/src/project.rs` already projects a circular edge, and a face projection already includes hole loops. Auto-project on a sketch created on that face already draws the hole. The gap was the click itself, including large holes whose centre is farther than a quarter of the mesh diagonal from every face (`the face no longer exists`).

## Change

- `hole_rim_along_ray` finds the nearest planar opening on the ray (inside the outer loop and an inner loop) and returns one point on each rim edge.
- Project uses those edges when the click is not already a solid or sketch entity. Planes and axes are only used when the ray does not pass through an opening, so the origin plane cannot hide a hole.
- A miss sets an error on the status line. A command error was already shown; a refusal that returns before that (a file operation in progress) is shown too.

## Test plan

- [x] `cargo test -p solidcraft-engine --lib a_ray_through_a_hole_opening_projects_its_rim`
- [x] `cargo test -p solidcraft-ui-egui --test projection_preselection clicking_a_hole_opening_projects_its_rim_and_a_miss_is_an_error`
- [x] `cargo test -p solidcraft-ui-egui --test projection_preselection` (the existing projection tests still pass)
- [x] `cargo fmt` on the touched files

Manual: plate with a through hole, sketch not already on that face, Project, click the dark centre. The rim appears as linked geometry. Click empty space: an error, no new curves.
