# What works

The full feature summary, moved out of the README. The commands by area are in [features.md](features.md); status and plans are in [ROADMAP](roadmap-detail.md).

**Status: early but usable for simple parts.** 75 of 76 public reference parts
match their analytic or FreeCAD-computed volume, area, centre of mass and bounding box
([oracle.md](oracle.md)); the other is a tracked bug.
These checks do not establish complete geometric or workflow equivalence. The commands SolidCraft has
are listed by area in [features.md](features.md); status and plans are in [ROADMAP.md](roadmap-detail.md).
Open work and known bugs are in [GitHub issues](https://github.com/tangency-labs/solidcraft/issues),
and help is welcome: see [CONTRIBUTING.md](../CONTRIBUTING.md).


- **Current-view DXF export** from File → Export Current View as DXF, with hidden edges
  omitted or dashed, shown body placements and the current camera. See [view DXF](view-dxf.md).

- **Sketches** on origin planes, offset planes or planar faces: lines, rectangles, circles,
  arcs, polygons, slots, ellipses, fit/control splines, conics and text; construction geometry,
  projection, trimming, fillets and chamfers, including corners at the origin.
  New face attachments retain supported construction identities and chosen occurrences;
  missing sources expose an explicit plane repair workflow. See [face attachments](face-attachments.md).
  Explicitly closed fit splines and closed associative offsets use periodic C2 interpolation.
  Supported convex line-and-arc offsets retain their identity when an inset consumes corner
  arcs, and linked extrusions follow the current boundary. Breaking the association freezes
  the boundary and keeps existing extrusions usable.
  Curve drawing and picking adapt together to zoom and display scale, including small circles
  at Fit and sketches in placed components.
- **Constraints and dimensions** solved by our own solver. Points the constraints fix relative
  to known geometry are placed exactly by construction (loci intersections and rigid-cluster
  decomposition, after Owen 1991 and Fudos–Hoffmann 1997); Levenberg–Marquardt solves the rest,
  and the rank of its Jacobian gives the remaining degrees of freedom. Constraints: coincident,
  point on curve, horizontal/vertical, parallel, perpendicular, tangent, curvature (G2), equal,
  concentric, collinear, midpoint, symmetry, fix; linear, aligned, horizontal, vertical,
  point-to-line, line-to-circle, point-to-circle, circle-to-circle, radius, diameter, angle, arc sweep, arc length and slot length dimensions driven
  by parameters. Over-constraining edits are rejected; fully constrained geometry turns black.
  Dragging solves the sketch every frame and rebuilds the model once, when the drag ends.
  Select a line or its two endpoints, type its dimension and press Enter to commit one undo
  step; with Sketch Dimension it's D, click, type, Enter. Fit keeps sketch geometry clear of the palette and navigation controls.
- **Profiles** found automatically, including regions split by lines and holes inside outlines.
  Newly captured complete adaptive boundaries retain their outer and hole source identities
  when spline span counts change. Partial or ambiguous boundaries do not qualify.
- **Features** on a parametric timeline: extrude, revolve, sweep, loft, fillet/chamfer,
  shell, draft, holes, threads, patterns, mirror, split, combine, move and construction geometry.
  The timeline rebuilds incrementally; features can be edited, suppressed, renamed, reordered
  and deleted. Supported geometry and current limitations are described in [ROADMAP.md](roadmap-detail.md).
  New revolves preserve symmetric and two-sided angular placement across reversed axes;
  older designs retain their saved convention when reopened or edited.
- **Parameters and configurations** with unit-aware expressions (`2 * width + 5 mm`,
  `angle / 2`), ordered rules, feature suppression and per-row body materials/appearances.
- **Assemblies** with nested components and occurrences, joints, motion studies, contact sets,
  interference and material-aware mass properties. Recorded body transfers preserve modelling
  history and world placement; 3D sketch wires retain their coordinate frames through edits.
- **Sheet metal** with flanges, hems, fold/unfold/refold, material cutouts, flat patterns,
  retained shallow prismatic pockets, exact line/arc/circle DXF outlines and bend tables;
  plastic enclosure features and rules. Pocket depth and side survive unfold/refold and appear
  separately from through-hole outlines in the flat preview and DXF.
- **Files**: design containers (`.sc`; `.solidcraft` still opens) with lossless binary mesh payloads;
  plain JSON remains supported for opening, Save As and export. Desktop Open can show a saved
  provisional picture from warm geometry caches while rebuilding before editing; caches never
  replace the parametric design. Export to STEP (AP242 with body names, colours and
  components as assemblies; revolved faces as native planes, cylinders, cones, spheres and tori),
  IGES 5.3 (manifold solid B-reps with names and colours), 3MF, STL (binary/ASCII) and OBJ, and
  assemblies with joints as URDF for robot simulators (urdf.md);
  3MF, STL, OBJ and glTF 2.0 (`.glb`/`.gltf`, its node tree as components and its materials as
  appearances) import as mesh bodies (they render, measure, move and export; solid features
  need B-rep bodies). The `mesh.convert` command reconstructs supported closed constant-section parts
  as parametric solids with planar and circular cylindrical faces;
  STEP import (AP203/AP214/AP242 solids, assemblies, units, names, colours) and IGES import
  (manifold solid B-reps, or trimmed and bounded surfaces sewn into solids) as a base feature
  that later features build on — open a `.step`/`.stp`/`.igs`/`.iges` file, insert one into a
  design, or drop it on the window ([docs/step-import.md](step-import.md)); FreeCAD
  `.FCStd` and Rhino `.3dm` solids import the same way through
  [cadmpeg](https://github.com/cadmpeg/cadmpeg) in the desktop app, the browser app and the CLI.
- **Render:** opaque surfaces can use metal fraction, roughness and dielectric IOR in the
  integrated product viewport and CPU preview PNGs. Existing simple colors remain supported.
  Fixed direct lighting is a preview; HDRI, refraction and texture maps are not implemented.
  See [opaque surfaces](render-opaque-pbr.md).
- **Display meshes** from our own tessellation planner: each face becomes a plan of (u, v)
  points and triangles that shares its edge nodes with its neighbours, so the mesh is watertight.
  Facets follow the chord tolerance, the facet angle and a longest side, with cells at most 2:1
  along straight directions. Box-bounded and four-cornered faces mesh as structured grids.
  Trimmed faces are cut into four-sided patches: squares around holes, butterflies in round
  faces, motorcycle traces from concave corners (after Lyon et al. 2019). Each patch then meshes
  as a Coons grid. Intersection edges are evaluated by Newton on both surfaces. A compute shader
  evaluates the plans for the whole scene in one dispatch, with a CPU fallback when the GPU
  can't build it. Planning and evaluating the 78 reference bodies takes about 65 ms, 50–65×
  faster than the previous mesher.
- **Desktop app** (egui + wgpu): toolbar with workspace tabs, browser, timeline, 3D viewport with
  axis view and navigation bar, sketch tools, feature dialogs, command palette, and a JSON-lines
  control channel so agents and tests can drive it.
  Feature errors and Help can open an editable **Create Issue** draft with relevant context
  and an optional reviewed screenshot; opening GitHub does not submit the report.
- **Headless CLI** `solidcraft-cli`: run command scripts, measure (volume, area, centre of mass,
  face/edge/vertex counts), export, render PNG snapshots and replay reference recipes.
  A snapshot can draw a body see-through and another solid in a colour of its own, and cut
  the model at a plane, so a picture shows the body a cut would remove inside the material
  it meets.
- **MCP server** `solidcraft-cli mcp`: AI agents model, measure and look at parts headless or
  in the running app (see below).
