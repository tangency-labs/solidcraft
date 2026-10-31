# Live link to Blender and other programs

A live link sends the design you are working on to another program and keeps it up to date. The
other program gets every visible body as a mesh it can work with: welded and closed, one polygon
for each flat face where it can be, quads on cylinders, cones, fillets and other curved faces,
and every polygon tagged with the face it came from. Each time the design changes (an edit, a
parameter, undo or redo, a body shown or hidden), the bodies that changed are sent again.

The other program only reads. It never changes the design, so you keep modelling in SolidCraft and
texture, render or prepare the part for a game in the other program.

## Connecting Blender

A separate SolidCraft add-on for Blender (4.2 or later) receives the meshes.

1. Start SolidCraft and open your design. The connection uses the control channel, which is on
   unless you turned off **Preferences → Assistant → Let AI agents connect**.
2. In Blender, open the sidebar in the 3D viewport (**N**), choose the **SolidCraft** tab and
   click **Connect**. The add-on finds the running SolidCraft by itself.
3. Turn on **Live** to follow every change, or click **Refresh** to fetch the design once.

Each body becomes an object, in a collection for its component. A part placed several times
shares one mesh. Corner normals come from the surfaces themselves, so curved faces look smooth
however few polygons they have. The edges where faces meet at an angle are marked sharp, and every
face boundary is marked as a seam for unwrapping.

## How fine the meshes are

The add-on can ask for coarser or finer meshes, for all bodies or for the ones you select:

- **Quality** follows SolidCraft's display qualities (coarse, normal, fine, ultra).
- **Chord** is how far a facet may stand off the true surface, in millimetres or as a share of
  the body's size; **angle** is how far one facet of a curved face may turn.
- **Topology** chooses n-gons and quads (the default), quads only, or triangles only.

## When it doesn't connect

- SolidCraft must be running on the same computer, with the control channel on (above).
- With several SolidCraft windows open, the add-on connects to the most recently started one.
- A body that SolidCraft cannot mesh is listed with the reason, and the others still arrive.

For developers: the connection is the [control channel](https://github.com/tangency-labs/solidcraft/blob/main/docs/control-protocol.md#live-link),
and the meshes are described in [Live-link meshes](https://github.com/tangency-labs/solidcraft/blob/main/docs/formats/live-mesh.md).
