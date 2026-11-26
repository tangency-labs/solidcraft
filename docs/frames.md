# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members and their profiles, corner cuts, the weld gap, end caps, mounting plates, bolted end plates,
attached gusset plates, and trimming or extending to members, planes and faces.
Welds and the cut list follow.

## The workflow

1. **Sketch the frame**: lines and circular arcs in a sketch, or a 3D sketch for a frame that leaves one plane
   (`sketch.options sketch_3d: true`, then `sketch.line3d` and `sketch.arc3d`). Drive the sizes with parameters as
   usual; members follow.
2. **Choose a profile**: a standard steel size by name (`{"size": "RSC 203x89"}`, below), a
   built-in one from its dimensions (below), or your own drawn in a sketch
   (a vendor's extrusion: insert their DXF with `sketch.insert_dxf`). A profile sketch's origin is
   the point that sits on each line, and its x and y are the profile's.
3. **Add members**: **Frame Member** (WELDMENTS › CREATE, `frame.member`) makes one body per selected
   line or arc. Enable **One Continuous Member** to make a connected tangent line–arc–line run
   into one tube. Select its segments in path order. Repeat for another physical member.
4. **Trim them**: **Trim/Extend** (WELDMENTS › MODIFY, `frame.trim`) butts members against the
   members they meet, or trims or extends them to a plane or face (below).

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}}
{"command": "frame.member", "params": {"sketch": "Frame", "lines": ["l5", "l6"], "profile_sketch": "45x45 section", "anchor": "bottom_left", "angle": "90 deg"}}
```

A run drawn as several lines end to end makes one member per line. To make it one member at its
full length, select the lines in the sketch and use **Merge Lines** (SKETCH › MODIFY or the
right-click menu, `sketch.merge_lines`, #20317). The first selected line keeps its id and runs the
whole length. Its direction relations stay and the pieces' lengths go. A line ending between the
pieces stays on the merged line. A Frame Member already built on the sketch names the merged line
in place of the ones that went, and drops their names and corner overrides; a Rotate step on a
line that went turns the merged member. Separate Frame Members on different lines of the run each
name the merged line, so they overlap: delete the extra ones. Merge Lines takes lines of a 2D
sketch; 3D sketch lines (`sketch.line3d`) are refused with `unknown curve`.

```json
{"command": "sketch.merge_lines", "params": {"curves": ["l1", "l2", "l3"]}}
```

Members are ordinary bodies: mirror, pattern, move, drill and combine them with the usual
commands. Where members meet they overlap unless you choose a corner type (see Corners) or use Trim/Extend.

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member, End Cap,
Mounting Plate, Gusset, Trim/Extend and Parameters. Like Sheet Metal it is always there, and it selects itself when a
frame member, end cap, mounting plate or gusset is the selected body (or the last body of the active component).

## The Frame Member dialog

- **Path**: pick the frame sketch's lines or circular arcs in the view (2D or 3D). **All Lines**
  makes a member on every line of that sketch instead, including lines drawn later.
- **One Continuous Member**: turn off **All Lines** and select lines and arcs in connected,
  tangent path order. Backwards curves are reversed automatically. Sharp joins need an explicit
  tangent bend arc. A continuous member cannot include point pairs.

Curved and continuous stock currently requires **Corner Type: None**, no corner overrides,
Fit Ends, Coped, Flip or gap. Unsupported combinations are refused before changing the design.
Curved and continuous members cannot yet be combined with multiple placement groups.
They also take world Z as up: an **Up Axis** other than Z is refused for them.
Separate curved members may share a plain None feature with point-pair members. Ordinary straight
members and point pairs retain Auto, corner, fitting and gap behavior described below.
- **Section (optional)**: pick the section's region in another sketch (the ring between two
  circles makes a tube). With nothing picked, **Profile** chooses a built-in profile and its
  dimensions follow; optional ones left empty take their defaults.
- **Corner Type**, **Flip** and **Corner Overrides** cut the members where two of them meet, as
  under Corners below, and **Gap** leaves a weld gap at each welded end (see Multi-trim and the weld
  gap). A new Frame Member and the command both start on None.
- **Anchor**, **Angle**, **Mirror** and **Up Axis** place the section on each line, as below.
  **Name** names the members (default: after the profile).
- **Offset A** and **Offset B** move the section from its anchor by a length, along the
  section's own axes as drawn (A across its width, B across its height; they turn with **Angle**,
  so they are not the world's X and Y): Anchor **Bottom Middle** with Offset B `50 mm` puts a
  tube's bottom 50 mm above its line. Both empty: the anchor sits on the line. The offsets are
  your intention: once typed (or set by an arrow or the marker in the view), changing a
  dimension of the profile, the **Anchor**, **Mirror** or **Angle** keeps them and moves the
  member to keep them (Bottom Middle with Offset B `50 mm` on a 40 mm tube, then a height of
  60 mm: the tube's bottom stays 50 mm from its line; Anchor **Top Middle** then puts its top
  there). They are saved as the member's `locate` (the point of the section on the line), so
  **Edit Feature** shows a member made with `locate` as the offsets from its anchor, and a
  member placed by its anchor alone with both offsets empty (OK then adds no `locate`). Offsets that
  cannot be applied (the point on the line would be past the largest profile, 10000 mm, or the
  section cannot be laid out) are refused at **OK**, naming Offset A and Offset B; a value that
  is not a length yet leaves the section where it was until it is one.
- **Hot points**: while the dialog is open, the view draws the section at the start of the first
  chosen line, as the member will be placed (angle and mirror applied), with a dot at each anchor
  (origin, centre, centroid and the corners and edge middles of its bounding box; anchors that
  fall on the same point share one dot). The anchor in use is ringed. Click a dot to make it the
  **Anchor**; Offset A and B are emptied (the anchor wins). Drag the marker at the line's start
  over the section, or past it to put the member off its line, to put that point on the line
  (Offset A and B are written from the anchor, rounded to 0.1 mm; hold Alt or Ctrl for no
  rounding); let go near a dot and it takes that anchor instead.
- **Arrows and arc**: an arrow labelled **A** out from the middle of the section's +A side and
  one labelled **B** out from the middle of its +B side drag the section along that axis: they
  change Offset A or Offset B and keep the other as typed (an expression stays an expression).
  Each drag adds whole steps (1 mm; 0.1 mm with Alt or Ctrl) to the offset as it was at the press,
  so a press that does not move a step changes nothing and a typed `20.3 mm` becomes `21.3 mm`
  one step on. An arrow's label is its letter alone while its offset is 0 or empty, and the
  letter with the offset once it is not (`A 5.0 mm`, in the design's unit, 0.1 mm shown), live
  while it drags. An arc past the section's corner farthest from the line turns the section about
  the point on the line, adding 15° steps (1° with Alt or Ctrl) to the **Angle** at the press
  (`7 deg` becomes `22 deg`). Dots, arrows and arc are hidden while the section is too small on
  screen or seen edge-on.
- While they are drawn, presses on the drawn section, the arrows, the arc and a margin of about
  24 px around them belong to the hot points: one that misses a handle there does nothing, so a
  slightly missed arrow never starts a box selection that would empty **Lines**. Elsewhere the
  view picks and box selects as usual.
- Only the first line of the group shown gets the section; the anchor is the same on every line
  of the group. Nothing is drawn until the lines and the profile make a member.
- **Groups**: one feature can lay several groups of lines in the same profile, each with its own
  lines, **Anchor**, **Angle**, **Mirror** and **Name** (uprights turned one way, rails another).
  **New Group** starts an empty group and shows it; click a group to show and edit it, **Remove
  Group** removes the one shown. A line belongs to one group only.
  Several groups currently require **None**, with no corner overrides, point pairs, **Flip**,
  **Fit Ends**, **Coped** or weld gap. Unsupported combinations report an error and keep the
  selected settings; they are not silently dropped. Use separate Frame Members for joined
  corners or point pairs. A single group uses the normal flat form and retains all corner,
  pair and finishing options. Later flat frames can fit their ends to earlier grouped members,
  using each group's actual anchor, mirror and rotation. **Offset A**, **Offset B** and the hot
  points act on the group shown and are kept with it.
- The preview shows the members as you change the values. **Edit Feature** reopens the dialog with
  the feature's values; the members keep their names through the edit, and a new **Name**
  renames them all. A member made with a whole profile sketch keeps it (the **Section** row shows
  the sketch; untick it for a built-in profile).

## Corners

**Corner Type** (`corner`) cuts the members where two of them meet: `none` (the default, members
overlap as before), `mitre`, `butt` or `auto` (shown as **Auto**, **None**, **Mitre** and **Butt**).
**Auto** is described in its own section below. A new Frame Member made in the dialog starts on None.

**Coped** (`coped`, a tick box to the right of Fit Ends to Earlier Frames, unticked by default) copes
every butt joint of the frame: the tube that gives way is cut to the other tube's real outer shape
(its outline with its radii, hole filled), not flat to its face or to its squared outline. That is a
Butt corner's butting member and the butts Auto chooses (each coped as below), a tube ending on the
middle of another, the tubes fitted where three or more end, the pieces where tubes cross, the
notches and splits where tubes overlap, and with Fit Ends to Earlier Frames the ends fitted to earlier
frames' tubes. Mitres stay mitres (a Mitre frame's corners, Auto's mitres and Mitre overrides). Auto's
angled butts at nearly in-line corners stay flat too: there the short tube meets the long tube's
square end, not its side. A round tube's squared outline is already its real one, so there Coped
changes only the butt corners. The box is greyed under None, which cuts nothing. A design or script
from an older build that says `coped_butt`, as the frame's type or an override's (in `frame.member`
or `timeline.edit`), is read as `butt` with `coped` ticked; on a None frame such an override is
then a plain Butt.

A **corner** is a point where two or more of the feature's members end, within 0.001 mm, and no
other member passes through: sketch-line members and members between two points (`pairs`) alike.
Where exactly two end, they make the corner. Where three or more end (a rectangle with a brace from
corner to corner), the two that come first in Auto's laying order (see Auto below: outer tubes first,
longest first, never the pick order) make the corner, finished with the frame's type and flip
exactly as if they were alone there, and every other tube ending there is cut to fit the tubes laid
before it there, as Auto fits it, whatever Fit Ends to Earlier Frames says: a brace fits into a
rectangle's mitre or butt. Where sketch lines cross, Mitre and Butt split the tubes exactly as
Auto does (see Crossings under Auto below: one stays whole, the others are cut into pieces fitted to
it, with the same piece names, Crossing overrides and Remove Tube), and a tube ending where they cross
(a post up to the middle of an X brace) is fitted there to the tubes laid before it, as under Auto.
A piece counts as its whole line wherever tubes are ranked: its line's place in the laying order, and
its line's length for which member of a butt runs through. A tube ending on the middle of another
(a post standing on a rail, a sketch line, a pair or a 3D-sketch line alike) is always fitted to it,
whatever Fit Ends to Earlier Frames says and whichever is laid first: the tube whose end lies there
gives way. Tubes that still overlap after that are notched or split (see Overlaps under Auto below).
None leaves crossing and overlapping tubes as they are. Two members in line (within 0.01 degrees)
stay square whatever the type.
A corner sharper than 5 degrees inside is refused: "Corner at (x, y, z) is too sharp to finish".

- **Mitre**: both members are cut on the plane that bisects the corner.
- **Butt**: one member runs through and the other butts against it. Unflipped, the longer member
  runs through; members within 1 mm of each other in length count as equal, and then the one nearer
  the sketch's X axis runs through, then the one drawn first. In that drawing order the sketch's
  lines come first and the point-to-point members after them, in their `pairs` order; a Frame Member
  of pairs alone (no sketch, so no X axis) goes by the pairs' order. **Flip** (`flip`) swaps them. The
  through member ends flush with the outside of the other, and the other is cut flat on the near
  face of the through member. A butt whose through member would run past the far end of the other
  member is refused: "Corner at (x, y, z) is too flat to butt".
- **Butt** with **Coped**: the through member is as for Butt. The butting member is cut to the
  through member's full outer shape, so it fits exactly (a fishmouth on round tube). It keeps its
  largest piece on its own side of the through member, and a member that lies wholly inside the
  through member is refused.

**Corner Overrides** (`corners`) give single corners their own type, member order and weld gap.
Each is saved by the ids of the members that meet there, in order (a line's id, or a pair's
`pair1`, `pair2`…; a point where three or more end takes an override only under Auto), not by the
point, so it survives a dimension change while the members still meet. The first member runs
through a butt (see Member order under Multi-trim and the weld gap). If they no longer meet, the
feature fails with "A corner override is not at a corner of the frame", and Edit Feature still opens
with that override listed so it can be removed (or `frame.multi_trim` with `remove` takes it away).
Changing the feature's Corner Type leaves existing overrides alone; a new override starts as the
feature's type.

A member cut away completely fails with "the member on line `id` was cut away completely", and one
its corners cut into pieces with "the member on line `id` is cut into pieces by its corners".

An **End Cap** goes on an uncut end, or on the through member's square cut end of a Butt corner
(coped or not) at a 90° corner, which is cut square to its own line. It refuses every other end: a mitre, the butting member's end of a
Butt (it lies against the through member), the cope face of a coped Butt, and the end of a tube fitted
where three or more end.

In the dialog, a box drag in the view picks the frame sketch's straight lines (not construction,
projected lines or arcs), and the corner points when Corner Overrides is active (Shift adds). Each
override is one row, **Corner 1**, **Corner 2**… in pick order: its type, its **Flip** (greyed for
None and Mitre) and a bin that removes it, with a **Gap** row under it. The arrow on the Corner
Overrides line folds the rows away; picking another corner opens them again. Pointing at an overridden corner in the view lights
its row (or the Corner Overrides line while the rows are folded).

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "round_tube", "diameter": 40, "wall": 3}, "corner": "butt", "coped": true}}
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}, "corner": "mitre", "corners": [{"members": ["l1", "l2"], "kind": "butt", "flip": true}]}}
```

**Known limits.** Each section is placed from world Z and the line's own drawn direction. Where two
members' sections come in turned or offset differently (lines in a vertical plane, lines drawn head
to head, or 3D sketches), a rectangular, off-centre, angle or channel section will not meet wall to
wall at a mitre or butt: the cut planes are right, but no section is turned to match. Square and
round tubes centred on their lines always meet. Corners of 3D-sketch lines cut (the tie falls to
drawing order) but cannot be picked in the dialog; set them through `corners` in the command or
with `frame.multi_trim`. Nor can a corner where a point-to-point member ends: Corner Overrides takes
the frame sketch's points and finds corners among its lines alone. Set those the same way with the
pair's id; Edit Feature
keeps them, each listed by its type with a bin to remove it, and Remove Tube drops the overrides that
name the tube it takes out. A box drag in the dialog doesn't pick 3D-sketch lines: click them one by
one, or list them in `lines`.

## Auto

**Corner Type Auto** (`corner: "auto"`) works out every joint of the frame for you. It is first in the
Corner Type list and a new dialog starts on it; the command's default stays `none`. Where the other
types finish only points where members end, Auto also handles T-junctions, crossings, and a different
joint for each corner by its angle.

**Laying order.** Tubes are laid in this order, and each is fitted to the ones before it: outer
tubes (the outline of the frame), longest first; then inner tubes that span the frame, squarest
first; then other inner tubes; then bridges. Tubes within 1 mm of each other in length tie; the tie
goes to the tube nearer the sketch's X axis, then to the drawing order. Member ids, body names and
face names do not depend on the laying order.

**Joint by angle.** At a corner of two tubes the angle is how far the short tube swings from in line,
rounded to 0.01 degrees:

| Swing from in line | Joint |
|---|---|
| 30° or less | angled butt |
| over 30° and under 60° | mitre |
| 60° to 120° | butt: the long tube runs through, the short tube is cut to fit |
| over 120° | mitre |

Between two different sections a mitre is replaced by a butt. A corner sharper than 5 degrees inside
is refused as before. Under Auto the frame's own **Flip** is hidden: an override's first member is
the long tube at its corner, and its Flip swaps which tube is long and which is short from Auto's
own choice.

**Fits.** A tube that ends against the side of another (a T-junction), or one of three or more tubes
ending at a point, is cut to fit the tube it meets, only near its end: flat on a box tube, with the
radii squared, and a fishmouth on a round tube. On an open section (an angle, a channel) the end
reaches across the open side to the face beyond, the far leg or the web, and an inside corner's
fillet is kept, not squared.

**Tubes in no sketch plane.** Point-to-point tubes (`pairs`) and 3D-sketch lines are laid after every
group, chained among themselves: where two of them end together they are mitred (or joined as an
override there says), and a third one ending there is fitted to the tubes already there. Their free
ends are fitted where they meet this frame's tubes or, with Fit Ends to Earlier Frames, earlier ones.
A tube of this kind that ends on one of the frame's sketch tubes, or where sketch tubes end, is fitted
to it, not mitred, whether or not Fit Ends is ticked; when two of them end at such a point, the second
is fitted to the sketch tubes and the first.

In the dialog a point-to-point tube is one **Path** row ("Tube n: a → b"). A first point waits with the
prompt "Pick the second point" and is not a row yet.

**Lined-up sections.** Under Auto the sections of all tubes are lined up: up is the sketch's normal
and the outer tubes run round the outline anticlockwise, so square and rectangular sections meet wall
to wall in any plane. A tube drawn against that direction is reversed; its face names stay the same.
The section line-up limit under Corners (Known limits) applies only outside Auto.

**Crossings.** Tubes of the sketch that cross are split at the crossing, under Auto and equally under
Mitre and Butt (None leaves them overlapping). The earliest in laying order
stays whole; each other tube is cut into pieces, each fitted to the tube that stays whole, so nothing
overlaps. Where more than two tubes pass through a point, or tubes also end there, the earliest stays
whole and the rest are cut and fitted in laying order. A crossing override (**Crossing** row, picked
at a sketch point lying at the crossing, also where another tube ends there) has **Flip** and
**Gap** only: its first member stays whole (with Flip, the other tube of the pair does). Its type is
ignored; its gap stands the pieces' ends off the tube that stays whole. Edit Feature lists an
override of a crossing with no sketch point as a Crossing row with a bin; it still applies.

Each piece is a tube of its own. The first piece keeps its line's name; a later piece is named
`<line's name> · <crossing tube's name>`. Piece names are pinned like line names, so renaming a piece
sticks. **Remove Tube** on a piece removes its whole line. Overrides name lines, never pieces: a
`corners` entry naming a piece is refused. End Cap and `names` accept pieces.

**Overlaps.** Under Auto, Mitre and Butt, two tubes of the frame whose solids still overlap
where no corner, crossing or fit joins them are resolved too: tubes in no sketch plane (pairs, 3D-sketch
lines) crossing each other or sketch tubes, and tubes that only clip each other where their lines
miss. The later of the two in laying order gives way (a piece of a crossing takes its line's place);
it is cut by the other's filled tube, only where they overlap. One piece left is a notch: the tube
keeps its name. Two or more is a split: the piece from the line's drawn start keeps the name, and each
later piece is named `<tube's name> · <name of the tube that cut it>`, its id `L>X` (L the tube's
line or pair id, X the cutting tube's; where the cutting tube is a piece of a crossing, X and the
name are its line's, so a crossing elsewhere on that line renames nothing). A piece wholly inside the
cutting tube is dropped; one that sticks out is kept. With Fit Ends to Earlier Frames, every tube
also gives way where it overlaps an earlier Frame Member's tube or a copy of one, which never changes. X is then `<feature id>:<member
id>` for an earlier Frame Member's own tube, so renaming that tube keeps the piece's id, or the body
name of a Move, Mirror or Pattern copy; the piece's name uses the tube's body name either way.
None leaves overlaps as they are. Split pieces behave like crossing pieces: their names are pinned
once renamed (`names` accepts `L>X`), **Remove Tube** on one removes its whole line or pair, and End
Cap goes on a piece's line end but refuses the end where it was cut ("not an open end"). To make the
other tube give way, add a `corners` entry (or `frame.multi_trim`) naming the two tubes' line or pair
ids, the one to stay whole first, or with Flip; its type is ignored, it takes no gap (an overlap is
notched, not welded), and an entry for two tubes that no longer overlap fails the frame with "A
corner override is not at a corner of the frame".

**Fit Ends to Earlier Frames** (`fit_ends`, ticked by default in the dialog, false when absent)
fits the tubes' free ends to the tubes of Frame Members made earlier in the timeline, to the Move,
Mirror and Pattern copies of those, and to this frame's own tubes laid before them. A corner that lies
inside an earlier tube, or on two of them, is fitted rather than jointed, and braces ending at such a
corner are fitted to each other. So is a corner of tubes in no sketch plane that lies inside an earlier
tube or one of this frame's own sketch tubes. On a Mitre or Butt frame only free ends (ends at
no corner) are fitted, to this frame's tubes before them in drawing order too, whether or not an
earlier Frame Member exists; None never fits. Earlier tubes are never changed. A tube whose end is fitted
so deep that its middle lies inside the earlier tube keeps what comes out of it; one wholly inside fails.

**Overrides.** Under Auto a `corners` entry may name any points where two or more tubes meet:
`members` holds 2 to 1000 distinct ids, the point-to-point tubes ending there included. Auto is in the
override type list only on an Auto frame; an Auto override on another frame builds as that frame's
type, with the override's own member order. While Corner Type is Auto the frame's Flip row is hidden,
and Flip lives on each override row.

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 25, "height": 25, "wall": 2}, "corner": "auto", "fit_ends": true}}
```

**Known limits of Auto** (and of crossings and overlaps under Mitre and Butt). Which tube
gives way at an overlap can be set only through the command's `corners` or `frame.multi_trim`:
these overlaps usually have no sketch point to pick, so the dialog has no row for them. A tube
passing through another at a very shallow angle is cut along the whole of their overlap, so it may lose a long sliver, as cut to fit
does elsewhere. A piece cut by a copy of an earlier tube has that copy's body name in its id, so
renaming the copy unpins the piece's name. One tube cut into three or more pieces by a single other
(a tube through both flanges of a channel or an I section), or by two pieces of one crossing line, gives
its later pieces the same id: only the first one's name is pinned, and the others keep theirs unpinned.
Under Auto, a tube ending on the middle of a tube its plan doesn't fit it to (a sketch tube ending on a
pair's middle, say) touches it end on middle and keeps overlapping it, as Auto always built it. A
piece is found from its name, and the longest tube name it starts with wins, so where tube names
themselves hold ` · ` a piece can be matched to the wrong line. A name pinned to a piece stays in
`names` when only the tube that cut it is removed; it is simply unused, and no other overlap piece is given
that name. A Move, Mirror or Pattern copy of an earlier frame's tube is fitted to only while that frame still has
the original tube: once the original is removed, later frames don't fit to the copy. A copy of one piece of
an earlier frame's split tube may not be fitted to by later frames.
When
a crossing or an overlap first cuts a line, references to that line's far end (an End Cap, a face pick)
stay on the line's first piece, which no longer has that end, and fail with an error. A short piece past a
crossing keeps only what lies past the tube that stays whole: a stub wholly inside that tube is
dropped (it has no body, and its pinned name is simply unused), and a stub that comes out past it is
kept, fitted to that tube's face. Typical X braces have no sketch point where the diagonals cross, so
their Flip can only be set through the command's `corners` or `frame.multi_trim`. A fitted face is
named after the cutting tube's piece id, so its name changes if that piece's id changes. With an off-centre anchor a piece's
crossing end can reach past the whole tube, and a later frame's end landing in that sliver is fitted
to air. A copy of a piece shorter than about twice the crossing tube's width may not be recognised as
a tool, and a later frame's end meeting it is left overlapping. A frame has at most 1000 members,
counting pieces. A flat end meeting a round tube's seam in some layouts is refused by the kernel with
an error. A member's face numbers can change when its line moves, because the kernel's face order
follows placement; faces fitted against a reversed tube can be renamed if its direction flips. A
brace landing at a very shallow angle can leave a small step.

## Multi-trim and the weld gap

**Multi-trim** is how a Frame Member cuts a tube by the shapes of other tubes: the cuts at corners,
copes, ends fitted to a tube's middle or where three or more end, the pieces at crossings, the
notches and splits where tubes overlap, and the ends fitted to earlier frames. It is SolidCraft's own
code in `crates/doc` (`frame_multi_trim.rs`). A frame stays one timeline entry: only the corners you
change are saved (Corner Overrides), and the frame works out every other corner each time it
rebuilds, so sketch edits keep working. `frame.multi_trim` (below) changes one corner of a frame.

**Gap** (`gap`, the row under Fit Ends to Earlier Frames and Coped; a length or an expression,
default 0) leaves room for a weld at every welded end. It must be 0 to 1000 mm: "`gap` must be
0…1000 mm, not 1200 mm". Members that run through keep their length; only the ends that meet another
member move back:

| Joint | With gap g |
|---|---|
| Mitre | each cut plane moves g/2 back along its member, measured square to the mitre plane |
| Butt | the butting member's cut moves g away from the member that runs through |
| Auto's angled butt | the long tube keeps its square cut; the short tube's cut moves g back |
| Butt with Coped | the butting tube is cut by the other tube's real outline grown outward by g |
| An end on another tube's middle, the fitted tubes where three or more end, Fit Ends to Earlier Frames | the end is fitted to the other tube's outline grown outward by g |
| Pieces where tubes cross | their ends are fitted to the outline of the tube that stays whole, grown outward by g |
| Overlap notches and splits | no gap |

A grown tube's own cut ends move out by g as well, so the clearance is g all round, also next to
that tube's cut face. On a 600 × 400 Mitre rectangle of 45 mm square bar, gap 3 makes every bar
3√2 ≈ 4.24 mm shorter (1.5√2 at each end); on a Butt rectangle, gap 2 leaves the two 600 mm bars as
they are and makes the 400 mm bars 4 mm shorter. With Fit Ends to Earlier Frames, a frame's ends stand
off the earlier frames' tubes by its own gap. The row is greyed under None, which cuts nothing. A gap
that leaves a member no length fails as any over-cut member does ("the member on line `id` was cut
away completely"), and an outline that can't grow by the gap (an inside corner rounded less than the
gap, or an arc meeting a side at a sharp corner) fails with "not supported yet: a weld gap of 20 mm
round this member profile…". At gap 0 every frame builds exactly as before.

A corner override's own **Gap** (`gap` on a `corners` entry; empty: the frame's) takes precedence at
its corner: at a crossing it stands that crossing's pieces off, and where three or more end it also
stands the fitted tubes off. An override of tubes that overlap takes none: "A corner override of
tubes that overlap takes no `gap`: an overlap is notched, not welded".

**Member order.** An override is saved as `{"members": [{"line": "l1"}, {"line": "l2"}], "kind":
"butt", "gap": "2 mm"}`: its members in order (a line's id or a pair's `pair1`…), its `kind` (`auto`,
`none`, `mitre` or `butt`) and its `gap` only when it has one. The order is fixed and decides the
build, whatever later sketch edits do to the members' lengths:

- Two members: the first runs through a Butt (coped or not) and is Auto's long tube at a butt or
  angled butt. A Mitre ignores the order.
- Three or more: the first two make the corner, in that order; the rest are fitted in laying order,
  whatever order they are listed in.
- A crossing: the first stays whole and the others are cut into pieces.
- An overlap: the first stays whole and the other gives way.

`kind: auto` is the frame's own type at that corner (Auto's table on an Auto frame) with this order.
A crossing's or an overlap's kind is ignored: only its order counts. Corners nobody overrode keep the
frame's own rules, the frame-wide **Flip** included.

**Flip** on an override is given, never saved. `frame.member`'s `corners`, `frame.multi_trim` and
`timeline.edit` take `flip` on an entry: the members are put in the order the frame's rules give
them now (the member that runs through or stays whole first) and, with `flip: true`, the first two
swapped. An entry without `flip` is saved in the order given. A member may also be written
`{"feature": …, "line": …}` (in `frame.multi_trim` and `timeline.edit`; a new frame has no name
yet): the frame's own name is dropped as the override is saved, and another frame's is refused ("not
supported yet: a corner with a member of another frame"). A design saved by a test build of this
change still opens: a member given as a bare id reads as `{line}`, `corner` as `kind`, `coped_butt`
as Butt with Coped ticked, and a saved `flip` is dropped, the members keeping their written order
(flip such a corner again where it needs it).

**`frame.multi_trim`** (Multi-Trim Corner; no ribbon button, as the dialog's Corner Overrides rows
are its UI) sets, changes or removes one override of a Frame Member in place, as one undo step:

```json
{"command": "frame.multi_trim", "params": {"feature": "Frame Member1", "members": ["l3", "l7"], "kind": "butt", "flip": true, "gap": "3 mm"}}
{"command": "frame.multi_trim", "params": {"feature": "Frame Member1", "members": ["l3", "l7"], "remove": true}}
```

- `feature`: the Frame Member, by id or name.
- `members`: 2 to 1000 line or pair ids (or `{feature?, line}`), in order, which must meet at a
  corner, crossing or overlap of the frame ("a corner override is not at a corner of the frame"). A
  piece of a crossing (`L>X`) is refused, as in `frame.member`: name its line.
- `kind`: `auto` (the default), `none`, `mitre` or `butt`. `flip` and `gap` as above.
- `remove: true` takes away the override of these members (in any order), and the corner goes back
  to the frame's own rules; `kind`, `flip` or `gap` with it is refused. It also removes an override
  whose members no longer meet after a sketch edit.

It replaces the override of the same members (in any order) or adds one, and returns the edited
feature as `frame.member` does.

In the dialog, the frame's **Gap** sits under Fit Ends to Earlier Frames and Coped, and each picked
Corner Overrides row has a **Gap** of its own, empty for the frame's (shown as its hint). Rows listed
with only a bin have none: an overlap takes no gap, and an X-brace crossing's, or that of a corner of
pairs or of a 3D sketch with no sketch point to pick, is set with `frame.multi_trim`. A row's **Flip**
shows ticked when its saved order differs from the order the frame would choose now: after a sketch
edit that changes which member is longer a row can show ticked, and its saved order still holds. A
row whose Flip is left alone goes back in its saved order on OK.

## Mounting plates and bolted end plates

In **Weldments > Create > Mounting Plate**, select a planar member end, including the
ring at a hollow tube's opening. Choose a rectangular or round plate, its dimensions,
thickness, offsets and rotation, and optional four-hole rectangular or bolt-circle patterns.
The plate is a separate body; its holes do not drill the member. Preselection, live preview
and Edit Feature work as for a frame member.

The plate centres on the current face's area centroid and follows its persistent identity.
A lost support produces an error rather than attaching to a nearby member. See the
[Mounting Plate command guide](book/command-guides/frame.plate.md) for `frame.plate`,
placement axes, expressions and hole-pattern limits.

## frame.member

- `sketch`: the frame sketch. `lines`: the curve ids to lay members on (2D or 3D sketch
  lines and circular arcs), each once; left out, every straight line drawn in the sketch: not construction, not
  projected or otherwise linked (a sketch on a face projects the face's edges; they are
  references, not members), and with a length.
- `continuous`: false by default. When true, explicitly selected `lines` form one physical
  member, in path order. Their ends must meet within 0.000001 mm and their tangents within
  0.000001 rad. Open line/arc paths are supported; splines and closed members are refused.
  The section starts with the orientation below and rotates through each bend, carrying that
  orientation into subsequent straights and bends in other planes. A bend whose axis crosses
  the anchored section is refused. This models nominal geometry, not tube deformation or springback.
- `profile` (a built-in profile) or `profile_sketch` (a sketch), one of them. A profile sketch's
  section is its material as nested loops alternate (an outline is material, a loop inside it a
  hole, a loop inside that material again), so two circles make a tube; pick other regions with
  `profiles` as `solid.extrude` takes them. Like `solid.extrude`, only what was drawn counts:
  projected edges are left out unless the drawn curves make no profile. It must be one piece.
- `anchor`: the profile point on the line: `origin` (the default: a built-in profile's centre, a
  profile sketch's origin), `center` (of its bounding box), `centroid`, or a corner or edge middle
  of its bounding box (`bottom_left`, `top`…).
- `locate`: `[x, y]`, the profile point on the line in mm from the built-in profile's own origin (a
  bar's or an angle's bottom-left corner, the middle of a channel's web back, the centre of the
  others; a profile sketch's origin). It takes precedence over `anchor`. A bounding-box anchor moves
  with the section when its size changes; a located point does not, so a member stays attached at
  the same point of its section (`[0, 0]` keeps an angle's heel on the line whatever its leg
  lengths). Edit Feature keeps it.
- **Orientation**: the profile's y points up (world +Z) as far as the line allows and its x is
  square to the line and to y (x × y runs along the line, so on a line along +X the profile's x is
  world +Y); a vertical member's x is world +X. `angle` (an angle expression) turns the profile
  about the line, counter-clockwise looking back from the line's end; `mirror` mirrors it across
  its y axis first. A line leaning less than 10⁻⁶ rad from vertical counts as vertical, so
  rounding in a 3D sketch never turns a post a quarter turn.
- `up`: `X`, `-X`, `Y`, `-Y`, `Z` or `-Z`, the world axis that is up for the profile (default `Z`,
  saved only when it is not). A part drawn with Y up takes `Y`: its members need no `angle`, and
  lines along Y are its vertical members, their x along world X (world Y when X is up).
  Under Auto, a member whose section Auto lines up (one in a sketch plane, or in a chain of
  members in one plane) takes that plane's normal as up instead (see Auto).
- Bodies are named after `body_name`, or the standard size (`RSC 203x89`), the profile
  (`Rect Tube 40x40x3`) or the profile sketch, then numbered on past names already taken (`(2)`, `(3)`…). Each member's name is kept
  with its line (the feature's `names`), so deleting or adding a line, or deleting another
  feature, leaves the other members' names alone; `body.rename` renames a member on its line.
  `names` (`{line id: name}`) gives names to keep, as Edit Feature (`timeline.redefine`) passes
  them unless the edit changes `body_name`.
  A continuous member uses `names: {"$path": "Main hoop"}` so its name survives reversing
  the path or changing the first selected segment.

- `groups` (`[{lines, anchor?, locate?, angle?, mirror?, body_name?}]`): several groups of lines
  in the feature's one profile, each placed and named its own way. Each group lists its lines, and
  no line is in two groups. With `groups`, give `lines`, `anchor`, `locate`, `angle`, `mirror` and
  `body_name` in each group, not beside them. A feature with one group saves as before, so designs without
  groups don't change.

### A bent chassis tube

Draw a line from `(100, -200)` to `(100, 0)`, a quarter-circle arc with centre `(0, 0)` from
`(100, 0)` to `(0, 100)`, and a line to `(-300, 100)`. With those curve ids `l1`, `a1`, `l2`:

```json
{"command":"frame.member","params":{"sketch":"Chassis","lines":["l1","a1","l2"],"continuous":true,"profile":{"kind":"round_tube","diameter":20,"wall":2},"body_name":"Main hoop"}}
```

The result is one tube, with two straight lengths of 200 and 300 mm and a 100 mm centreline
bend radius. Its nominal centreline length is `500 + 50π` mm. Grouping three separate
members or combining their solids does not record this continuous manufacturing path.

Bend-list output and flattened round-tube cope DXF templates are subsequent fabrication
steps tracked under #4003. A continuous member does not yet produce either export.

Every built-in profile is SolidCraft's own parametric definition, built in code
(`solidcraft_doc::frame`). Standard steel sizes (below) fill in a profile's dimensions from a
published table; for other stock you give the dimensions of what you buy. Any other section, a
vendor's aluminium extrusion for example, comes from a sketch: draw it or insert the vendor's DXF.

## End caps

**End Cap** (WELDMENTS › CREATE, `frame.end_cap`) closes open ends of members with plates, one
separate body per end, named after the member (`Post End Cap`). Pick the end faces in the view:
each must be a flat open end of a frame member, square to its line: an uncut end, or the through
member's end of a Butt corner, coped or not. The butting member's end and a cope face are refused ("not
an open end of the member"), as is a mitre ("not square to the member's line").
Curved or multi-piece continuous members are refused until End Cap supports their transported
terminal frames. A continuous member consisting of one straight line retains both open ends.

- The plate covers the outside of the profile: on a closed section (a square or rectangular
  tube, a pipe, a bar) it is the profile's outer outline; on an open section (an angle, a
  channel, a beam, an extrusion with slots) it is the profile's outside edges with their rounds
  and chamfers, and each inward stretch (a slot, between flanges) is closed straight across
  along the rectangle round the profile, so the whole end is closed.
- **Offset** sets the plate in from that outside edge: tick it, then give the inset as a
  **Ratio of thickness** (0.5 puts a 6 mm cap 3 mm in from the edge) or a **Length**. Unticked,
  the plate is flush with the outside of the profile. A rounded corner keeps its rounding less
  the inset; a corner rounded less than the inset becomes sharp.
- **Thickness** is the plate's thickness. **Outward** puts it past the end, so the frame gets
  longer by the thickness; **Inward** cuts the member back by the thickness and puts the plate
  where the end was, so the frame keeps its length.
- **Corner Chamfer** cuts each outside corner along both edges, measured from where the edges meet. On a
  rounded corner it must be at least the rounding, which it cuts away; a round cap has no corners
  to chamfer.
- Caps follow the member: change its profile, its line or the frame sketch and they move and
  resize with it. Renaming the member keeps them. A cap whose member or face is gone reports an
  error instead of picking another end.
- Caps follow any outline of lines and arcs. On an open section the chamfer cuts the
  plate's outside corners. An inset that leaves no plate is refused, as is a chamfer longer than its
  edges allow.

```json
{"command": "frame.end_cap", "params": {"faces": [{"face": [18.5, 0, 300], "body": "Post", "index": 7}], "thickness": 6}}
{"command": "frame.end_cap", "params": {"faces": [{"face": [18.5, 0, 0], "body": "Post", "index": 6}], "thickness": "cap_t", "offset": 2, "chamfer": 4, "direction": "inward"}}
```

`faces` are picks as the view makes them: a world point on the face, its body and face index
(`path` for a face in another occurrence). Give the inset as `offset` (a length) or `ratio`, not
both. **Edit Feature** reopens the dialog with the faces and values.

## Rotate Member

**Rotate Member** (WELDMENTS › MODIFY, `frame.rotate_member`) turns one member about its own line
after the Frame Member is made: pick a flat side of the member, then give the **Angle**, or pick a
line for **Parallel To** and the side is turned to lie parallel to it. The member spins about the
centre of its profile's bounding box, so it stays where it is.

- `angle` is the turn to, counter-clockwise looking back from the line's end, measured from the
  profile as the Frame Member laid it (0 leaves it as laid). It is a number or parameter
  expression, so a parameter can drive it.
- **Parallel To** is worked out once, when you pick the line, and stored as that number. It is not
  a lasting constraint: moving the picked line afterwards does not turn the member again. The line
  may be a straight edge, a sketch line or an axis; a line along the member, or a curve, is refused.
- A side can lie parallel to a line in two ways, 180° apart. The command takes the one nearest the
  member's current turn; to get the other, type the angle yourself and add 180.
- Each OK adds one **Rotate** step to the timeline after the Frame Member. A later step for the
  same member wins over an earlier one. A step whose angle does not evaluate reports its error
  alone, and the member stays as the steps before it left it.
- Names don't change: the member keeps its body name, and its end caps and other references follow.
- Members inside components are not supported yet: the command refuses a member in another
  component or in a placed occurrence.

```json
{"command": "frame.rotate_member", "params": {"face": {"face": [20, 0, 150], "body": "Post", "index": 4}, "angle": "30 deg"}}
{"command": "frame.rotate_member", "params": {"face": {"face": [20, 0, 150], "body": "Post", "index": 4}, "parallel_to": {"sketch": "Ref", "curve": "ref"}}}
```

`face` is a pick as the view makes it (a world point on the face, its body and face index);
`parallel_to` is a `{sketch, curve}` line or an edge or axis pick. Give `angle` or `parallel_to`;
when both are given, `parallel_to` wins. **Edit Feature** reopens the step with its angle.

## Trim/Extend

General Trim/Extend currently accepts unjointed Frame Members: **None**, without corner overrides,
**Fit Ends** or **Coped**. It refuses members made with Auto, Mitre, Butt, overrides, fitted ends
or copes, both as members to trim and as member targets. Rebuilding their original stock would
discard their existing cuts. Their ordinary Frame Member and Multi-Trim Corner operations stay
available. A planar face on such a member can still be used as a plane target.

**Trim/Extend** (WELDMENTS › MODIFY, `frame.trim`) moves the ends of members to what they meet:

- **Another member** (a butt joint): the end stops flat against that member's side. The side is
  worked out from the other member's line and profile, not from a face of its body, so the joint
  follows sketch and parameter edits: widen the frame and the rails between the posts grow with it.
- **A plane or a planar face**: an origin or construction plane, or a flat face of any body. A
  face that slants across the member cuts its end at that slant. A face is attached as a sketch on
  it is, so the trim follows the face: make the plate under a post thicker and the post stands on
  its new top.

A member trims or extends alike: an end short of its target grows to it. Each member keeps its
body and its name, and later trims see it as it now is (trim the cross members to rails that were
themselves trimmed to the posts).

```json
{"command": "frame.trim", "params": {"members": ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"], "to": ["Post", "Post (2)", "Post (3)", "Post (4)"]}}
{"command": "frame.trim", "params": {"members": ["Brace"], "to": {"plane": {"face": [0, 200, 500]}}, "gap": "1.5 mm"}}
```

- `members`: the members to trim, each a member's body name or `{feature, line}` (its Frame Member
  feature and its line's id). A trim keeps them by feature and line, so renaming a member or its
  feature keeps the trim.
- `to`: what to trim them to, one or a list: a member (its body name, or `{member, side}` with
  `side` one of `right`, `left`, `top`, `bottom` of its profile; by default the side the trimmed
  member meets), or `{plane}` as `sketch.create` takes it (`XY`, a construction plane,
  `{origin, normal}`, or `{face: [x, y, z]}` for a planar face, with `body`, `index` and `path` for
  the exact face picked).
- `end`: `auto` (the default: for each target, the end of the member nearer it), `start` or `end`.
- `gap`: the weld gap between each end and its target, a length expression (default 0, at most
  1 m).

A member meets a member target when the line through the middle of its profile passes through the
target's profile and along the target's length; a plane it crosses. With several targets, each
moves the end it meets, so one trim can butt every rail of a frame against its posts. It is refused,
with the reason, when a member meets none of the targets, when two targets would move the same end
(trim them in separate features), when a chosen side faces away from the member, and when the
member's body was changed after it was made (a hole drilled in it, say): a trim rebuilds the member
from its line and profile, and would lose that change, so trim it before the features that change
it. A changed member is also refused as a member target, because its sketch line and profile
may no longer describe its actual side. Use a planar face target to follow the modified body.
This includes symmetric rotations whose outline stays the same and bodies transferred into
another component after the member was made. Members of another component
are not supported yet.

### The Trim/Extend dialog

- **Members**: pick the members to trim (their bodies or faces of them).
- **To**: pick members, planar faces or planes. A face of an unmodified member selects that
  member; a face on a modified member is kept as a planar-face target.
- **End** (Nearer End, Start, End) and **Gap** as above.
- **Edit Feature** reopens the dialog with the trim's picks and values. Targets a pick can't show
  (a member's chosen side, a face, a plane given by its origin and normal) are listed as **Keep:
  n more targets**, kept while ticked; untick it to drop them all and pick again.

## Built-in profiles

A profile is JSON with a `kind` and its dimensions, each a number in the design's units or a
length expression over its parameters (`"tube_wall"`, `"2 in"`): a member follows a parameter
change like any feature. Dimensions marked *optional* default from the profile's size when left
out, made smaller where a thick section leaves less room for a radius;
`frame.profile.properties` returns the profile in millimetres with every default written out.

| Kind | Required | Optional (default) |
|---|---|---|
| `bar` | `width`, `height` | `corner_radius` (0) |
| `round_bar` | `diameter` | |
| `rect_tube` | `width`, `height`, `wall` | `outer_radius` (2 × wall), `inner_radius` (outer − wall) |
| `round_tube` | `diameter`, `wall` | |
| `angle` | `width` (leg along x), `height` (leg along y), `thickness` | `root_radius` (thickness), `toe_radius` (thickness / 2); together at most the shorter leg's inside length |
| `channel` | `height`, `width` (flange length), `web`, `flange` (thicknesses) | `root_radius` (web, at most the room in a flange and half the web's inside height), `toe_radius` (0), `flange_slope` (0: parallel flanges) |
| `i_beam` | `height`, `width`, `web`, `flange` | `root_radius` (web, at most the room in a flange and half the web's inside height) |

The profile's bounding box is centred on its origin, x across and y up. Angles have their heel at
the bottom left; channels open towards +x.

`flange_slope` is an angle (a number of degrees, at most 20) that tapers a channel's flanges,
thicker at the web, as on taper flange channels: `flange` is then the thickness at the middle of a
flange's inside length, between the web and the toe, as their tables give it.

## Standard steel sizes

`{"size": "RSC 203x89"}` is a profile too: the named size with the dimensions its standard
publishes, given alone (a size with dimensions is refused). Spacing, case and `×` don't matter,
a channel's mass may follow (`RSC 203x89x30`), `EA` or `L` name equal angles, and a square named
as an RHS is found as the SHS it is. The design saves the size's dimensions beside its name, so it
keeps its section whatever later tables say, and members are named after the size.

| Family | Kind | Standard | Example |
|---|---|---|---|
| RSC | `channel`, 5° flange slope | BS 4-1:1993 taper flange channels (withdrawn) | `RSC 203x89` |
| PFC | `channel` | BS 4-1:2005 parallel flange channels | `PFC 200x90x30` |
| RSA | `angle` | BS EN 10056-1 equal angles | `RSA 50x50x5` |
| SHS, RHS | `rect_tube`, corners 1.5 × wall outside and 1 × wall inside | BS EN 10210-2 hot-finished | `SHS 100x100x5`, `RHS 200x100x6.3` (depth × width) |
| CHS | `round_tube` | BS EN 10210-2 hot-finished | `CHS 42.4x3.2` |

`frame.profile.list` lists every size by name with the table its dimensions were read from; each
size's built section is within 0.5 % of the area that table gives.

Dimensions are refused, with the dimension named, when they are not finite, are outside
0.01 mm–10 m (radii: 0, or 0.01 mm–10 m), or do not fit: a wall of half the tube or more, a radius
given larger than the room for it.

## Commands

- `frame.profile.list` `{family?}`: every kind, its dimensions and a working example; and the
  standard sizes by name, family by family.
- `frame.profile.properties` `{profile, density?}`: the section, worked out exactly from its lines
  and arcs: `area` (mm²), `centroid` (mm from the bounding box centre), `ixx`, `iyy`, `ixy` (mm⁴
  about the centroid), `width`, `height`; with `density` (kg/m³), `mass_per_m` (kg/m); `size` when
  the profile is a standard size.
- `frame.profile.insert` `{profile, at?, anchor?, angle?, fixed?, sketch?}`: draws the profile into
  the sketch being edited, its `anchor` point (`center`, `centroid`, or a corner or edge middle of
  the bounding box such as `bottom_left`) on `at`, turned by `angle` radians. The curves are locked
  (`fixed`, default true) so solving the sketch keeps the shape. The outline and each hole are
  separate sketch regions: extrude the material region (pick it with `profiles: [{point}]`).

```json
{"command": "frame.profile.insert", "params": {"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}, "at": [0, 0], "anchor": "bottom_left"}}
```

## Gussets

**Weldments › Create › Gusset** (`frame.gusset`) adds a separate reinforcing plate between two planar support faces. Select the faces in order, enter the two leg lengths and thickness, and use the preview to check placement. A triangular outline uses zero outer clipping; **Clipped Tips** cuts equal distances from the two outer tips. **Corner Relief** removes the inside vertex, leaving room at the existing weld corner. The plate is named **Gusset**, then **Gusset (2)** and so on, unless you give it a name.

The legs follow the selected faces into their outward-facing joint space. This also supports non-right-angle joints. The three thickness-placement icons show the plate on the negative side, centred on the gusset plane, or on the positive side. Click an icon to update the live preview; the selected button stays highlighted and its tooltip names the placement. Positive direction is the cross product of the first and second support normals; swapping supports reverses it. **Joint Offset** moves the plate along that direction from the centre of the faces' common span.

```json
{"command":"frame.gusset","params":{"supports":[{"body":"Post","index":2,"face":[20,0,100]},{"body":"Rail","index":5,"face":[100,0,40]}],"leg1":"80 mm","leg2":"100 mm","thickness":"6 mm","relief":"10 mm","clip":"0 mm","placement":"center","offset":"0 mm","body_name":"Joint plate"}}
```

Face indices in this example are illustrative: obtain the current exact picks from selection or `model.faces`. The saved feature holds persistent face identities, not those transient indices. Length numbers use the design unit; picked point coordinates are world millimetres. Every length can instead be a parameter expression.

The entire contact strip must fit on material on both finite faces. Oversize legs/thickness, holes crossing a contact strip, parallel faces and lost or replaced supports return an error. The supporting bodies stay separate and unchanged; the gusset is not a weld or a strength calculation. The first tool requires planar faces. Curved/coped supports and fastener patterns remain separate work.

Edit Feature restores both exact picks and dimensions; preview, undo/redo and save/reopen use the same command. Frame-member supports also retain the producing sketch line, so another line cannot silently take over an attachment through a reused body name.
