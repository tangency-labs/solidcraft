# Command illustration guidelines

Use this guide when creating or revising graphical command hints. Its authoring
and review rules incorporate the feedback from the Model and Sketch illustrations.
They apply to dropdown menus and pinned tab pictograms.

Current coverage is **193 illustrated commands**, including Extrude and **22 Sketch
construction animations**, across Model, Sketch, Surface, Assemble, Inspect and
Sheet Metal. **Illustrated command hints** and **Animate command hints** are separate
opt-ins in Preferences > Appearance; both are off by default.

The [coverage list](command-illustration-coverage.md) records each command's media
or text-only decision. The [asset README](../assets/command-hints/README.md) covers
capture commands, source recipes and export specifications. Keep visual rules
here, editable recipes under `assets/command-hints/source/`, and provenance under
`attribution/pending/`.

## Explain the construction, not only the finished shape

A picture must explain the command's defining input or effect. A finished circle
or line alone cannot explain how it was constructed. Add the smallest amount of
real context that makes the principle visible: defining points, a center, a
reference edge, a construction guide or a native measurement.

Compare related commands together. If their illustrations look interchangeable,
revise the defining inputs or sequence. The title and written hint supplement the
illustration; they must not carry the entire difference between variants.

These decisions from actual feedback are requirements for subsequent artwork:

| Reviewed case | Authoring rule |
| --- | --- |
| Circle variants showed only circles. | Show the defining center and radius, diameter endpoints, three circumference points, or tangent references. Animate the real picks and changing preview. |
| Rectangle, Arc and Slot results hid their construction. | Show their distinct defining points and input order, including centers, corners, arc direction, centerlines and width. Use construction animations for these families. |
| Splines showed only the completed curve. | Animate point placement. Distinguish fit points from control points and retain the control polygon when it explains the result. |
| Midpoint Line looked like an ordinary line. | Show a visible reference for the first midpoint pick and growth on both sides. The poster must also distinguish it: the current example uses a construction guide and two native 20 mm half-length measurements. |
| Ordinary Line lacked a building sequence. | Show the first endpoint, the growing segment, the second endpoint and completion of the chain. Its anchored endpoint stays in place. |
| Circumscribed Polygon had too many circles. | Show one tangent incircle and the polygon. Omit redundant internal construction guides from the authored example; verify that the intended geometry remains correct. |
| Break seemed to have no visible effect. | Show genuine split points and selection of one resulting segment. Do not invent a gap or removed material. An unchanged silhouette is not by itself a reason to omit an illustration. |
| Dimension needed an illustration. | Show a real driving dimension, its native annotation and the geometry responding to the value. Reference measurements used to explain construction must be driven measurements that do not move the subject. |
| Axes and planes were too faint. | Review them at tooltip size in both themes. Use the stronger native axis and plane capture styles specified below. |

The 20 mm values are one verified example, not a required size for every midpoint
illustration. The general requirement is an identifiable midpoint and equal halves.

## Popup layout and toolbar parity

- Use a **480 × 300 logical pixel media area** with a **960 × 600 PNG poster**.
  GIF frames are 480 × 300. Use the existing popup container and theme spacing;
  do not invent separate chrome for a particular command.
- Keep the localized command name and shortcut above the media, and concise help
  and any disabled reason readable. Keep captions outside the captured scene.
- Place dropdown hints beside the whole menu, with the existing left/right
  fallback. They must not cover its selectable rows.
- Pinned pictograms use the same command media and motion preference. A family
  button must show the illustration for its remembered variant.
- Scale media proportionally to fit small windows and screen edges. Review it at
  its actual display size, not only as an enlarged export.
- Keep hints passive. Hovering must not run a command or alter a design.
  Playback controls need a separate interaction design before being added.
- Preserve text-only hints for commands without media. Active analysis commands
  retain their native Turn off hint rather than showing a creation demonstration.

## Visual grammar

| Element | Rule |
| --- | --- |
| Subject | One simple example and one operation. Make the relevant geometry large enough to read. |
| Context | Keep only references that explain the defining picks or effect. Removing noise must not remove the construction principle. |
| Camera | Orthographic; top view for planar sketches and a consistent isometric view for ordinary solids. |
| Framing | Frame the complete sequence, including native annotations. Keep camera target, scale, angle and crop fixed through the animation and its paired theme capture. |
| Existing geometry | Use SolidCraft's native neutral bodies, silhouettes and reference curves. |
| Focus | Use actual selection highlights and native previews. Colour must support shape and sequence, not carry the explanation alone. |
| Background | Supply light and dark variants using their native viewport backgrounds. |
| Noise | Hide the grid, unrelated origin axes and planes, browser, timeline, navigation controls, shadows and unrelated geometry. Hide points, dimensions and constraint glyphs only when they do not explain the command. |
| Sketch curves | Use capture-only blue emphasis for the subject and native preview. Keep native dash patterns and useful defining points; remove unrelated profile tint and glyphs. |
| Construction axes | Use three times the ordinary stroke width, a bright native capture accent and a contrasting halo. Preserve native position, extent and dash spacing. |
| Construction planes | Use roughly 65% opaque orange fill and an opaque contrasting border at twice the ordinary width. Keep the underlying part visible. |
| Guides | Prefer one necessary centerline, support circle or path over a complete collection of solver helpers. Native cleanup in the capture recipe must not change normal command behavior. |
| Annotations | Retain genuine dimensions, measurement lines, radius labels and analysis overlays when they explain the operation. Keep them readable, separated and inside the crop. Never substitute invented labels for an engine result. |
| Cursor | Omit it from posters and geometry-only concept sequences. For interaction sequences, use one plain native pointer and meaningful click ripples; hide the Agent tag and coordinate readout. |

Move examples away from the sketch origin when its editing marker would add
noise. Useful native dimension input boxes may remain during construction, but
must not obscure the defining picks or be clipped by the illustration crop.
Keep ordinary modeling display unchanged: illustration emphasis is a temporary
capture style, not a new application default.

## Choose the representation that explains the command

Use construction animation when pick order, anchoring or changing previews are
part of the principle. The current Line, Circle, Rectangle, Arc, Slot and Spline
sequences follow this rule. A blank input followed by a generic finished outline
is insufficient when it conceals the defining inputs.

Every animation also needs an informative static poster. Preserve the defining
points, necessary guides or native measurements so motion-off and browser users
can still understand the command. Do not blindly choose a final frame that loses
that context.

Use paired before/after stills for effects such as Fillet, Chamfer, Shell, cuts,
transformations and constraint changes when motion adds little. Keep their scale
and projection identical. Keep text-only help when the useful information is a
report, file choice or state workflow that a geometric illustration would hide.
Record that decision in the coverage manifest and reconsider it when real
selection, topology or diagnostic feedback can explain the operation.

Capture actual SolidCraft states and previews. Do not morph two pictures, invent
intermediate geometry or copy media from another CAD product. If a capture uses
a programmatic command commit rather than a complete UI interaction, record that
in its source recipe and authoring notes; do not present it as a full UI tutorial.

## Animation pacing and replay

Interaction sequences use approximately **one deliberate click per second**.
Show the approach, the click and enough of the native response to read the action.
Keep the pace steady throughout; do not accelerate later steps to fit a long
workflow. Choose a smaller example or fewer representative inputs instead.

After completion, hold a clean result for **three full seconds**, then return to
the initial state and repeat while hovered. Keep the camera still during this
hold. Do not reverse a construction or removal operation to make a loop.

Construction sequences vary in length with their number of picks. The initial
Extrude concept sequence is a separate example: one second on its input, one
second of native model growth, then the three-second result hold. It contains no
mouse clicks and is not a recording of the command dialog.

The current authoring tool uses four 250 ms frames per Sketch input. The native
player overrides the encoded final-frame delay with its three-second hold; raw
source GIF timing is therefore not the tooltip's replay timing. Give standalone
review GIFs the same three-second hold so feedback assesses the intended pace.

Playback starts after the hover delay and restarts on a new hover. Motion is a
separate opt-in. Disabling it, running in the browser, or setting egui animation
time to zero uses the static poster. The current implementation does not read
the operating system's Reduce Motion setting. Keyboard-focused hints remain
future work.

Keep exports within the current player's bounds: at most 32 GIF frames, 10 seconds
of encoded duration, 4 MiB of encoded input and 24 MiB of decoded frame data.
Use 480 × 300 GIF frames, not full-size poster frames. Playback and bounded native
decoding are shared by menu and pinned-icon hints.

## Native construction and context details

- Three-point Arc picks **start, end, then through**. Three-point Arc Slot picks
  **start, through, then end**. Preserve that distinction in the animation.
- Midpoint Line keeps the chosen reference stationary while both endpoints grow
  symmetrically. Ordinary Line grows from its first endpoint. Verify endpoints,
  midpoint, equal half lengths and unchanged reference geometry.
- Spline examples must show how their fit or control points affect the curve.
  Keep the finishing action visible and use only the necessary control guides.
- Surface pictures distinguish uncapped sheets from solids.
- Assembly couplings use real driven poses; they do not imply that creating a
  coupling immediately moves the parts. As-Built preserves placement and retains
  the native joint glyph.
- Inspect uses actual measurement lines, combs, annotations and interference
  meshes. A data-only reply does not justify an invented overlay.
- Sheet Metal uses the actual flat viewer for flat patterns and perforations.
  Frame it with its own native pan and zoom controls.

## Capture and verification workflow

1. Use a separate app instance with its own settings and control directory, an
   explicit verified port and a saved, nonempty capture anchor. Check its process
   and documents before capture; do not assume a discovery port identifies the
   intended window when several apps are open. A failed isolation check stops
   dependent capture actions.
2. Create and verify a separate scratch document before issuing modeling
   commands. Preserve the original document, its modified indicator, visibility,
   camera, Home state, cursor settings and capture styles.
3. Build the small native example and declare the critical expected geometry.
   Verify solver and timeline health, counts, relevant lengths or radii, body
   measurements and joint values. Added explanatory references must not alter the
   intended result; driven dimensions must not move it.
4. Frame the whole example, including annotations and the largest preview. Hide
   unrelated noise and capture both themes with identical composition.
5. Use `ui.screenshot` for native tool previews, picks, selection and annotations,
   with a fixed viewport crop. Geometry-only `ui.render` does not include every
   selection overlay; do not paint a substitute and call it a viewport capture.
6. Assemble native frames without changing their geometry. Check encoded pacing
   and actual tooltip playback, including the final hold and static fallback.
7. Close only the verified scratch document and restore the original state.
   Never discard an edited user window to restart a capture. Keep intermediate
   screenshots, contact sheets and logs under ignored `target/`.

Use the [asset README](../assets/command-hints/README.md) for the existing capture
commands. Maintain one editable authority for each exported illustration. The
Sketch construction recipe owns both its GIFs and posters; do not retain a
competing still recipe for those same exports. Retain provenance and run the
repository's resource checks before integration.

## Review before integration

- Can a new user identify the defining input and result from the poster within
  two seconds? Does each animation step make the current input's role clear?
- Are neighboring variants distinguishable by their inputs or construction,
  rather than only by their titles?
- Does the midpoint/reference/anchoring principle remain visible in the poster
  when motion is disabled?
- Does every guide earn its place? Are redundant circles, origin markers and
  unrelated editing controls removed?
- Are axes, planes, points and native labels legible at 480 × 300 in both themes,
  with no overlap, occlusion or clipped annotations?
- Does the animation show the correct native input order at a steady pace and
  hold the result for three seconds before replay?
- Do list and grid menus, nested families and pinned pictograms show the correct
  command's media, fit the screen and preserve help and disabled reasons?
- Do opt-out/static behavior and active analysis Turn off hints remain correct?
- Do measurements and topology support what the picture claims, and does the
  current coverage manifest match the implementation?

Revise an asset that fails these checks before extending the same visual pattern
to more commands. Use the actual-size captures and source recipes for review;
source geometry alone cannot establish that the exported tooltip is clear.
