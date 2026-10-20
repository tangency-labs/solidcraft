# Viewport ground shadows

Choose **Ground shadow: Off, Ellipse or Projected** in **Preferences → Graphics**
or the viewport's **View settings**. Ellipse remains the default and the inexpensive
option. Projected follows the silhouette and spaces between visible bodies.
Both use the existing virtual plane slightly below the visible bodies' lowest point,
in the selected Z-up or Y-up frame. The plane does not clamp to the origin grid,
create selectable geometry or change the saved design.

The engine command is shared by UI and control-channel callers:

```json
{"command":"view.ground_shadow","params":{"mode":"projected"}}
```

`mode` accepts `off`, `ellipse` and `projected`. Existing `on: true`,
`on: false`, and empty/null toggle requests keep their behaviour and retain the
chosen style while switching visibility. The command changes display
preferences without a modelling undo step or camera movement. Graphics presets
leave the selected shadow alone. Cancel, Apply and Restore Defaults work normally.
Old saved `ground_shadow` booleans keep their meaning (on uses Ellipse, off stays
off); `preferences.ground_shadow_style` remembers `ellipse` or `projected` while
the existing visibility flag and `ui.set {"groundShadow":false}` remain compatible.
Ambient occlusion is controlled independently.

Projected is a contact-shadow approximation, not an area-light simulation. It
projects coverage onto the ground along the up axis and applies the same blur
width to every caster, regardless of its height. The Gaussian sigma is about 2.3%
of the coverage's longest XY span, so scaling the whole model keeps its appearance.
Camera zoom, rotation, viewport size and DPI do not change the world-space width.
Sub-texel holes may disappear in the fixed-resolution mask. Transparent faces use
their maximum coverage opacity, avoiding darker shadows merely from overlapping
triangles; stacked transparent parts are not physically simulated. Coverage follows
the viewport's displayed opacity, including its existing 5% minimum for transparent faces.

The renderer reuses the normal vertex/index buffers, including GPU-evaluated bodies
and live previews, with palette opacity and all placements of shared meshes.
The projection ignores camera culling and keeps its own small placement buffers.
No body triangles are copied into another CPU mesh. Three
256 × 256 RGBA8 images use 768 KiB; no shadow depth map is required. A separable
Gaussian with support ±16 texels uses 17 bilinear texture fetches per pass,
equivalent to the dense 33-tap discrete filter. There are no per-pixel exponential
evaluations or height bands. Projection/blur are cached across camera-only motion;
geometry, visibility, preview, clipping, up-axis or shadow-setting changes refresh
them. Disabled/ellipse mode releases these resources. A device with an insufficient
texture limit uses Ellipse; the model size is not capped by a shadow triangle limit.

Projecting many triangles and shading the ground still have a GPU cost, especially
on software adapters. Ellipse remains available for those cases. Performance
comparisons must distinguish first use, model updates and cached camera motion,
and report adapter, viewport, MSAA and supersampling settings. Use `ui.screenshot`
for review images: the existing model-snapshot path does not apply viewport effects.
