# Command hint media

Original SolidCraft illustrations for the opt-in **Illustrated command hints**
preference. Models are built with normal SolidCraft commands and captured by its
GPU viewport. Exports are compiled into the app and work offline. Authoring tools
are separate from the application runtime.

Coverage includes **193 unique illustrated commands** across Model, contextual
Sketch, Surface, Assemble, Inspect and Sheet Metal. Shared commands reuse the
same artwork. Commands whose workflow is clearer in words keep text-only hints;
see [the complete decision list](../../docs/command-illustration-coverage.md).

Read the [visual guidelines](../../docs/command-illustration-guide.md) before
authoring or regenerating media. They incorporate the reviewed feedback on
construction principles, context, contrast, pacing and toolbar parity. Compare
related commands at actual tooltip size and check the static poster as well as
the animation; a generic final outline is not enough to distinguish variants.

`source/model.json` is the editable analytic model for the initial solid-operation
examples: a Rectangle sketch, Extrude, Fillet and Hole in timeline order. `source/extrude.json` adds the height parameter
used for the animation. Use the ordinary SolidCraft commands to edit either.
Each still export is 960 × 600, displayed at up to 480 × 300 logical pixels.
GIFs are 480 × 300 and contain at most 32 frames. Extrude has 26 frames; the
22 Sketch construction sequences use native input and previews. When **Animate
command hints** is enabled, the player repeats them in dropdown and pinned
toolbar hints while hovered, holding the final frame for three seconds before
each replay. Motion is off by default.

The `*-menu.json` source files contain reproducible examples for the covered menus. Each sample
stores setup commands, the illustrated operation, one fixed camera, genuine
selection/display settings, expected measurements and its single/pair layout.
Only the necessary construction references remain visible. The optional
`focus_point` is an editorial ring at a measured construction point, not extra
model geometry.

Axis illustrations use `after_ui.hint_capture.emphasize_axes: true`. SolidCraft
renders those axes at three times their usual stroke width, in a bright accent
with a contrasting edge. Positions, extents and dash spacing remain native model
geometry. Plane illustrations use `after_ui.hint_capture.emphasize_planes: true`:
a roughly 65% opaque orange fill and an opaque contrasting outline at twice the
usual width. The model remains visible through the plane. The capture tool
restores all session-only styles after each sample. Selected sketch curves use
`emphasize_sketches: true`, with a brighter blue and a wider native stroke. The same capture flag emphasizes native tool previews and suppresses coordinate readouts and pointer identity labels, retaining the native arrow, click ripples and useful dimension boxes.
Ordinary modeling display is unaffected.

Still Sketch pictures are normally captured after finishing the sketch, with its grid,
points, constraints and profile tint disabled when they are unrelated to the
example. Keep construction lines only when they explain the operation. Assembly
couplings use real driven poses; expected joint values are checked during capture.
The As-Built picture preserves the original component positions. The Unstitch
picture separates its genuine surface bodies to expose the result.

Inspect pictures retain meaningful native measurement and analysis annotations.
Their `after_actions` use the actual UI tools and picks; engine query results alone
are not drawn as invented overlays. Sheet Metal Flat Pattern and Perforate use the
native 2D flat preview, including real bend lines and perforation circles. Perforate
does not make visible holes in the formed B-rep.

A `crop_scale` of 0.5 captures a centered 480 × 300 logical pixel region directly
at 2× density. The 3D camera distance scales with the crop to preserve framing while
keeping native labels legible. Flat previews use their own camera and recorded
native pan/zoom actions. Other stills retain the original 960 × 600 logical crop.

`source/sketch-construction.json` is the sole editable recipe for the 22 Sketch
building animations and their static posters. Native defining points and sparse
construction guides make the poster useful when motion is off. Midpoint Line
uses a visible center reference and two native 20 mm half-length measurements;
its animation grows both halves together. Ordinary Line grows from its first
endpoint and ends its chain with a right-click. The scenes sit
away from the sketch origin. Four native frames per input give one click per
second, followed by the player's three-second result hold. Splines finish with
a right-click; tangent circles use the real native picks and radius input. For the two-tangent
circle, the authoring tool explicitly executes the same native engine command
after recording the field: synthetic Enter does not commit that floating input
on the capture host. Its radius and center are checked analytically.

Capture them with the authoring tool against a separate, verified clean window:

```sh
python3 -B assets/command-hints/tools/capture-sketch-animation.py --port 54940 \
  --token-dir target/command-illustrations/model-capture-control \
  --anchor target/command-illustrations/capture-anchor.sc
```

`--only sketch-circle-center,rectangle` selects individual sequences. The tool
requires the expected saved anchor to be the only document, with no unsaved edits,
active sketch, tool, dialog or file operation, before changing the window. It repeats
this check before every scratch document. Verify the isolated process and settings
directory when launching the capture app; the document check does not verify its launch environment.
The tool
checks solver health and actual result curve types, preserves the native camera
through each sequence, and restores the original document, view and cursor.
Intermediate frames and verification stay under ignored `target/`.

## Reproduce the illustration set

Start a separate app window with isolated settings and a saved, nonempty anchor
design, then run the authoring tool against that window's explicit control port:

```sh
python3 -B assets/command-hints/tools/capture.py --port 54940 \
  --token-dir target/command-illustrations/model-capture-control \
  --anchor target/command-illustrations/capture-anchor.sc
```

`--only chamfer,plane-offset` regenerates selected slugs. Intermediate captures
and measurement reports stay under `target/command-illustrations/model-captures`.
The tool verifies its scratch document, clean timeline, declared counts, volumes and joint values. It never replaces the original design,
and restores the original view and reference display setting after each sample.
Python and ImageMagick are needed only for authoring these assets.

Menu-recipe cameras describe the model-render framing. The native capture crop
uses 1.25 times that camera distance; paired pictures use a further factor of
1.5 and two central 480 × 600 crops. This keeps projections uniform, with no
stretching or gradient seams between the input and result. Reference-through-body
display makes internal construction planes and axes legible; it is restored after
the capture. Each result is still rendered by SolidCraft itself.

## Initial solid reference captures

These camera values describe the initial Model examples. Menu and Sketch
construction recipes declare their own cameras and layouts; use those recipes
when regenerating their exports.

Open the source recipe in a separate SolidCraft document. Disable grid, origin,
unrelated sketches, browser, timeline, navigation controls and ground shadow.
Use orthographic projection and capture both light and dark themes.

- Rectangle: top view, target `[0,0,0]`, distance 108.
- Solids and profiles: target `[0,0,9]`, yaw `-0.7853981633974483`,
  pitch `0.6154797086703874`, distance 125.
- Captures used a 1728 × 998 logical pixel window at 2× scale. The fixed viewport
  crop is physical `(768,432,1920,1200)`, resized to 960 × 600. Keep the same
  camera and crop throughout each sequence; adapt the crop for another window.
- Current menu before/after pairs join two central 480 × 600 crops in a
  960 × 600 canvas. The connecting arrow is an editorial direction cue, not model
  geometry. Keep editorial captions outside media; meaningful native dimensions
  and analysis annotations may remain.

Capture the selected profile before Extrude; the edge at `[24,-16,9]` before
Fillet; and the curved face at approximately `[21.94975,-13.94975,9]` afterward.
Use `select.set` and actual viewport screenshots for these overlays: `ui.render`
does not currently include face, edge or profile selection highlights.
The Hole pair starts with the completed fillet so only the hole changes.

The extrusion animation uses actual SolidCraft model states, not pixel morphs.
Hold the selected profile for 1000 ms, then render heights `18*i/24` for
`i=1..24`. Give each frame 40 ms, except every sixth frame gets 50 ms, so the
growth lasts exactly 1000 ms. The encoded source appends the result for 2000 ms;
the player replaces that final delay with its three-second replay pause. Model renders
use the same orientation, target `[0,0,9]`, distance 100, no grid or axes, and
960 × 600 output; crop `(105,65,750,469)` and resize to 480 × 300 to match the
viewport composition. Native GIF decoding occurs beside the window thread;
disabling motion and the browser use the paired static poster. A host that sets
egui animation time to zero also gets the poster; the current player does not read the
operating system's Reduce Motion setting.

Keep intermediate captures and review compositions under ignored `target/`.
Only these consumed exports and editable source recipes belong here. Provenance
is recorded in `attribution/pending/illustrated-command-hints.md`.
