# Sheet metal names: sheets report history, the engine names

Applies to the surface-first sheet model on `fix-6748-develop` (#6748, #9245). The plan of record
for names is [`persistent-naming-proposal.md`](persistent-naming-proposal.md) (#7990); read the two
together. Status: this is the method we are implementing now. The branch still carries an older
sheet-only naming layer (`sheet_face_names.rs`'s `FaceNamer`, `sheet:<body>:…` face names,
`<panel>:rim.…` edge names, `tag_panels`' `F<feature>.<k>`); it is being removed, not extended.

## The rule

Sheet code computes no names. A sheet build reports **history**: for each face of the body it
builds, which part of the sheet the face was made for. The central engine (`lineage.rs`,
`history_names.rs`, `name_lookup.rs`, `naming.rs`) turns that into names, exactly as it names an
extrude's or a fillet's faces. One naming system, for every feature.

Two things never decide a name, an ordinal, or which piece keeps an identity:

- **An index or an order.** Not a panel, joint, face or pick index, not creation order, not the
  order a kernel or a boolean lists faces in. A timeline reorder or a cut that removes a panel must
  rename nothing it did not touch.
- **A base.** No panel, sheet or feature is special. Any sheet is a valid holder of information;
  features may join, split or destroy any arrangement of sheets. What must hold is that names stay
  stable and every flat develops valid. A question like "is this piece still joined to its base"
  is not asked.

## What a sheet build reports

For each face, a structured part (never text):

- a panel's two caps: the surface side (the oriented surface's outward side) and the side one
  thickness into the material;
- a rim's side face, with its origin: the sketch curve it lies on, or the edge it was made on;
- a bend zone's inner and outer faces and its two ends;
- a tab's caps, a hole or cut wall, a relief wall.

Panels are identified by a **recipe key**: the feature that made the panel plus what it was made
on. A base flange's panel: its sketch profile. A contour flange's: its sketch line. A converted
sheet's: the source body's face. A flange's, hem's or path flange's: the rim it was made on. A
fold's moving piece: the panel folded and where the piece starts along the fold line's own
direction.

Where pieces could tie, the part carries an **ordinal from the feature's definition**: the
parameter along the sketch curve where a hole wall's piece starts, the distance along a fold line
or a bend's anchor. Never a build or kernel order (#7990 change 4).

The engine compares the report with the previous build's. A face whose part existed before keeps
its name; a face made for a new part is named by the feature that made it. That includes a face a
later feature adds to a panel an earlier one made: it is the later feature's, named after the face
of that panel it was made from (a rim past a bend's end after that bend end, a tab's cap after its
bend's face on the same side). The feature whose build made a face decides; a feature number or
creation order never does. Two small engine extensions carry this, both shaped like what the
engine already has:

- a **grown face**, `F<feature>:<role>:<face it was built on>`, the same shape as today's
  `F<f>:inner:<face>` and `F<f>:blend:<edge>`: a flange's top is named after the rim it stands on;
- an optional **definition ordinal** on a made face, which piece numbering uses before anything
  else.

## The names a sheet's faces get

| Face | Name |
|---|---|
| a base flange's caps | `F<b>:top`, `F<b>:bottom` (an extrude's vocabulary) |
| a rim on a sketch line, on any panel a sketch made | `F<b>:side:<curve>` |
| a flange's caps | `F<f>:top:<rim>`, `F<f>:bottom:<rim>` |
| a flange's far edge and ends | `F<f>:far:<rim>`, `F<f>:end0:<rim>`, `F<f>:end1:<rim>` (`end0` at the joint's anchor origin) |
| a bend zone | `F<f>:bend.inner:<rim>`, `F<f>:bend.outer:<rim>`, `F<f>:bend.end0:<rim>`, `bend.end1` |
| a flange's rim past its bend's end, where a closed corner runs it on | `F<f>:past0:<rim>`, `past1` (`past0` at the joint's anchor origin) when its own feature closed the corner; `F<g>:past0:<bend end>` when a later feature `g` did |
| a tab's caps (bend material riding with one panel) | `F<f>:tab.top:<rim>`, `tab.bottom` when the bend's own feature made the tab; `F<cut>:tab.top:<bend face on that side>` when a later cut freed it |
| a hole or cut wall | `F<cut>:side:<curve>`, pieces told apart by the faces that bound them, else `#k` by where each starts along the curve |
| a relief wall | `F<f>:relief:<edge between the two rims>`, `<f>` the feature whose build cut it |
| a converted sheet's faces | the source body's face names, carried |

Edges are the engine's: `<face A>|<face B>`. A flange or hem pick stores that edge name.

## The flat

The unfolded body keeps the folded body's faces: one face per panel cap, bend zone, tab, rim, end
and wall, coplanar caps sewn along the tangent lines. Each flat face is the folded face of the same
part, so it has the same name, and an unfold followed by a refold renames nothing. The flat pattern
and the DXF are unchanged; they are built from the sheet's regions, not from the body's faces.

Why not one merged flat face: the engine's merge rule today names a merged face after its first
origin by face index (see the list below), and a refold that splits a merged face could only give
its pieces that one name back. One top face waits for the engine pass.

## Folds

A fold given no fixed point decides once, when the feature is created, which side stays: on a
flange the side toward its own bend, else the larger side, as before. It stores that as the side
left or right of the fold line's drawn direction. Every rebuild uses the stored side; an edit that
makes the other side larger flips nothing. A `fixed` point is converted to a stored side the same
way. The fold dialog shows which side stays and swaps it.

## Sketches on sheet faces

A sketch attaches to a sheet face by the engine's name, like any other face. Its frame comes from
the sheet through one central hook: a sheet body's planar face supplies its own sketch frame, so a
sketch drawn on the flat lands where it belongs after a refold.

## Corrections to earlier guidance on #6748

Some of what we wrote on #6748 and in reviews was wrong. If your PR follows it, change it:

1. **"A bend row runs along the bend with the earlier panel's material on the left."** The rule
   keys on the joint's own definition: its `a` side is the panel the bend was made on, and that
   panel's material lies left of the anchor's direction. "Earlier" reads as creation order, which
   a reorder can change. Say and implement: the material of the panel the bend was made on (the
   joint's `a` panel) on the left.
2. **"The base panel's rims are named after the sketch lines."** That privileges one panel. Every
   panel a sketch made reports its rims' sketch curves as origins, and the engine names them.
   Code keyed to a base panel's curves (`base_curves`) goes.
3. **`F<feature>.<k>` panel names, and pick or panel order in names.** Replaced by recipe keys and
   definition ordinals.
4. **Sheet-specific face and edge names** (`sheet:<body>:…`, `<panel>:rim.<rim>:top`). They were a
   second naming system. Sheets report parts; the engine names.
5. **The flat laid out in the first panel's plane** (`places.first()`, develop rooted at panel 0).
   The sheet records its own flat frame when it is made and carries it; a flat never moves
   because a panel was cut away or features were reordered.

## What the central engine still gets wrong, and why it waits

The interface above is final: what a feature reports does not change when the engine is fixed.
These are in how the engine stores, numbers and finds names, and sheets meet each of them:

1. **A merged face is named after its first origin by face index.** A join flush with a panel's
   cap names the result after whichever cap sorts first.
2. **Pieces nothing around them tells apart are numbered by place on the display mesh**, or by
   origin face index for older features. A hole's two halves can swap `#1` and `#2` when the part
   is mirrored. The definition ordinal above fixes this for faces that report one.
3. **A split pick resolves to the nearest piece, or is refused.** A sketch on a flange's top that a
   later slot cuts in two attaches to one piece by its stored point; #7990 change 5 resolves to
   all pieces.
4. **Body identity follows the least face name.** A cut that parts a sheet into two bodies gives
   the first body's name to whichever piece's least face name sorts first.
5. **A face with no history falls back to geometry on the display mesh** (`F<k>:face:<i>`), which
   changes with sizes. That is a mesh read where exact history belongs.
6. **Names are text, re-parsed, and grow.** A return flange on a return flange embeds the whole
   chain and can pass the 512-byte limit on names that commands accept; a reorder can change which
   name a piece qualifier belongs to. #7990 change 3 (structured, one level deep) fixes both.
7. **A reference to an alias finds nothing.** After a join merges a tab into a panel's cap, a
   sketch on the tab is lost rather than following the merged face.

We are not fixing these in the sheet work. Each change touches every feature, one changes the save
format, and others are working in the same code now (#5861, #11359, #11658, #13147). They get one
pass, as their own pull request, in the proposal's order of work, after #9245 merges: first a count
of how every reference resolves, then lineage fallback, ordinals, all-pieces picks, body identity,
and structured names. Until then, feature code must not work around them: no local renumbering, no
local fallbacks, no names built in feature code. A workaround in one feature is one more place the
pass has to find.
