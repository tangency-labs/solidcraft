# Components and joints

A design is a tree of **components**. **New Component** (`component.create`) adds one, and
**Create Components from Bodies** (`component.from_bodies`) turns bodies into components. Each
component has its own origin, bodies, sketches and timeline entries. The **active component**
(the radio button in the browser) receives new sketches and features; the others are drawn
faded.

A component is placed by its **occurrences**. Paste creates another instance of the same
component, and **Paste New** creates an independent copy. Move an occurrence with the move
triad, then **Capture Position** to keep it or **Revert** to put it back. SolidCraft asks before
the next command if a move wasn't captured. **Ground** fixes an occurrence in place, and
**Unground** (the same row of the browser's context menu) frees it again.

The design's first part is grounded when it is made: the first occurrence placed directly in the
design by **New Component**, **Create Components from Bodies** or an import. Parts made after it,
standard parts and copies made with Paste or Paste New start free, so do parts made inside other
components. Any number of parts may be grounded, or none; opening a design never grounds
anything. A grounded part refuses Move until it is ungrounded, and `component.create` takes
`grounded: true` or `false` to choose for itself.

**Drag a part to put it somewhere else.** With the design active, press the left button on a part
and drag: the part follows the pointer, the point you grabbed moving in the plane through it that
faces the view, and stays where you let go, with its position captured. The whole drag is one
undo step, and Escape during the drag puts the part back. The drag moves the placement of the
active component that holds the part, so with the design active a part inside a subassembly
moves the whole subassembly; activate the subassembly to move the part inside it. Moving a
definition shared by several placements moves it in all of them, as Move does. A grounded part
does not move, and a part that joints hold to something grounded moves only as far as its joints
allow (below). Dragging any member of a chain joined to nothing grounded moves the whole chain,
preserving its joint values. Shared nested definitions move in every instance; the selected
instance follows the pointer even when its parent is rotated differently from the first one. A drag that starts on empty space, on a part that can't move, or
with Shift, Ctrl (Cmd) or Alt held draws a selection box instead. Other uncaptured moves stay
pending. Scripts and agents do the same with `occurrence.drag {occurrence or path, at, point}`,
which moves the placement by `point - at` in world coordinates and answers the placements it
`moved`.

Paste New copies nested component definitions, sketches, feature histories and generated
dimension parameters. Repeated child instances remain shared within the new copy. Materials,
appearances and wholly internal joints/motion records receive copied references. Design-wide
user parameters remain shared; retained external links and omitted cross-boundary motion
records are reported. Unsupported destructive links refuse the entire copy. See
[individual-instance and copy details](https://github.com/bherbruck/solidcraft/blob/main/docs/occurrence-workflows.md).

**Extract to File** (`component.extract`, in a component's right-click menu) moves a component
into a design of its own: its sketches, features, bodies, subcomponents and the joints among them
go to the new `.solidcraft` file at its origin, with the parameters they use, and the component
becomes linked to that file, as **Insert Design** makes it. Its placements and the joints to the
rest of the assembly stay. Components whose own history builds the same bodies are offered to
become placements of the new link too (`component.copies`, `component.replace_with_link`).
The menu offers Extract only for a component that can leave: nothing outside uses its sketches
or changes its bodies, no bodies were moved into or out of it, and none of its subcomponents is
joined to the rest of the design.

**Joints** (`joint.create`, J) connect two components by joint origins that snap to faces, edges,
circles and vertices: rigid, revolute, slider, cylindrical, pin-slot, planar and ball, with
offsets, flip and limits. **As-built Joint** keeps the current position, **Rigid Group** locks
components together, **Drive Joints** moves a joint through its values, and **Motion Link**
couples two joints. A rigid joint holds its parts still, so Drive Joints has nothing to drive on
it: it says so and offers **Edit Joint**, where the joint's type changes to Revolute (rotate),
Slider (slide) or Cylindrical (both). Links can be chained (one joint drives a second, which drives a
third), and a value is carried through a chain of up to 64 links in whatever order they were
made. Contact sets stop motion where bodies would pass through each other,
**Motion Study** keys joint values along a timeline, and **Interference** shows overlapping
volumes in red, lists the pairs closer than its **Clearance** and each body's nearest neighbour.

In the Joint and Joint Origin dialogs, picking a straight edge puts the origin at its midpoint
with the motion axis along that edge. A revolute joint turns about the picked edge; a slider
moves along it. Round edges use their circle's centre and normal. Existing saved joint origins
keep their axes.

A joint origin on a face or a hole keeps the turn it had when it was picked: when the face tilts
(a bend angle or a rotation changing with a parameter) the part on it tilts with the face but
doesn't spin about it.

A joint origin can also sit on an edge of a flat face (`"edge": [x, y, z]`): at the edge's middle,
pointing out of the face with its y into it, or with `"corner": [x, y, z]` at the corner near that
point, its x and y running along the two edges into the face. The edge, the face and the corner
are found again by name, so a bracket set 40 mm in from a panel's corner (a planar joint with
values `[40, 40, 0]`) stays 40 mm in from that corner when the panel changes size.

A joint's offset, angle and values can be typed as expressions over the design's parameters
(`"offset": "gap + 2"`, `"values": ["travel"]`). The joint keeps them and works them out again
whenever the design changes, so a part seated `gap` above a panel moves with `gap`. Renaming a
parameter carries into the joints that use it, and deleting one they use is refused. If an
expression stops working out (say a parameter becomes an angle), the joint is left out of the
solve and `joint.solve` gives the reason, instead of placing the part from old numbers. Typing a
plain number, or driving the value, drops that expression.

**Relations** are joints that tie only part of a placement, so a part is placed the way a
designer describes it, one relation at a time: `parallel` (z along z, free to move and turn
about it), `perpendicular` and `angle` (z square to z, or at the joint's `angle` from mated),
`point_on_plane` (a point on a face's plane, `offset` off it), `point_on_line` (a point on an
axis) and `line_on_plane` (an axis on a face's plane: a cylinder's radius as `offset` lays it
tangent). A relation is made where the parts are: B moves no further than the relation needs.
Several relations between the same two parts add up; the solver meets them together, each one's
degrees of freedom come off what is left, and one that can't be met is reported, not forced:

```json
{"command": "joint.create", "params": {"type": "parallel", "a": "Panel top", "b": "Bracket bottom"}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel top", "b": "Bracket corner"}}
{"command": "joint.create", "params": {"type": "parallel", "a": "Panel front", "b": "Bracket face", "flip": true}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel front", "b": "Bracket corner", "offset": -40}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel end", "b": "Bracket corner", "offset": -40}}
```

leaves the bracket flat on the panel, square to its front, its corner 40 mm in from the front and
the end, wherever the panel's size takes them. Coincident faces are a `planar` joint, a distance
between faces is `planar` with an `offset`, concentric is `cylindrical` and two points that meet
are `ball`.

The check behind it, `inspect.clearance`, does all of that in one call for any number of bodies,
which suits scripts and agents checking a whole assembly:

```json
{"command": "inspect.clearance", "params": {"clearance": 1, "nearest": 2, "skip_within": "component"}}
```

It returns `overlaps` (pairs sharing at least `min_volume`, 0.01 mm³ by default, with the shared
volume, area, box and centre), `near` (pairs closer than `clearance`, with the distance and the
two nearest points; a contact sharing less than `min_volume` is listed here at 0 with its
`overlap_mm3`), `nearest` (each body's `nearest` closest bodies) and `unchecked` (pairs whose
overlap could not be worked out, with the reason). `against` limits the check to pairs with one
body in `bodies` and the other in `against`, and `skip_within` leaves out pairs inside one
occurrence (`"occurrence"`) or one placed component, its subcomponents included
(`"component"`): the parts of a bought component that touch by design. Distances are measured on
the exact faces and edges; the meshes only rule out pairs that are too far apart, and the pairs
left are checked in parallel. `inspect.interference` remains for the overlaps alone: it clears
pairs whose meshes stay apart without intersecting them, intersects the rest in parallel, and
returns after `budget_s` seconds (default 240) at most, listing the pairs it had no time for as
not checked.

Mesh bodies (STL, 3MF and OBJ imports) are not checked by either command: each is listed in
`unchecked` as `{a: name, b: null, reason: "mesh body: not checked"}`, so `count: 0` or an empty
`overlaps` is not mistaken for "no overlap" when a mesh body was in the design.

**Gear**, **Rack and Pinion** and **Screw** (on the **ASSEMBLE** panel) are motion links whose
ratio comes from the parts rather than a typed number:

- **Gear** (`joint.gear {a, b, ra, rb}`) couples two joints that turn. Joint 2 turns ra/rb as far
  as joint 1, the other way round; **Internal gear** (one of them is a ring gear) turns both the
  same way. Pick the round face of each gear, such as its pitch cylinder, or type the radii. Tooth
  counts work as well as radii, since only their proportion counts. Picking a gear's face also
  chooses the joint that gear turns on.
- **Rack and Pinion** (`joint.rack_pinion {a, b, radius}`) couples a joint that turns with one
  that slides: the rack travels the pinion's pitch radius times its turn in radians (r·θ). Either
  joint may drive the other. In `joint.list` the ratio is given per degree, as for any link between
  an angle and a distance.
- **Screw** (`joint.screw {joint, pitch}`, or `{a, b, pitch}` for a revolute joint and a slider)
  advances `pitch` per turn. A cylindrical joint is a screw on its own: its turn drives its travel.
  The thread is right-handed; **Reverse** makes it left-handed.

A radius or pitch may be an expression of the design's parameters, and a picked face is found
again whenever the design changes. Change the parameter or the gear's diameter, and the ratio
follows. The new link leaves the driven joint where it is, so teeth modelled in mesh stay in mesh.
**Reverse** turns the driven joint the other way, for joints whose axes point opposite ways.
Redefining a link (`link: i`, such as turning it the other way with **Reverse**) also leaves the
driven joint where it is, so the gears stay in mesh at the pose they are in. These
couplings are motion links, so they work in closed loops, with contact sets and in motion studies:
a rack can push the piston of a slider-crank, or a pinion can steer through a rack and its tie
rods. Drive the joint that drives the coupling (joint 1, the pinion, or the screw's turn), not the
driven one. A joint follows one link at most, and a coupling that would drive its own driver is
refused. `joint.list` lists each coupling's definition under `links` with an `error` when it can't
be worked out (the picked face is gone); the link then keeps its last ratio.

**Cam** (`joint.cam {a, b, profile}`, on the **ASSEMBLE** panel) makes a follower ride a cam. The
cam turns on a revolute joint (a); the follower is a slider (b, a translating follower such as a
valve tappet) or a revolute joint (b, an oscillating follower such as a rocker arm). The profile
is the cam's outline in a plane square to its axis: a closed profile of a sketch in the cam's
component, or the cam's edges picked on its body (one click on an edge takes the closed loop it is
on in that plane). The follower is:

- a **Knife edge**: a point on the cam;
- a **Roller** of a radius, typed or taken from the roller's round face (picking that face also
  says where the roller is);
- a **Flat face** square to a slider's line, such as a bucket tappet.

A translating follower's line runs along its slider's axis, through its **Tip** when one is picked
(its knife's point or its roller's centre), so a follower offset from the cam's axis rides the
right flank. An oscillating follower needs its tip, or its roller's face, to know its arm. The
follower rides the cam from the side it is on (**Other side** takes the other one). The follower's
travel over a turn of the cam is worked out from the profile and kept with the link, one value a
degree with a smooth curve between them that never overshoots its samples. It is worked out again
whenever the design changes: change the cam's sketch, a parameter or the roller, and the follower
follows. Like the gears, the new link leaves the follower where it is (model it touching the cam),
and the cam is a motion link: it drives in **Drive Joints**, plays in motion studies, works in
closed loops (a tappet can be the piston of a slider-crank) and stops at contact sets. The profile
may also be on the grounded part, with the follower's guide on a turret that the cam joint turns.
`joint.list` gives a cam's link `lift` (the follower's lowest and highest values and its rise)
instead of a ratio. A profile that is not closed, a line or an arm that misses the cam somewhere,
a follower that would jump, and axes that are not square or parallel are refused.

A joint moves one component against another, so its two picks must be on different components.
A STEP file with several parts often comes in as bodies of one component, and picks on two of
those bodies make no joint. The Joint and As-built Joint dialogs say so under the picks and offer
**Create Components from Bodies**: each picked body gets a component of its own and stays where it
is, the picks are kept, and the joint previews. `joint.create` refuses such origins with the same
advice.

Joints may close a loop, as in a four-bar linkage or a slider-crank. When one joint of the loop
is driven, the other joints in the loop follow so that every joint still meets, and the
mechanism stays in the assembly mode it started in (an open four-bar does not flip to the
crossed one). If the motion would take a loop past the point where it can still close, for
example driving a rocker beyond its swing, the motion stops at the last position where the loop
closes, and the drive reports `stopped_by_loop`. A loop that cannot close at all is listed
under `conflicts` and nothing in it moves. `joint.list` reports how many degrees of freedom each
part still has with its loops closed: the moving parts of a four-bar have one between them, and a
loop locked by a rigid joint has none. A joint a motion link drives adds no freedom of its own:
two meshed gears on a fixed housing have one between them, each turning with the driving gear. The Drive Joints dialog says when a drive stops short and
why. While it previews, a row under the values gives the value the joint reached and the reason
(the loop that can't close further, or the two parts a contact set keeps apart), and after OK the
status line repeats it. The value box keeps what you typed. The dialog opens on the joint
picked in the browser, or on the first joint that moves when none is picked.

**Drag a part that joints place** and it moves as far as its joints allow: the point you
grabbed follows the pointer in the plane through it that faces the view, the joints on the
part's chain take the values that bring it nearest, within their limits, through motion links,
and with closed loops kept closed, and a four-bar's rocker stops at the end of its swing. The
whole drag is one undo step. A grounded part, or one its joints hold fixed, is not moved; a drag
on it draws a selection box. Each frame runs `joint.reach`, which scripts and agents use
directly to put a part's point somewhere: `joint.reach {occurrence, point}` moves the joints on
its chain from where they are to the nearest pose (the arm keeps its elbow up or down, and a
linkage stays on its branch). `at` picks the part's point (the centre of its bodies by
default), `z` with `axis` also turns one of its axes, and `joints` names the only joints to
move (the other joints of a closed loop they are in follow). The answer says whether it
`reached` the point and how close it got (`error_mm`, `error_deg`), and when it falls short,
the limits it rests on (`stopped_by_limit`) or the loop that stopped it (`stopped_by_loop`).
Contact sets are not checked by a reach; drive the joints for that.

The solver places what it reaches from the root and from grounded components. A chain of joints
that is joined to nothing grounded is never placed, so driving one of its joints moves nothing.
The drive says so: `joint.drive` answers `moved: false` with a `reason` and the `unconnected`
part ("Pedestal:1 is not connected to anything grounded"), the Drive Joints dialog shows the same
row, `motion.play` lists such joints under `unmoved`, and `joint.list` names every such chain under
`floating`. Ground a part of the chain, or join it to a grounded part (a rigid as-built joint keeps
it where it is), and it moves.

`joint.drive` answers the joint's values as `joint.list` gives them, in degrees and millimetres
with their units and limits, and `moved` says whether any part moved. A joint's origins are mated
by default: B's z axis is turned against A's, as two faces meet. `flip: true` lines them up
instead, so two origins given at one point with the same z stay as they are. Without an
`angle`, a new joint (other than a relation, which starts where its parts are) takes no turn or a half turn about its axis, whichever turns B less: two
faces that already meet, a plate under a panel, stay as they are instead of B spinning about
their shared normal. For scripts that drive many steps, `quiet: true` leaves the per-part
degrees of freedom out of joint answers, and
`motion.play` with `quiet: true` gives the poses of only the parts that moved.

**Motion Trace** (`motion.trace`, on the **ASSEMBLE** panel) draws the path a point of a part
takes while a joint is driven or a motion study plays, such as the coupler curve of a four-bar.
Pick the point on the part (a face centre, a circle centre, an edge midpoint or a vertex), then
either a joint with the values to drive it from and to (a turn of the joint from where it is, at
first) or a motion study. The path previews in the viewport as the mechanism would move: closed
loops follow, and the path ends where a loop can no longer close or a contact set stops the
motion, which the dialog says under the values. OK keeps the path as a 3D sketch spline in a new
sketch of the root component (**Motion Trace1**, ...), where no joint moves it, and it can serve
as a sweep or pipe path. New traces retain their source and update when the mechanism's
joint origins, limits, motion links or driving conditions change. Playing or dragging the
mechanism does not redefine the trace: it keeps the assembly branch from when it was made.
Unrelated parameter and display changes do not resample it. A path that comes back to where
it started is kept as a closed, smooth spline.
`motion.trace {occurrence, point, joint, from, to, index?, samples?}` (or `{occurrence, point,
study, samples?}`) answers the world points without changing the design, and
`motion.trace.keep` with the same parameters (and `name?`) keeps them. `point_ref` can retain a
joint-origin snap (such as `{occurrence, face: [x,y,z]}`), so the point follows its resized
face; without it, the point is fixed in the part's coordinates. The dialog retains its face
or circle snap. `motion.trace.edit {sketch, from?, to?, samples?}` changes a joint trace's
driving conditions; `joint` with `from` and `to`, or `study`, replaces its motion source.
Expressions in `from` and `to` follow parameter edits. Study traces follow edits to the
referenced study, including its step count when Samples was left at the default.
`auto_update: false` in `motion.trace.keep` makes a fixed curve; in `motion.trace.edit` it
detaches an existing association, preserving the curve. Curves saved by earlier versions
remain fixed. `motion.list` includes the associations and their errors. If a source is lost,
the sketch warns and keeps its last successful curve until the source is restored or the
association is detached. Changes and their generated curves undo and redo together.
A trace takes 2 to 500
samples: 100 by default for a joint, one per step for a study.

The [Chebyshev linkage demo](https://github.com/tangency-labs/solidcraft/blob/main/docs/chebyshev-motion-trace.md)
builds a complete four-joint mechanism, plays a saved study and demonstrates the
same retained trajectory updating after a link-length or driving-range edit.

**Export Animation…** in the **Motion Study** dialog saves the study as an animated GIF, drawn
from the viewport's camera as the study plays (the keys in the dialog count, even before OK).
`motion.export_animation` does the same for scripts, the CLI and agents, without a window: give a
`study`, or a `joint` with `from` and `to` (and `index?`) to animate a drive, and a `path`. A path
ending in `.gif` gets an animated GIF that loops; any other path is a folder that gets the frames
as `frame-0001.png`, `frame-0002.png`, and so on (made if missing; frames of a longer earlier
export there are removed), for a video tool. The web app writes GIFs only. `samples` is
the number of frames (one per step for a study by default, 100 for a joint), `width` and `height`
their size in pixels (800 × 500 by default, at most 4096 a side), and `fps` the GIF's speed (20
by default); `grid: false` and `axes: false` leave out the ground grid and the origin's axes.
`camera` takes the reply of the control channel's `ui.camera` (or any of its fields;
the rest come from the view `view.apply` chose, or the home view framing the model). The frames
are drawn by the same renderer as `solidcraft-cli snapshot`, on a copy of the design, which is not
changed. A GIF keeps one palette for all its frames, so colours don't flicker, and stores only
what changed from one frame to the next. Frames are capped: at most 1000, and a GIF holds at most
2^26 pixels of frames (about 160 frames of 800 × 500).
An exploded view animates the same way: give its name or id as `view` (instead of a `study` or a
`joint`) and the parts come apart at a steady pace, from the assembled placement to where the view
shows them; `back: true` brings them back together, exploded at the middle frame. That takes 41
frames by default, or 81 with `back`, and the home view frames the parts both assembled and
exploded. As with `explode.show`, the design keeps its placements.

**Motion analysis.** The **Motion Study** dialog's **Study** box opens a saved study to edit, and
by its timeline the dialog charts what the study does.
**Plot** picks a moving joint value (a joint picked in the Browser is picked here too), or
**Picked point** once a point is picked in the dialog's **Point** box, and **Quantity** whether to
show its value, velocity or acceleration; the chart runs over the
study's steps with a cursor at the step shown, and a click or a drag on it moves to that step.
Values are in degrees and millimetres; rates are per step, or per second once **Duration** says
how many seconds the whole study takes. A picked point charts its distance from where it
starts, its speed and the size of its acceleration. **Check collisions** plays the study and
lists the pairs of parts that pass through each other, with the steps where they do (a click on
one shows the first of them); those steps are shaded on the chart. It checks the parts picked in
**Components** (against the ones picked in **Against**, when there are any), else every
component, or the pairs of the design's contact sets (a choice beside the button once it has any).

`motion.plot {study, joints?, occurrence?, point?, samples?, duration?}` answers the same
numbers: for each joint value its `value`, `velocity` and `acceleration` per sample (with their
units), and for a point its `position`, `velocity` and `acceleration` vectors, `speed` and
`acceleration_norm`. The study plays as `motion.play` places it (closed loops follow, contact sets
stop it), each sample from the one before, on a copy: the design is not changed. Velocities and
accelerations are finite differences between samples, central inside and one-sided at the ends,
so they are as smooth as the samples are dense (up to 1000, one per step by default). Angles are
unwrapped, so a joint that turns on past half a turn keeps counting.
`motion.interference {study, occurrences?, against?, contact_sets?, samples?}` answers each
colliding pair with its `first_step`, `last_step`, the `steps` ranges where they overlap, how
far into each other (`depth_mm`, the most at `deepest_step`) and their shared `volume_mm3`
there (null when it is not cheap to work out). It plays the study as `motion.play` does, but
contact sets don't stop it, and parts are tested at the samples only: a part that passes right
through another between two samples is missed. Parts are told apart by occurrence, as contact
sets tell them apart: an occurrence inside a sub-assembly placed twice is one part, so its two
copies are not checked against each other. A check takes at most 256 bodies; on a large design,
list the `occurrences` to check (an arm, with `against` the parts it must miss). When the work
runs out it says `truncated` and the step it `checked_to`.
`joint.sweep {joint, from, to, index?, step?, refine?, occurrences?, against?, contact_sets?,
report_gaps?, apply?}` drives one joint value from `from` toward `to` and answers how far it gets
before parts touch: `stopped_at` (the last value clear of contact), `stopped_by` (the two parts
and the point `at` which they touch, or `"loop"` where a closed loop can no longer close, or
`"limit"`), and with `report_gaps` the nearest gap at every sample and the nearest each part came
to another (`min_gap`). The samples are fine enough that no part passes through another between
two (or `step` apart), and the first contact is bisected until no part moves more than `refine`
(0.01 mm by default) across it. Gaps are distances between the parts' tessellations
(`gap_method: "mesh"`): exact for flat faces, within `gap_tolerance_mm` for curved ones. Pairs
already touching at `from`, such as parts joined face to face or a door resting on its frame, are
listed in `touching_at_start` and stop the sweep only once they pass into each other, deeper than
they start. The sweep runs on a copy (closed loops follow, contact sets don't stop it) and
leaves the design as it was unless `apply` is true.

**Insert Design** (`component.insert_design {path, translate?}`, on the **ASSEMBLE** panel and
in the **File** menu) adds a component linked to another saved design, so each part can live in
its own document. A STEP or mesh file picked there is inserted as **Insert STEP** or **Insert
Mesh** would. The part's solid bodies come
in as they sit in that design, and they are kept in the assembly, so it opens even without the
part file. The link is stored relative to the assembly's folder once the assembly is saved.
After the part file changes, **Update Links** (`component.update_links`) reads it again.
To edit the source, right-click a linked component or occurrence in the Browser and choose
**Open Linked Design**. It opens the part in another tab and reports if its file is missing.
**Break Link** (`component.break_link`) keeps the bodies and forgets the file, and
`component.links` lists the links and whether their files are found. A linked component holds
the part's bodies only: its timeline, parameters and sheet-metal data (flat pattern, bend
table) stay in the part's own design.

If a source file moves or is renamed, right-click its linked component or occurrence in the
Browser and choose **Relink Source (All Instances)…**. Choose the saved `.solidcraft` design
to use. Relinking updates that component's stored path and solid bodies for **all its
instances**, keeping their identities and placements. Cancelling or choosing a missing,
invalid, empty or self-referencing design leaves the assembly unchanged. The operation is
one undo step and the new path is saved relative to the assembly when possible.

Scripts use `component.relink {component, path}`. The reply includes `previous_path`, `path`,
`warnings`, `joint_errors` and `joint_conflicts`. Changed geometry can invalidate a joint's
face reference: relinking reports those joints in the reply and UI status/message history;
repair the joint or undo the relink. Relink does not repair or verify the intended meaning of
existing joints.
When linked geometry changes and the component has joints, a reminder to check those joints
is also shown even if the solver reports no error: a successful solve does not prove that a
joint still describes the intended connection.

STEP export writes the component tree as an assembly.

Right-click a component in the Browser and choose **Properties** to inspect
material-aware mass, volume, surface area, centre of mass and world bounds. Descendants and
repeated instances contribute to assembly totals; pending moves are included. A component
uses its first placement; `occurrence.properties` selects a specific instance. Root Properties
measures the complete assembly. Volumes are summed, including overlapping bodies, rather than
measuring their union. The same workflow is available through `component.properties` and
`occurrence.properties`; see
[placement and measurement details](https://github.com/bherbruck/solidcraft/blob/main/docs/assembly-properties.md).

## Part properties and the bill of materials

A component definition carries its own **part properties**: a part number, a description and a
revision, other tools' data under a reverse-DNS key (the same rules as the design's own
properties), and free text properties such as a material grade or a finish. Every occurrence of
the component shares them, Paste New copies them, and they are saved with the component.
`component.metadata {component}` reads them and
`component.metadata.set {component, part_number?, description?, revision?, values?, properties?}`
changes them in one undo step (an empty value removes a field). The root component is the
design itself: its properties are `document.metadata`'s. Standard parts (`parts.insert`) get a
part number from their standard and size (`ISO 4762 M6x20`, or a bearing's designation) and a
description; **Change Part Size** renumbers them, keeping a field you changed yourself.

**Bill of Materials** (`assembly.bom`, on the **INSPECT** panel) counts the placed components:

- `levels: "flat"` (the default) lists the parts at any depth, each counted through its
  subassemblies: a part placed once in a subassembly that is placed twice counts 2. A
  subassembly is listed through its parts, unless it has bodies of its own.
- `levels: "top"` lists the direct children, a subassembly as one row.
- `group_by: ["part_number"]` (the default) puts definitions with the same part number in one
  row; a definition without a part number gets a row of its own. `["definition"]` gives every
  definition its own row.
- `group_children: true` keeps what is placed inside a part with a part number in that part's
  row: a bolt with a washer and a nut placed inside it is one row, the washer and the nut listed
  under it as `children` (items 1.1, 1.2…, with `each` per bolt and `count` in all) instead of
  rows of their own. Bolts with different contents (one with a nut, one without) are different
  rows. `total_occurrences` counts the children too, so it is the same either way.
- `component` lists one subassembly's parts instead of the design's.

Rows are sorted by part number (rows without one come last) and hold `item`, `part_number`,
`description`, `revision`, `count`, `definition` (the component names), `components` (their ids)
and `properties`, with `total_occurrences` beside them. `path` writes them to a `.csv` file
(the columns item, part_number, description, revision, count and definition, every field
quoted; children follow their row) or a `.json` file. The window lists the same rows, switches
between the levels and groupings and shows children under their part, edits a picked row's part
number, description and revision, and exports them. Changing part properties changes no
geometry, so it doesn't evaluate the model or re-seat joints again.
