# Weldments: profile library, placement points, corners and the cut list

Status: proposal, not adopted. Nothing below is built unless it says so. Related: #4003 (frames
and weldments), roadmap item M15. What has landed is described in [frames.md](../frames.md).

This note proposes how the rest of #4003 fits together: a profile library you set up once, named
placement points on every profile, member groups whose corners are worked out together, weld
gaps, trims, end caps, gussets, welds and a cut list. It describes behaviour from general
fabrication practice and public CAD conventions. No other product's text, files, tables or
templates were used, and none may be in what gets built.

## Why change the shape of the feature

Today `frame.member` makes one body per sketch line. Each body is an extrusion cut square at both
ends of its line. A feature knows only its own lines, and two features know nothing about each
other. So members overlap at every corner, and nothing records how two members should meet.

A fabricator thinks about a frame differently:

1. **The stock comes first.** A shop keeps a small set of sections: 40×40×3 SHS, 50×25×2.5 RHS,
   a vendor's 45×45 aluminium extrusion. You choose from these sections by name. You don't type
   dimensions into every feature.
2. **Where the section sits matters.** You set the section on the sketch line by a point that
   means something: the inside of an angle's heel, a channel's back face, a tube's top outer
   corner. A corner of the bounding box is often not that point.
3. **The corners are where the work is.** Each joint is mitred, or one member runs through and
   the other butts against it. Each joint has a weld gap, and some need a cope. A member's
   cut-list entry comes from its corners: its length and both end angles.

This proposal builds those three ideas into the data model, so corners can be exact, can be
edited and follow parameter changes.

## Goals

- A **profile library** per design, plus a personal library carried between designs, whose
  entries members refer to by id. Editing an entry updates every member that uses it.
- **Placement points**: named points on each profile, generated automatically for built-in
  profiles and named by the user on sketch profiles. You can step through them in the dialog.
- **Member groups** in one feature. Corners are worked out automatically between the members of
  that feature, both within a group and between groups.
- **Corner treatments**: none, mitre, butt and cope. Each has a weld gap. Defaults apply per
  group and between groups, and a single corner can be overridden.
- **Trim/Extend** of members to faces, planes and other bodies, as its own feature.
- **End caps, gussets and welds.**
- A **cut list** that groups identical members by exact geometry and gives quantity, length,
  end angles, material and mass. It can be exported.
- Everything is a command with input and output schemas. Every model decision uses exact
  geometry. Hostile input is refused with a message and never panics.

## Non-goals

- Shipping size tables from a standards body or vendor. The ground rules forbid it. Users build
  their own library, or insert a vendor's DXF, as they do today.
- Curved members (rolled tube along arcs and splines). The model below leaves room for a curved
  segment kind, but this proposal covers straight segments only.
- Structural analysis. The cut list gives section properties and mass. Loads are out of scope.
- Nesting stock lengths (how many 6 m bars a job needs). Listed under "Later".

## Terms

New ideas need glossary entries (`docs/book/src/reference/glossary.md`), one term per idea. The
glossary already has *frame profile* for the section a member follows. This proposal adds:

| Term | Meaning |
|---|---|
| *profile library* | the named frame profiles a design (or a person) keeps for members to use |
| *library profile* | one entry in it: a frame profile, its placement points and its stock data |
| *placement point* | a named point of a frame profile that can sit on a member's line |
| *member group* | segments of one member feature that share a profile, placement and corner defaults |
| *junction* | a place in the frame sketch where member ends meet each other or meet a member's span |
| *corner treatment* | how the members at a junction are cut: none, mitre, butt or cope |
| *through member* | at a butt or cope, the member left uncut that the others stop against |
| *weld gap* | the space left between two members' cut faces at a junction |
| *cut-list item* | identical members (or other bodies) counted together, with their quantity |

`Anchor` stays as the saved key name for compatibility. The label becomes *placement point*.

## 1. The profile library

### What an entry holds

Built in step 1 (`crates/doc/src/frame_library.rs`):

```rust
pub struct LibraryProfile {
    /// Never reused in the design (`lp1`, `lp2`...): what members refer to.
    pub id: String,
    /// Unique ignoring case: `SHS 40x40x3`.
    pub name: String,
    /// Up to three levels it is filed under in pickers, outermost first: `["Steel", "SHS"]`.
    pub category: Vec<String>,
    pub section: LibrarySection,
    /// Placement points it names, beyond the common and generated ones.
    pub points: Vec<NamedPoint>,          // {name, at: [x, y]} in the profile's own mm
    pub default_point: Option<String>,    // none: origin
    pub material: Option<String>,
    pub description: Option<String>,
    pub stock_length: Option<Expr>,
}

pub enum LibrarySection {
    BuiltIn { profile: ProfileSpec },
    /// Regions copied from a sketch's section, held by the entry.
    Drawn { regions: Vec<Region2> },
}
```

`Drawn` holds the copied regions, not a `SketchId`, so an entry can be exported, imported into
another design and kept in the personal library. A timeline sketch can do none of these. A member
made from a timeline sketch keeps working as it does today. A drawn entry's named points are
copied positions too: `{name, sketch_point}` reads the point once, when the entry is made or edited.

### Placement points

Every profile has the points `Anchor` already offers: origin, centre, centroid, and the corners
and edge middles of its bounding box. Each built-in kind also generates the points a fabricator
uses, worked out from the exact dimensions; inside corners are where the faces meet before any
rounding. [frames.md](../frames.md#placement-points) lists them. A library profile adds the points
it names.

A member feature saves `point` (a name), used instead of `anchor` when given. Without it, a
library profile's members sit on its default point unless `anchor` is given; other members sit on
`anchor`. A mirrored profile keeps its bounding-box points on the mirrored outline, while the
kind's and entry's points move with the material.

### Where libraries live

| Library | Stored in | Notes |
|---|---|---|
| Design library | the design, key `frame_profiles` | Self-contained: a design opens identically on any machine. |
| Personal library | preferences, like the personal Hole presets ([hole-presets.md](../hole-presets.md)) | Shown alongside the design library in pickers. |
| Library file | `solidcraft-frame-profiles/1` JSON | Import and export, so a shop can share its stock list. |

Choosing a personal entry in a member dialog copies it into the design library first, with a new
id if its id is taken. A design never depends on someone's preferences. If the copy matches an
existing design entry exactly, the existing entry is reused. (Step 2; step 1 has the design library
only.)

### Rules

- Editing an entry rebuilds every member that uses it.
- Deleting an entry that members use is refused. The refusal names the features that use it.
  `frame.library.replace` moves those members to another entry first.
- Names are unique ignoring case. Ids are never reused within a design.
- Limits, as with other user lists: 1000 entries per library, 128 placement points per entry,
  names of up to 96 UTF-8 bytes.

### Commands

| Command | Does |
|---|---|
| `frame.library.list {scope?: design\|personal\|all}` | entries, their categories, points and properties |
| `frame.library.add {name, category?, size?, profile \| sketch \| from_sketch, points?, material?, stock_length?, scope?}` | adds an entry; `from_sketch` copies a timeline sketch's section in |
| `frame.library.edit {id, ...}` | changes an entry; members follow |
| `frame.library.remove {id}` | refused while members use it |
| `frame.library.replace {from, to}` | moves every member from one entry to another |
| `frame.library.import {path \| json, scope}` / `frame.library.export {ids?, scope}` | library files |
| `frame.profile.points {profile \| library}` | every placement point and its position (mm) |

## 2. Member features with groups

### The data model

`FeatureKind::FrameMember` gains groups and corner settings. The fields it has today become the
single group of an old design (see 4).

```rust
FrameMember {
    path_sketch: SketchId,
    groups: Vec<MemberGroup>,
    corners: CornerDefaults,
    /// Per-junction overrides, keyed by junction id (see 3.1).
    corner_overrides: BTreeMap<String, CornerOverride>,
    /// Kept as today: member names pinned by line id.
    names: BTreeMap<String, String>,
}

pub struct MemberGroup {
    pub id: String,                  // "g1", "g2"..., stable through edits
    pub name: Option<String>,        // what its members are named after
    pub lines: Vec<String>,          // curve ids; each line belongs to at most one group
    pub profile: MemberProfile,      // LibraryProfile { id } (step 1) | Library { profile } (inline) | Sketch { .. }
    pub point: PointRef,             // a placement point name, or one of today's Anchor values
    pub angle: Option<Expr>,
    pub mirror: bool,
    /// Optional: turn the profile so its x axis points along this direction as closely as
    /// the line allows (a sketch line or an edge), instead of the "y up" rule.
    pub align: Option<AlignRef>,
}
```

`frame.member` keeps its current parameters as the one-group shorthand. `frame.members` takes
`groups`. Edit Feature edits the whole feature.

A line used by two groups, or twice in one group, is refused, and the refusal names the line.
Lines in different features never form junctions with each other. To join two members, put them
in one feature. Trim/Extend (section 5) handles members of different features.

### The dialog

- **Profile**: pickers for the library levels (category › category › size) over the design and
  personal libraries, with a **Library...** button that opens the library manager. **Section**
  (pick a timeline sketch) and **Built-in** (dimensions inline) stay available, so nothing that
  works today goes away.
- **Placement point**: the preview shows every placement point of the profile as a dot at the
  first member's start. Click a dot, or press a key to step to the next point. The current point
  shows in the dialog by name.
- **Groups**: a list with **New Group**. Picking lines adds them to the selected group. Each
  group has its own profile, point, angle, mirror and align.
- **Corners**: the default treatment within a group (mitre for a new feature), the default
  between groups, both weld gaps, and which group runs through by default (section 3). These
  change every junction of the feature at once, except junctions with an override. **Reset all**
  clears every override, so every junction follows the defaults again.
- The preview marks every junction, with overridden ones marked differently. Clicking a marker
  opens a small corner panel: treatment, through member, gap, and **Reset** to the defaults.
  Shift-click (or a box select) picks several markers, and the panel then sets them all in one
  go. These are `corner_overrides` entries.

Each control maps to a parameter of `frame.members`. A command call never opens the dialog.

## 3. Junctions and corner treatments

### Finding junctions

When the feature is evaluated, it builds a graph from its members' lines in the frame sketch:

- **Nodes** are member ends. Two ends join when they are the same sketch point, which is the
  usual case when lines are drawn in a chain, or when their positions agree within 10⁻⁶ mm
  (relative to the model size, as other tolerances are). Positions are taken from the solved
  sketch, so this is exact geometry.
- An **end on a span** is an end that lies on another member's line strictly between its ends,
  within the same tolerance.

Junctions are classified:

| Kind | What meets | Default treatment |
|---|---|---|
| L | two ends at a node, not collinear | the group default (within or between groups) |
| I | two ends at a node, collinear | butt with gap (two pieces); mitre is not offered |
| T | one or more ends on another member's span | butt, the spanning member runs through |
| Multi | three or more ends at a node | butt: the first-listed member runs through (see 3.4) |

Lines that cross mid-span (an X) are not junctions. Both members stay whole, and the feature
reports a warning that they overlap.

**Junction ids** have to survive sketch edits, because overrides hang off them. A node's id is the
sketch point id the ends share (`p12`). A tolerance-matched node takes the smallest end point id.
A T junction is `p12@l7`: the end point and the line it lands on. An override whose junction no
longer exists stays in the feature data, so undoing a sketch change brings it back. It is skipped
when evaluating, with a warning that names it.

### Treatments

Every cut is a plane, or for a cope the through member's section, built from the line directions
and the exact profile extents. Each member is first extruded past its line by enough to reach
every cut, then cut with kernel half-space booleans
(`convex_polyhedron` or a planar split). No mesh is used. The extension each end needs is worked
out from its own junction: the profile's largest extent divided by the sine of the angle between
the members there.

Let `a` and `b` be unit vectors from the node along the two members.

- **None**: square ends at the line ends. This is today's behaviour.
- **Mitre** (L only): both members are cut on the plane through the node with normal `a − b`, the
  plane that mirrors one member onto the other. With gap `g`, each cut plane moves `g/2` back
  along its own side of the normal. Lines in different planes (a 3D frame) still have a mitre
  plane. When the two profiles differ, or their turns don't match across the plane, the faces
  don't line up. The mitre is still made, with a warning that names the junction.
- **Butt**: the through member is left as it is. The other member is cut by a plane square to
  its own line at the through member's near face, less the gap. The near face is the extent of
  the through member's profile towards the butting member, worked out exactly from the profile's
  lines and arcs, so a round tube butts against its tangent plane. A butt between members at an
  angle can be **square** (cut square to the butting member, the default) or **flush** (cut on
  the through member's face plane, at an angle).
- **Cope**: the butting member is trimmed by the through member's section, offset outwards by
  the gap and extruded along the through member. The 2D region offset is exact (the sketch
  offset already produces exact offsets), so the cope is exact. This is the joint for tube meeting
  tube, or a channel fitted into a beam.

There is no fixed smallest angle, because the real limit depends on the profile's size and the
member's length, not on the angle alone. Small angles cost no more to compute. What goes wrong
near parallel is the geometry: a mitre face is about the profile's size divided by the sine of
the angle long (a 40 mm tube at 0.01° would need a cut about 230 m long), and booleans with a cut
plane almost parallel to the member's faces are poorly conditioned. So the rule is:

- Two ends whose lines are within 10⁻⁶ rad of each other are collinear: an I junction, as
  `VERTICAL_TOLERANCE` already treats near-vertical lines.
- Otherwise any angle is allowed, as long as each cut stays within its own member. A cut that
  would reach past the member's other end, cross that end's cut, or remove the member entirely
  is refused. The refusal names the junction and suggests butt or None.

### Defaults and gaps

```rust
pub struct CornerDefaults {
    pub within_group: Treatment,   // default: Mitre
    pub between_groups: Treatment, // default: Butt
    pub gap_within: Option<Expr>,  // default: 0
    pub gap_between: Option<Expr>, // default: 0
    /// Between groups, which group runs through: earlier listed (default) or later listed.
    pub through: Through,
    pub butt_cut: ButtCut,         // Square (default) | Flush
}

pub struct CornerOverride {
    pub treatment: Option<Treatment>,
    /// Line id of the through member (butt, cope, multi).
    pub through: Option<String>,
    pub gap: Option<Expr>,
    pub butt_cut: Option<ButtCut>,
}
```

Gaps are length expressions, so `weld_gap` can be a parameter.

### Junctions where three or more members meet

At a multi junction, members are ranked by group order, then by line order within the group. An
override's `through` picks the top-ranked member instead. That member runs through. The next one
butts or copes against it. Each one after butts or copes against everything ranked above it at
that node. A mitre between the top two with the rest butting against both is a later option
(see Later).

### Corner results

Evaluating records, for each member end, the treatment, the cut plane (or cope) and the junction
id, as data on the body's feature result, not in the saved design. The cut list (section 7),
Trim/Extend, gussets and welds read these records instead of measuring geometry again.

## 4. Saving, old designs, and the format change

- New design keys: `frame_profiles` (design library). In the `FrameMember` feature: `groups`,
  `corners`, `corner_overrides`.
- One file per PR in `crates/doc/src/format/pending/`. The PR that adds groups includes a `step`
  that rewrites an old `FrameMember`: its `lines`, `profile`, `anchor`, `angle`, `mirror` and
  `body_name` become one group `g1`, with `corners.within_group` and `corners.between_groups` set
  to `none`. An old design therefore rebuilds exactly as before, because corner treatments
  change existing UI defaults (AGENTS.md). New features get the mitre and butt defaults.
- A design saved with groups, opened by the last release: that release warns about the newer
  format and keeps what it cannot read, as with any pending change.
- The personal library is a preference, not part of the design format.

## 5. Trim/Extend

A feature of its own, `frame.trim`, for joints the junction graph doesn't cover: members of
different features, members meeting a plate or a floor, or a tube fitted to a curved body.

- **Members**: the member bodies to trim or extend, each with the end to work on (picked near
  that end, or `start` or `end`).
- **Boundary**: planar faces, construction planes, or bodies.
- **Mode**: *flat* (cut on the boundary's plane; for a body boundary, the plane of its nearest
  face square to the member, refused when that face isn't planar) or *cope* (cut by the
  boundary body).
- **Gap**, an expression. **Allow extend**: a member that stops short of the boundary is
  lengthened to meet it, refused when the boundary is not ahead of that end.
- The trimmed bodies keep their names, and their end records (3.5) are updated, so the cut list
  shows the new angles.

## 6. End caps, gussets and welds

**End caps** (`frame.end_cap`): pick open member ends of hollow profiles. Settings: thickness,
inset from the outer edge, chamfered or rounded corners, outward (adds length) or inward (sits
inside the tube). The cap's outline is the member's end face offset inwards, so it is exact on
mitred and angled ends too. One body per cap, named after the member.

**Gussets** (`frame.gusset`): pick two planar faces of members meeting at a junction. Settings:
triangular or chamfered, leg lengths `d1` and `d2` (and the chamfer for the chamfered kind),
thickness, position across the joint (inside, centred, outside), and a corner clip for weld
clearance. The gusset is a prism of a polygon in the plane square to the two faces' line of
intersection, so it is exact. Refused when the faces are parallel.

**Welds** (`frame.weld`): pick faces or edges in pairs. Settings: type (fillet first), leg size,
continuous or stitch (length and pitch), one or both sides. A weld is recorded with its
reference faces and drawn as a display bead (a mesh is fine for display). It doesn't change the
solids. Welds count towards mass. A weld's volume is worked out exactly from its cross-section (a
fillet is leg² / 2) times its length (for a stitch weld, the welded length), not from the display
bead, and its density is the weld material's (by default, the material of the members it joins).
Welds appear in the cut list as a total weld length and mass per size, are included in the
weldment's mass and physical properties, and are the data drawings will use for weld symbols
later.

## 7. The cut list

### What it lists

- Every member body from member features, after Trim/Extend.
- Optionally every other solid in the weldment (plates, caps, gussets), with its bounding box and
  thickness when it is a flat plate.

### Grouping

Members are grouped into one cut-list item when, compared exactly with the design tolerance:

- same profile (library id; or same resolved dimensions or section for inline and sketch
  profiles), and the same mirror state,
- same length,
- same end treatment at each end: the cut angle, the cut direction relative to the profile's x
  and y, and cope or not.

Two members that match after swapping their ends are the same item. Mirror-image pairs (a left
and a right piece) are separate items by default. An option lists them together as "2 off,
opposite hand".

### Columns

Item, quantity, description (the library entry's template, filled in), length, angle 1, angle 2
(each with the face of the profile it slopes towards), material, mass each, total mass, total
length per profile. The bottom line totals: mass, and length per library profile, which is what
a shop orders.

- **Length** is the member's longest extent along its line, from the exact cut planes. The
  shortest extent is also available, which is what saws mark for mitres.
- **Mass** uses the profile's exact area (`frame.profile.properties`) times length for square
  ends, or the body's exact volume once ends are cut, times the material density.

### Stable item numbers

Item numbers are kept in the design (`cut_list.items: {grouping key: number}`), so adding a member
doesn't renumber the shop's drawings. A new grouping takes the next free number. `frame.cut_list.renumber`
starts again.

### Commands

| Command | Does |
|---|---|
| `frame.cut_list {feature?, include_other_bodies?}` | the items, their members and totals as JSON |
| `frame.cut_list.export {format: csv\|json, path?}` | writes the list |
| `frame.cut_list.renumber {order?: profile\|length\|current}` | renumbers items |
| `frame.cut_list.set {item, description?, properties?}` | user text per item, kept by grouping key |

The Browser shows a **Cut List** folder under the weldment with one row per item. Selecting a row
selects its bodies. Drawing tables are a later step that reads the same data.

## 8. Exactness, limits and safety

- Every decision here (junctions, cuts, extents, groupings, lengths, angles) uses solved sketch
  geometry, exact profile curves and kernel booleans. Nothing is added to
  `xtask/mesh-decisions.txt`.
- Limits: `MAX_MEMBERS` (1000) per feature stays, plus 64 groups per feature and one override per
  junction. Library limits as in section 1.
- Every refusal names the line, junction, group or library entry involved. Errors are `Result`s.
  `hostile_params_never_panic` covers each new command.
- Rebuild cost: junction finding is O(n log n) over member ends using a spatial sort, so it
  doesn't need O(n²) comparisons for the 1000-member maximum. The cuts of a member depend only
  on its own junctions, so members can be built independently (and in parallel where the
  performance skill allows).

## 9. Delivery plan

Each step is one PR with one topic, claimed with a draft PR. Each step that saves something new
adds its one pending format file.

1. **Library and placement points** (built on branch `frame-profile-library`). `LibraryProfile`,
   `frame_profiles`, the `frame.library.*` commands and `frame.profile.points`, generated placement
   points for every built-in kind, named points on drawn sections, `MemberProfile::LibraryProfile
   { id }` and `point` on `frame.member`, and in the Frame Member window a Library row, a Point row
   and Save to Library. Tests: each kind's generated points against hand-computed positions; edit
   an entry or a parameter it uses, members follow; remove refused while in use; save and reopen.
2. **Personal library and library files.** The preference store, import, export, and copy into the
   design on first use.
3. **Groups and junctions.** `groups`, `CornerDefaults`, the junction graph and its ids, mitre and
   butt (square and flush) with gaps, and the old-design `step`. Tests: a rectangle mitres into
   four members with exact 45° faces and the given gap; a T butts; a 3D corner mitres; old designs
   rebuild unchanged.
4. **Dialog.** Library pickers, placement point dots, groups list, junction markers and the
   corner panel (`corner_overrides`). Screenshots in the PR, as AGENTS.md requires.
5. **Cope**, and multi-member junction ranking.
6. **Trim/Extend.**
7. **Cut list** and its export, stable numbers, Browser folder.
8. **End caps.** 9. **Gussets.** 10. **Welds.**

Steps 1 and 2 don't depend on 3. Steps 6–10 depend on the end records from step 3.

## Later

- Mitred top pair with the rest butting at multi junctions.
- Merging members joined by zero-gap mitres into one body (a bent-tube look), still listed as
  separate cut-list items.
- Curved members along arcs and splines, with their developed length in the cut list.
- Stock-length nesting: bars needed per library profile, with kerf.
- Weld symbols and cut-list tables in drawings.
- Holes and slots patterned along a member (bolt lines), which follow the member's length.

## Decisions

1. **Material.** A library entry's material is assigned to each member body it makes, so mass,
   physical properties, render and the cut list agree. A material set on one member by hand
   wins over the library's and survives rebuilds. The cut list always shows the body's actual
   material, so it never contradicts the mass.
2. **Corner defaults.** New member features, `frame.member` included, mitre within a group. Every
   junction can be changed at once from the dialog's Corners defaults, or one at a time (or a
   selection of several) by picking junction markers in the view. Old designs migrate to none.
3. **No fixed smallest angle.** A corner is refused only when its cut can't be made within its
   member (section 3); collinear means within 10⁻⁶ rad.
4. **Welds count towards mass**, from their exact cross-section and length (section 6).
