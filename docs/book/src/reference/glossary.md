# Glossary

This page sets SolidCraft's **ubiquitous language**: one agreed term for each idea, defined once
and used the same way in labels, command ids, parameter keys, the design file, the docs, the MCP
tools and every translation.

Two rules apply:

- **One idea, one term.** When this page names a term for an idea, use that term, not a
  synonym.
- **One term, one idea.** A word that means two things gets two qualified terms (for example
  *construction entity* and *construction plane*), and each meaning gets its own entry in the
  message catalogues.

A pull request that adds a new idea (a command, a parameter, a file key, a panel) adds its term
here. A pull request that renames a label, id or key to match this page keeps the old id or key
working as an alias and lists it in `docs/compatibility.md`.

## How an entry reads

Each entry gives the term and a **definition**: one short sentence in plain technical English
that could replace the term in a sentence. It names no product and doesn't use the term to define
itself, in the way ISO 704 and ISO 10241-1 describe for terminology. A note follows, with the
context:

- **In SolidCraft:** the command ids, parameter keys and design-file keys that use the term.
- **Don't use:** the other words for the same idea.
- **Elsewhere:** general names that people coming from other CAD software may know.
- **For translators:** what the term must not be confused with.

Some of the ids and labels below still use a word this page says not to use. They change in
later pull requests, one topic at a time, each pointing at its entry here.

## Terms

### appearance

How a body, face or component looks on screen and in renders.

An appearance sets colour and surface finish only. It has no effect on mass or strength. Compare
*material*.

- **In SolidCraft:** `appearance.assign`, `appearance.library`, `appearance.list`; the
  `appearances` design-file key.
- **Don't use:** look, colour (for the whole appearance), render material.
- **Elsewhere:** appearance, visual material, colour.
- **For translators:** not the physical material.

### bill of materials

The list of the components placed in a design or in one component, with the quantity of each.

It lists every part at any depth, or only the direct children. Each component's *bill of
materials type* decides whether it has a row and whether what it holds is counted.

- **In SolidCraft:** `assembly.bom` ("Bill of Materials") and its window on the INSPECT panel;
  each row's `item`, `part_number` and `count`; a component's `metadata` and `properties`
  design-file keys, which `component.metadata.set` changes.
- **Don't use:** BOM in labels; parts list (for the bill of materials itself).
- **Elsewhere:** bill of materials, BOM, parts list.
- **For translators:** not a parts list on a drawing, which is one table that shows it.

### bill of materials type

The rule that decides whether a component has a row in a bill of materials, and whether what it
holds is counted there.

There are five:

- *normal*, something you make: a row; each occurrence adds one to its quantity.
- *phantom*, something just to group: no row; what it holds counts where it is placed.
- *reference*, something to use as a reference: no row, and nothing inside it counts.
- *purchased*, something you buy: one row; nothing inside it is counted or listed.
- *inseparable*, something made as one piece (welded, glued, riveted and so on): one row; of
  what it holds, only the purchased components are listed, and only in a list of every part.

- **In SolidCraft:** the `bom_type` part property, which `component.metadata.set` changes and
  `component.metadata` reads; the `bom_type` of each `assembly.bom` row.
- **Don't use:** BOM structure, item type, kind; made (for *normal*: it reads as "already
  made").
- **Elsewhere:** BOM structure, BOM type.
- **For translators:** *normal* here is not the sketch line type, a direction or the display
  quality (see *Words with more than one meaning*); *reference* is not a construction plane;
  *phantom* is not a hidden or see-through display.

### body

One connected solid or surface shape that features create and change.

A design can hold many bodies. A component groups bodies; an occurrence places them.

- **In SolidCraft:** `body.rename`; the `bodies` browser folder; `body_name` parameters.
- **Don't use:** part, solid (for a surface body too), object.
- **Elsewhere:** body, solid body, surface body.
- **For translators:** not a component and not a design.

### browser

The panel that lists the design's components, bodies, sketches, construction planes and joints as
a tree.

- **In SolidCraft:** `browser.group`, `browser.move`, `browser.order`, `browser.visibility`, and the
  other `browser.*` commands.
- **Don't use:** model tree, scene browser, parts browser.
- **Elsewhere:** browser, model tree, feature tree, design tree.
- **For translators:** not the web browser. A language with its own CAD word for the tree may use
  it (in French, *arborescence*), so that "browser" and "web browser" don't share a word. See
  *web browser*.

### component

A named container of bodies, sketches, construction geometry and other components that can be
placed one or more times.

Each placement of a component is an *occurrence*. Editing a component changes every occurrence
of it.

- **In SolidCraft:** `component.create`, `component.activate`, and the other `component.*`
  commands; the `components` design-file key.
- **Don't use:** part, assembly, sub-assembly (for the component itself).
- **Elsewhere:** component, part, sub-assembly.
- **For translators:** not an occurrence.

### constraint

A rule that fixes the relation between sketch entities, such as coincident, parallel or tangent.

A constraint with a value is a *dimension*.

- **In SolidCraft:** `sketch.constraint.coincident`, `sketch.constraint.parallel`, and the other
  `sketch.constraint.*` commands; the `constraint` parameter key; the sketch's `constraints`
  design-file key.
- **Don't use:** relation, condition.
- **Elsewhere:** constraint, geometric constraint, relation.
- **For translators:** a joint between occurrences is not a constraint.

### construction entity

A sketch entity that guides the drawing but isn't part of any profile.

A construction entity can be a line, an arc, a circle or any other curve, and it can carry
constraints and dimensions like any sketch entity.

- **In SolidCraft:** the `construction` parameter of the sketch drawing commands; the
  `construction` field of a sketch curve; the `ui.construction` catalogue entry.
- **Don't use:** "construction" on its own where it could mean a *construction plane*.
- **Elsewhere:** construction line, construction geometry, reference line.
- **For translators:** not a construction plane, axis or point. The browser's Construction folder
  and this line type still share the `Construction` catalogue entry, so German, Korean and Turkish
  show the folder as "construction line".

### construction plane, construction axis, construction point

A plane, axis or point that a feature creates to place other features, and that is not part of
any body.

They are listed together in the browser's *Construction* folder.

- **In SolidCraft:** `construct.plane.offset`, `construct.axis.two_points`,
  `construct.point.vertex`, and the other `construct.*` commands; the CONSTRUCT toolbar panel.
- **Don't use:** "construction" on its own where it could mean a *construction entity*; work plane.
- **Elsewhere:** construction plane, reference plane, datum plane (and axis, point).
- **For translators:** not a sketch's construction entity.

### curve

A sketch entity that is not a point: a line, an arc, a circle, an ellipse, a spline or a conic.

- **In SolidCraft:** the sketch's `curves` design-file key; curve ids in parameters such as `path`.
- **Don't use:** geometry, edge (for a sketch curve).
- **Elsewhere:** curve, sketch curve, segment.
- **For translators:** not an edge of a body.

### design

The model that is open in SolidCraft: its timeline, components, bodies, sketches, parameters and
settings.

A design is saved as a *design file*.

- **In SolidCraft:** `file.new` ("New Design"), `document.units` ("Change Design Units"),
  `document.metadata` ("Design Properties"), `doc.open`, `doc.recover`.
- **Don't use:** document, doc, project, file (for the model itself), part.
- **Elsewhere:** design, document, model, part file, assembly file.
- **For translators:** not the *Project* design property and not the sketch Project command.

### design file

A file in which SolidCraft saves one design, with the `.sc` extension (`.solidcraft` in earlier releases; those files still open).

- **In SolidCraft:** `file.save`, `file.save_as`, `doc.open`; the `solidcraft/N` format tag.
- **Don't use:** document, project file.
- **Elsewhere:** design file, model file.
- **For translators:** "file" is only for what is on disk: opening, saving, importing and
  exporting.

### dimension

A constraint that sets a size, such as a length, a distance, an angle or a radius, by its value.

A dimension can drive the sketch, or be driven by it and only show the value.

- **In SolidCraft:** `sketch.dimension`, `sketch.dimension_text`, `sketch.toggle_driven`; a
  dimension's id is a constraint id.
- **Don't use:** measurement (for a dimension), size.
- **Elsewhere:** dimension, dimensional constraint, driving dimension.
- **For translators:** not a drawing dimension and not a measured value.

### equation curve

A sketch curve whose shape comes from formulas over a range of one variable.

It is a fit point spline through fixed points that the formulas place, and they place them again
when the design's parameters change. Compare *spline*, whose points are placed by hand.

- **In SolidCraft:** `sketch.equation_curve`, `sketch.equation_curve.edit`; the `equations`
  design-file key of a sketch; `variable_unit` says whether the variable is a plain number or a
  length like its range.
- **Don't use:** formula curve, function curve, parametric curve (for the command).
- **Elsewhere:** equation-driven curve, curve from equation, function plot.
- **For translators:** "equation" is a mathematical formula, not a balance or an equality constraint.

### feature

One step in the timeline that creates or changes geometry, such as an extrude, a fillet or a
construction plane.

- **In SolidCraft:** `timeline.edit`, `timeline.suppress`, `timeline.reorder`, and the other
  `timeline.*` commands; the `features` design-file key.
- **Don't use:** operation, step (in labels); feature (for something SolidCraft can do in
  general: say *tool* or *capability*).
- **Elsewhere:** feature, operation.
- **For translators:** French uses *fonction* for a timeline feature, so "AI features" had to
  become *fonctionnalités IA*. Translate "feature" only in the timeline sense.

### folder

One of the browser's fixed sections of a component: Bodies, Sketches, Construction, Joints.

A folder is part of every component and can't be renamed or removed. A container that someone
makes is a *group*.

- **In SolidCraft:** the `folder` parameter of `browser.group`, `browser.order` and
  `browser.visibility` (`bodies`, `sketches`, `construction`).
- **Don't use:** folder for a group.
- **Elsewhere:** folder, node.
- **For translators:** not a group and not a folder on disk.

### group

A named container that someone makes, in the browser or in the timeline, to keep items together.

A group changes only how items are listed. It has no effect on the geometry or the order in which
features are computed.

- **In SolidCraft:** `browser.group`, `browser.ungroup`, `browser.rename_group`; `timeline.group`,
  `timeline.ungroup`, `timeline.rename_group`, `timeline.move_to_group`; the `group` parameter key;
  the `groups` parameter of `timeline.delete` and `timeline.suppress` (a group with all its features);
  the `browser_groups` and `timeline_groups` design-file keys.
- **Don't use:** feature folder, folder (for a group).
- **Elsewhere:** folder, group.
- **For translators:** not a rigid group (a joint) and not a fixed browser *folder*.

### live link

A connection through which another program follows the open design and receives its bodies as
polygon meshes whenever the design changes.

The receiving program (a modelling or rendering program) only reads: it never changes the design.
It may ask for a body to be tessellated more or less finely. Compare a *design link*, which
brings another design file's bodies into this one.

- **In SolidCraft:** `live.meshes`; the `control.subscribe` method and its `design.changed`
  events; the `live_meshes` entry of `control.capabilities`.
- **Don't use:** bridge, sync (for the connection itself), link (alone).
- **Elsewhere:** live link, bridge, live connection.
- **For translators:** not a design link or a component link.

### material

The physical substance of a body, which sets its density and mechanical properties.

A material sets mass, centre of gravity and strength values. How the body looks is its
*appearance*.

- **In SolidCraft:** `material.assign` ("Physical Material"), `material.list`, `material.create`,
  and the other `material.*` commands; the `materials` and `user_materials` design-file keys.
- **Don't use:** material for a render or visual material.
- **Elsewhere:** material, physical material.
- **For translators:** not an appearance.

### moment of inertia

How a body's mass resists turning about an axis: the mass times the square of its distance from
that axis, summed over the body.

SolidCraft reports the whole inertia tensor about the centre of mass in world axes, with its
*principal moments* and *principal axes*: the three axes with no products of inertia between
them. The *first moments* are the mass times the centre of mass.

- **In SolidCraft:** `inertia_g_mm2`, `principal_moments_g_mm2`, `principal_axes`,
  `first_moment_g_mm` and `about` in `component.properties` and `occurrence.properties`; the
  Properties window's Moment of inertia section.
- **Don't use:** rotational inertia, second moment of mass, mass moment.
- **Elsewhere:** moment of inertia, inertia tensor, mass moment of inertia.
- **For translators:** not the second moment of area of a cross-section, a different quantity.

### occurrence

One placed copy of a component inside its parent component.

A component can have several occurrences. Moving an occurrence moves only that copy; editing the
component changes every copy. The browser names occurrences with a number, such as `Bolt M20:3`.

- **In SolidCraft:** `occurrence.copy`, `occurrence.move`, `occurrence.drag`, `occurrence.ground`,
  `occurrence.properties`; the `occurrences` design-file key; the `path` of occurrence ids.
- **Don't use:** instance, placement.
- **Elsewhere:** occurrence, instance, component instance.
- **For translators:** not a *pattern instance*.

### part number

A design property that holds the identifier an organisation gives a design.

- **In SolidCraft:** the `part_number` design property, shown in Design Properties and set with
  `document.metadata.set`.
- **Don't use:** part number for a standard part's size.
- **For translators:** see *Words with more than one meaning*, "part".

### pattern instance

One copy that a pattern makes of its features, bodies or sketch entities.

The first instance is the original.

- **In SolidCraft:** `solid.pattern.rectangular`, `solid.pattern.circular`, `solid.pattern.path`,
  `sketch.pattern.rectangular`, `sketch.pattern.circular`; the `count` and `spacing` parameters
  ("between instances").
- **Don't use:** instance for an occurrence; element, item.
- **Elsewhere:** instance, element, occurrence.
- **For translators:** not an occurrence of a component.

### personal sheet metal rule

A sheet metal rule kept in the user's preferences instead of a design, ready to add to any design.

Picking one in Sheet Metal Rules fills the rule manager, and its OK copies the rule into the
design, which then keeps a copy of its own. It is an app setting, not part of a design.

- **In SolidCraft:** Sheet Metal Rules › Personal rules (`sheet.manage_rules`); the
  `personal_sheet_rules` key of the stored preferences.
- **Don't use:** preset, template, library rule.
- **Elsewhere:** rule library, sheet metal style library.
- **For translators:** "personal" as in the user's own, not private or confidential.

### print face

The flat face of a body that lies on the print bed when the body is sent to a slicer.

A body has one print face at most. The export turns a copy of the body so this face points down
at Z = 0, with the centre of mass over the origin; the design itself does not move.

- **In SolidCraft:** `body.print_face`; the `print_faces` of a design; the `print_faces` and
  `warnings` of a `file.send_to_slicer` reply.
- **Don't use:** bottom face, base face (both say where a face is now, not where it is printed).
- **Elsewhere:** place on face, lay flat, bed face.
- **For translators:** the face the part stands on while it prints, not the printer's bed.

### reference edge

A straight edge of a flat face that a Single hole's centre or a cylinder's base centre keeps a
set distance from, measured in that face's plane.

Up to two reference edges place one hole or cylinder. Each distance is a parameter, and the
feature follows its edges when the part changes.

- **In SolidCraft:** the `references` parameter of `solid.hole` and `solid.cylinder` (each
  `{edge, distance}`); the windows' Reference 1, Reference 2 and Distance 1, Distance 2 rows; the
  "Reference distance" input (`ui.reference_1`, `ui.reference_2`, `ui.reference_distance`).
- **Don't use:** datum edge, anchor edge.
- **Elsewhere:** reference edge, edge offset, distance from edge.
- **For translators:** an edge of a body, not a construction axis or a reference plane; the
  distance is a length, not a reference value for light or material.

### rollback marker

The marker in the timeline after which no feature is computed.

Moving the rollback marker back shows the design as it was at that step. Features after the marker
are kept, not deleted.

- **In SolidCraft:** `timeline.rollback`, `timeline.roll_to`; the `marker` design-file key.
- **Don't use:** timeline marker, history marker, rollback bar.
- **Elsewhere:** rollback bar, rollback marker, end-of-part marker.
- **For translators:** "history" is the undo history, not the timeline. French had three names
  for this marker while English did.

### settings profile

A named set of input and interface preferences that can be switched, exported and imported as
one.

A settings profile holds the Interface, Navigation and Sketch preferences, the interface
language, keyboard shortcuts, the toolbar and the S box favourites. Settings that belong to one
computer (the 3D mouse, graphics, units) stay out. It is an app setting, not part of a design.

- **In SolidCraft:** `settings.profile.list`, `settings.profile.save`, `settings.profile.apply`,
  `settings.profile.rename`, `settings.profile.delete`, `settings.profile.export`,
  `settings.profile.import`; the `settings_profiles` key of the stored preferences; the
  `solidcraft-profile/1` file format.
- **Don't use:** preset, configuration, workspace (for the set of settings).
- **Elsewhere:** settings profile, user profile, preferences set.
- **For translators:** not a sketch profile and not a frame profile (see *Words with more than
  one meaning*, "Profile"), and not a design configuration.

### sketch

A set of points and curves, with their constraints, on one plane, that features use as profiles,
paths and guides.

- **In SolidCraft:** `sketch.create`, `sketch.finish`, and the other `sketch.*` commands; the
  `sketches` browser folder.
- **Don't use:** drawing, profile (for the whole sketch).
- **Elsewhere:** sketch, 2D sketch.
- **For translators:** not a drawing (a sheet with views and dimensions).

### sketch entity

A point or a curve in a sketch.

- **In SolidCraft:** the `entities` parameter of the sketch commands (it takes point and curve
  ids); the sketch's `points` and `curves` design-file keys.
- **Don't use:** geometry (for sketch content), element, object.
- **Elsewhere:** sketch entity, sketch geometry, sketch element.
- **For translators:** "entity" names both points and curves.

### standard part

A ready-made component, such as a screw, nut, washer, pin or bearing, that SolidCraft builds from
a standard size.

- **In SolidCraft:** `parts.library` ("Standard Parts"), `parts.insert`, `parts.fastener`,
  `parts.resize`; the `parts` design-file key.
- **Don't use:** library part, part (on its own), content.
- **Elsewhere:** standard part, library part, fastener, catalogue part.
- **For translators:** see *Words with more than one meaning*, "part".

### timeline

The ordered list of a design's features, which SolidCraft computes from first to last.

- **In SolidCraft:** the `timeline.*` commands; the `features` design-file key, in timeline
  order.
- **Don't use:** feature history, history (for the timeline).
- **Elsewhere:** timeline, history, feature tree.
- **For translators:** not the undo history.

### view angles

The yaw, pitch and roll, in degrees, that give the direction a view looks along and how its
picture is turned.

Yaw is the heading of the line of sight about the up axis (Front view 0°, Left 90°, Right −90°,
Back 180°); pitch is how far the eye is above the ground plane (Top view 90°, Bottom −90°); a
positive roll turns the picture clockwise. They are the camera's own angles, which `view.apply`
and the MCP camera take in radians.

- **In SolidCraft:** `view.angle_readout` and the View angles readout under the orientation
  gizmo; the `yaw`, `pitch` and `roll` of `view.apply`'s `camera`.
- **Don't use:** camera angles, view rotation.
- **Elsewhere:** view orientation, azimuth and elevation.
- **For translators:** the view's pitch is a tilt, not a thread's or a pattern's pitch.

### web browser

A program that shows web pages, such as the one that runs SolidCraft's web build.

- **In SolidCraft:** the web build; the `home.in_this_browser` catalogue entry.
- **Don't use:** browser on its own.
- **For translators:** not the *browser* panel. Where a language uses one word for both, use the
  CAD word for the panel.

## Words with more than one meaning

An English word that means two things can be translated correctly for only one of them while both
share a catalogue entry. Each meaning below gets its own term and, in time, its own entry.

| Word | Meaning | Use | Where |
|---|---|---|---|
| Construction | a sketch entity that isn't part of a profile | *construction entity* | sketch drawing commands, `ui.construction` |
| | the browser folder of construction planes, axes and points | *construction plane, axis, point* | the Construction folder, `construct.*` |
| Instance | one copy a pattern makes | *pattern instance* | pattern commands |
| | one placed copy of a component | *occurrence* | "Paste (Instance)" on `occurrence.copy` |
| History | the undo and redo history | history | Undo, Redo |
| | the list of features | *timeline* | "Feature history" (`ui.feature_history`), "Roll History Marker" |
| Part | a standard part | *standard part* | `parts.*` |
| | a body, an occurrence or the design | *body*, *occurrence*, *design* | catalogue texts that say "part" |
| | a design property | *part number* | Design Properties |
| Profile | a closed region of a sketch that a feature uses | profile | `solid.extrude`, `solid.revolve` |
| | the cross-section a frame member follows | frame profile | `frame.profile.*` |
| | a named set of preferences | *settings profile* | `settings.profile.*`, Preferences › General |
| Gap | the space a hem leaves | hem gap | the Hem dialog's Gap row |
| | an open end in the sketch continuity legend | open end | the sketch continuity legend |
| Clearance | the Clearance command | clearance check | `inspect.clearance` |
| | a minimum distance field | clearance | the Interference and plastic rules dialogs |
| Flat | a hole's flat bottom | flat bottom | the Hole dialog's tip |
| | a hem that lies flat on the sheet | flat hem | the Hem dialog's type |
| Round | a round relief or corner shape | round (shape) | relief shapes, plastic rests, ruled surface corners |
| | a fillet | *fillet* | `solid.fillet` |
| Normal | a sketch entity that isn't a construction entity | normal (line type) | the sketch palette's line type |
| | the direction perpendicular to a surface | normal (direction) | the ruled surface's Normal mode |
| | the middle display quality | normal (quality) | `ui.display_quality_normal` |
| | a component with a row of its own in a bill of materials | normal (bill of materials type) | `bom_type` |
| Quantity | the number of pattern instances | count | the pattern dialogs' Quantity rows, `ui.quantity` |
| | what the motion plot shows: value, velocity or acceleration | plotted quantity | the motion study's Quantity row, `motion.plot_quantity` |
| Reference | an edge a hole or cylinder keeps its distance from | *reference edge* | `references` of `solid.hole` and `solid.cylinder`, `ui.reference_1` |
| | the measured values a material is checked against | reference (material) | `material.editor.reference` |
| | the distance a light's intensity is given for | reference distance (light) | `render.cpu_light_reference` |

Some translations show the clash today: Japanese and Chinese show a hole's flat bottom with the
word for "unfold" or "plane", Chinese shows "Round" as "fillet", and Chinese and Russian show the
Normal line type as "normal vector".

## Two commands, one label

A label names one command. Today two pairs share one:

- "Edit Feature": `timeline.redefine` and `timeline.edit`.
- "Break Link": `component.break_link` (an occurrence's link to another design) and
  `sketch.break_link` (a projected sketch entity's link to its source).

Search lists both under the same name, and the label lookup gives both the same translation.

## A feature's type

A feature type has one name: its design-file key, such as `sheet_flange`. The label (Flange) is
its display name. Replies and scripts should use the design-file key; today `timeline.inspect`
reports a different name (`EdgeFlangeFeature`), so a script can't match one against the other.

## Open questions for the maintainer

- **British or American spelling** in the docs. The labels are settled: `en.json` is American
  English, and English (UK) (`en-GB.json`, #2501) holds the British spelling. The docs still use
  both (colour and color, centre and center); ids and keys use American spelling.
