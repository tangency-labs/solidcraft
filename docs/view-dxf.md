# Export the current view as DXF

Choose **File → Export Current View as DXF…**, choose whether to omit hidden edges or
export them as dashed lines, set **Curve tolerance**, then choose **Export DXF…** and a
destination. Both settings are remembered. The Japanese menu is
**ファイル → 現在の表示をDXFでエクスポート…**.

On desktop, the export runs on a background worker while the window continues repainting
and responding to its title-bar controls. A progress card shows mesh preparation, surface
preparation, projection, hidden-line removal and writing, with a count and progress bar for
the current stage. **Cancel** stops at the next mesh or traversal checkpoint; a single
meshing call or sort may need to finish first. The model waits during the export and remains
unchanged on success, failure and cancellation. Individual control-channel
`file.export_view_dxf` commands use the same worker.
The browser's export still runs synchronously on its single thread.

An export replaces its destination atomically only after the complete drawing is ready.
Cancellation that takes effect before the final write preserves any previous file. Once
that atomic write starts, it finishes; a late Cancel reports the completed save.

The drawing uses the camera when Export is pressed, including roll, the world's up axis
and perspective. Only shown bodies are exported, at their current component/occurrence
placements. Hidden bodies neither draw nor hide other bodies. The complete projected
geometry is exported, including geometry beyond the viewport's edges. Grid, dimensions,
constraints, selection highlights and other overlays are excluded. Turn off section analysis
before exporting.

Sketches the viewport shows are exported too, so a design with only a sketch can be
exported. Only profile curves are included: drawn curves, text outlines and drawn 3D-sketch
curves; construction geometry, centerlines, projected references and points are not. Sketch
curves go on their own `SKETCH` layer, each curve as one `LINE` or `LWPOLYLINE` (arcs,
circles and splines as straight segments at the curve tolerance). Finished sketches are
hidden behind bodies like body edges (dashed on `HIDDEN` when hidden edges are exported); the
sketch being edited is drawn on top, as in the viewport. The export is refused only when
there is neither a shown body nor a shown sketch curve.

Coordinates are 2D, Z=0, in millimetres. Orthographic views retain 1:1 dimensions in the
view plane. Perspective views retain foreshortening, projected onto the plane through the
camera target; they are intended for illustrations rather than dimensioned fabrication.
Curves and curved silhouettes are approximated with straight segments at a default
model-space chord tolerance of 0.01 mm. The setting accepts 0.0001–1 mm; smaller values
produce finer curves and generally need more time and memory. This is not an exact analytic
curve export or a tolerance guarantee for the original imported surfaces.

The AutoCAD 2000 ASCII DXF contains `VISIBLE` (continuous), `SKETCH` (when a sketch is
shown) and, when requested, `HIDDEN` (2 mm dashes / 1 mm gaps) layers. Hidden curves use connected `LWPOLYLINE` entities with
continuous linetype generation so the pattern does not restart on each chord. The pattern
is embedded in the file. The polyline flags follow the
[Autodesk DXF reference](https://help.autodesk.com/cloudhelp/2017/ENU/AutoCAD-DXF/files/GUID-748FC305-F3F2-4F74-825A-61F04D757A50.htm).

Edges are split where another surface occludes them, including partial occlusion by another
body. Hidden edges are never included in omit mode. Triangle diagonals and smooth seams
are excluded; curved-body silhouettes are included. Very complex drawings fail with an
error rather than returning an incomplete export. If geometry crosses the perspective
camera's eye plane, zoom out before exporting.

Repeated component placements share their export mesh and triangle hierarchy. Each placement
keeps its own edge choices, position and orientation; no bodies are fused or omitted to reduce
the workload. A hierarchy over placed parts rejects unrelated occluders before searching a
part's triangles. Already hidden intervals are skipped, and a fully hidden line stops searching.
Desktop mesh preparation and independent line queries use at most eight computation workers;
the browser runs serially. Export memory, candidate lines, output lines and total occlusion
work remain bounded, and exceeding a budget returns an error without writing a partial drawing.

Programmatic callers use `file.export_view_dxf` with `camera: Camera`, `hidden: "omit"` or
`"dashed"`, optional `tolerance` (0.0001–1 mm) and optional `path`. Without `path`, the
command returns the DXF text. The command does not modify the design or its undo history.
The result also includes `stats`: placed body and shared mesh counts, source triangle count,
retained mesh bytes, worker limit, candidate segment count, occlusion work, and preparation,
projection and occlusion times in milliseconds. These timings exclude STEP import and DXF
serialization; the worker limit is a maximum, not a claim that every worker was busy.
