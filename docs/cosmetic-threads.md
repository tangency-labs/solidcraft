# Cosmetic thread display

Cosmetic markings are **off by default**, including when preferences saved by an older
version have no setting for them. Turn on **Preferences > Graphics > Cosmetic thread markings**
to display them. The switch applies immediately, without moving the camera or rebuilding
the design, and is saved in application preferences. Cancel, Apply and Restore Defaults
follow the other Graphics settings. It is not a document property or a geometry feature.

Tapped Hole and non-modeled Thread features now mark the threaded cylindrical wall with
original, slightly tilted bands. Plain holes remain smooth. Both internal and external
threads use the evaluated thread metadata, including partial length and occurrence placement.
This is schematic hatching, not a manufactured thread profile or helical solid.

The strokes lie on the same triangles as the displayed surface, follow display refinement,
use the existing edge color and depth-tested line renderer, and are clipped by section planes.
Selection and hover still use the original faces and their normal highlight passes. No
geometry, volume, physical material, design format, or mesh-export data changes.

The shaded desktop/web viewport and its GPU/CPU model snapshots share the setting and the
same marking generator. Ordinary headless scene helpers leave markings off; callers can
explicitly request them through `scene_with_thread_markings`. The marking needs no texture
file, compute shader or new graphics capability.
The wireframe viewport omits the marking. Transparent or faded bodies omit it because the
ordinary depth-tested line pass cannot represent their visibility correctly.

Pitch determines spacing until a bounded schematic density is reached (up to 32 bands).
Large threaded assemblies reduce the density target, down to four bands, before the hard
scene limits apply so repeated parts share the marking budget. Short regions may show fewer
bands. The count includes hidden thread metadata, so hiding parts does not increase density.
Strokes have a fixed subpixel output width, like the existing edge renderer; very small holes
may still require zooming in. A face must remain an analytic cylinder, coaxial with the stored
thread and within its diameter range. Removed/replaced or unsupported surfaces are not marked.
One shared scene budget bounds successful and unsuccessful work (500,000 inspection units,
65,536 strokes; 32,768 per body). Lazy face classification also charges face traversal. The
viewport caches the scene across ordinary camera moves; LOD refinement rebuilds it with a fresh
bounded budget. Very large models can therefore omit later markings. This is not a thread inspection or dimension
tool. There is no separate major-diameter entry circle. Physically modeled threads continue
to display their actual geometry and do not receive this cosmetic hatch.

The evaluated metadata records the target cylinder's actual radius and placed body name.
This prevents marking a later enlarged bore and keeps copied/rotated occurrences associated
with their own walls. It adds no saved feature field. The schematic band's orientation can
change after arbitrary rotations because its tilt seed uses world axes.

The marking is an original Rust construction using SolidCraft's existing line and material
conventions. It introduces no artwork, texture assets or dependencies.
