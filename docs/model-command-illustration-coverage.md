# Model command illustration coverage

The Model tab is covered: **65 illustrated commands** and **seven deliberate text-only hints**.
The contextual Two-point Rectangle illustration is retained separately from this Model inventory.
Conditional Finish Editing also remains text-only.

Images are original captures from SolidCraft, in both themes. Construction markers and
input selections come from its viewport; a small editorial ring identifies tiny construction points.
All solid recipes are checked for closedness, expected dimensions/volumes and clean feature status.

## Illustrated commands

### Sketch (4)

| Command | Source |
| --- | --- |
| `sketch.arc3d` | `assets/command-hints/source/sketch-menu.json` |
| `sketch.create` | `assets/command-hints/source/sketch-menu.json` |
| `sketch.line3d` | `assets/command-hints/source/sketch-menu.json` |
| `sketch.spline3d` | `assets/command-hints/source/sketch-menu.json` |

### Create (22)

| Command | Source |
| --- | --- |
| `plastic.boss` | `assets/command-hints/source/create-menu.json` |
| `plastic.lip` | `assets/command-hints/source/create-menu.json` |
| `plastic.rest` | `assets/command-hints/source/create-menu.json` |
| `plastic.snap_fit` | `assets/command-hints/source/create-menu.json` |
| `solid.boundary_fill` | `assets/command-hints/source/create-menu.json` |
| `solid.bounding_solid` | `assets/command-hints/source/create-menu.json` |
| `solid.box` | `assets/command-hints/source/create-menu.json` |
| `solid.coil` | `assets/command-hints/source/create-menu.json` |
| `solid.cone` | `assets/command-hints/source/create-menu.json` |
| `solid.cylinder` | `assets/command-hints/source/create-menu.json` |
| `solid.emboss` | `assets/command-hints/source/create-menu.json` |
| `solid.extrude` | `assets/command-hints/source/model.json` |
| `solid.hole` | `assets/command-hints/source/model.json` |
| `solid.loft` | `assets/command-hints/source/create-menu.json` |
| `solid.pipe` | `assets/command-hints/source/create-menu.json` |
| `solid.revolve` | `assets/command-hints/source/create-menu.json` |
| `solid.rib` | `assets/command-hints/source/create-menu.json` |
| `solid.sphere` | `assets/command-hints/source/create-menu.json` |
| `solid.sweep` | `assets/command-hints/source/create-menu.json` |
| `solid.thread` | `assets/command-hints/source/create-menu.json` |
| `solid.torus` | `assets/command-hints/source/create-menu.json` |
| `solid.web` | `assets/command-hints/source/create-menu.json` |

### Modify (13)

| Command | Source |
| --- | --- |
| `face.delete` | `assets/command-hints/source/modify-arrange-menu.json` |
| `sheet.convert` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.chamfer` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.combine` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.draft` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.fillet` | `assets/command-hints/source/model.json` |
| `solid.offset_face` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.press_pull` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.replace_face` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.scale` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.shell` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.split_body` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.split_face` | `assets/command-hints/source/modify-arrange-menu.json` |

### Arrange (6)

| Command | Source |
| --- | --- |
| `solid.align` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.mirror` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.move` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.pattern.circular` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.pattern.path` | `assets/command-hints/source/modify-arrange-menu.json` |
| `solid.pattern.rectangular` | `assets/command-hints/source/modify-arrange-menu.json` |

### Construct (20)

| Command | Source |
| --- | --- |
| `construct.axis.edge` | `assets/command-hints/source/construct-menu.json` |
| `construct.axis.normal_to_face` | `assets/command-hints/source/construct-menu.json` |
| `construct.axis.through_cylinder` | `assets/command-hints/source/construct-menu.json` |
| `construct.axis.two_planes` | `assets/command-hints/source/construct-menu.json` |
| `construct.axis.two_points` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.along_path` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.at_angle` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.axis_point` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.midplane` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.offset` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.perpendicular` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.tangent` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.three_points` | `assets/command-hints/source/construct-menu.json` |
| `construct.plane.two_edges` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.along_path` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.center` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.edge_plane` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.three_planes` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.two_edges` | `assets/command-hints/source/construct-menu.json` |
| `construct.point.vertex` | `assets/command-hints/source/construct-menu.json` |

## Text-only decisions

| Command | Reason |
| --- | --- |
| `config.configure` | Chooses named configuration values; model-only media would obscure the choice. |
| `config.table` | A configuration table workflow, best explained with text. |
| `parameters.change` | A parameter editor workflow; an isolated model picture would hide the relationship to named expressions. |
| `sketch.edit` | Opens an existing sketch; the text explains the selection and edit workflow. |
| `sketch.point3d` | The point icon and text explain placement; a larger picture needs distracting edit controls. |
| `solid.remove` | A direct removal action; its consequences belong in text and confirmation. |
| `timeline.delete` | History deletion and its dependants need text, not a generic disappearing body. |

`component.finish_edit` is a conditional Model action for linked-component editing; its
text explains leaving that editing context. It does not change model geometry.

## Maintenance

`every_covered_menu_command_has_an_explicit_illustration_decision` compares this decision
set with the real Model ribbon registry. Adding a Model command requires a picture or
an explicit text-only decision. Source and capture specifications are in
[the media README](../assets/command-hints/README.md).

Animated Extrude retains the agreed three-second pause on its result before replay.
Other commands use readable stills. The app decodes requested media lazily and retains
at most eight cached images or animations; visiting the entire menu does not retain them all.

Coverage for Sketch, Surface, Assemble, Inspect and Sheet Metal is recorded in [the combined coverage list](command-illustration-coverage.md).
