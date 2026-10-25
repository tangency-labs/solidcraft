# Sketch Project does nothing when the click is in a hole

## Summary

With Project active, clicking the opening of a hole (the dark centre, not the rim) adds no sketch geometry and reports no error. The tool stays ready until it is cancelled.

This is the interactive miss. It is not the occurrence-aware Project work in #47 / #2523, and it does not change silhouettes.

## Steps

1. Create a plate with a through hole. One case that shows it: box 40 × 30 × 20 mm, hole diameter 10 mm, position `[20, 15, 20]`, depth 20 mm.
2. Start a sketch on a plane that does not already project that face. XY offset 30 mm is enough. (A sketch created on the holed face already receives the hole from auto-project; this bug is the later click.)
3. Sketch → Reference → Project.
4. Click the centre of the hole, not the rim.

Headless equivalent: start `sketch.project` and click the world point `[20, 15, 20]`.

## Expected

The rim becomes linked sketch geometry (drawn purple, fixed, `LinkKind::Project`). A circular hole is stored as two semicircle edges, so the circle is two linked arcs of the hole radius.

A click that hits nothing should say so. It should not look like the tool ignored the click.

## Actual

Nothing is added. The status line keeps the Project hint. There is no toast. Cancel leaves the sketch unchanged.

A recording of that click is the report this draft is written from: Project is armed, the cursor is in the hole, the click produces no highlight and no geometry.

## Cause

The opening has no triangles, so it is not a face hit. Edge picking only accepts an edge within 6 screen pixels, so the centre is not an edge hit either. `sketch_tools::on_click` then returned without calling `sketch.project`.

That is why the failure is silent. `SolveApp::run` already puts a command error on the status line (`finish_command`). This path never got that far. Swallowing the `Result` with `let _ = app.run(...)` is not what hides this click.

The engine can already project the rim when it is given the edges. Projecting one semicircle draws one linked arc of the hole radius. Projecting both draws the circle.

Sending the click point as a face is not a substitute. `nearest_face` rejects a point farther than a quarter of the mesh diagonal (`the face no longer exists`). A diameter 80 mm hole in a 100 mm plate is past that limit, even though the rim is a perfectly ordinary circular edge. A smaller hole can happen to resolve as the surrounding face; the opening itself still has to be found as a loop.

## Fix

`solidcraft_doc::project::hole_rim_along_ray` takes the click ray and returns the nearest hole opening it passes through: a point that lies inside a planar face's outer loop and inside an inner loop. Each edge of the smallest such loop is named by a point on it, in world space.

The Project tool uses that only when the click is not already a vertex, edge, face, or other sketch's curve or point. Those still project as themselves. The rim edges go into one `sketch.project`, so one undo removes them. An origin plane behind the opening does not take the click. If the ray hits nothing, the status line is `Project: nothing under the cursor`.

A body past the scan limits (8 000 faces, 20 000 edges, 400 000 triangles) is skipped. That click still gets the error, rather than hanging.

Hidden bodies are skipped. A placed body is tested in the mesh frame `mesh_in_place` already uses. This does not add occurrence paths; that remains #2523.

## Tests

- `cargo test -p solidcraft-engine --lib a_ray_through_a_hole_opening_projects_its_rim`
- `cargo test -p solidcraft-ui-egui --test projection_preselection clicking_a_hole_opening_projects_its_rim_and_a_miss_is_an_error`

The engine test checks a ray through the opening, an angled ray, a ray through material, a ray aimed away, a hidden body, then projects the two rim edges onto a sketch and checks radius and centre. It also checks that a point in a large opening is rejected as a face (`the face no longer exists`) while the rim ray still projects.

The UI test clicks the hole centre with Project active (origin shown) and expects at least two linked arcs of radius 5 mm about `(20, 15)`. It then hides the origin, clicks empty space, and expects an error containing "nothing" with no new curves.
