# Face-attached sketches

New face-attached sketches store the selected source body, its creating feature, an exact
face certificate, and the source and author occurrence paths. The sketch's picked origin
and orientation are stored in the source face's canonical plane. Editing the source height
or moving its placement therefore updates the sketch plane while preserving its local
geometry and dimensions.

Supported planar sources are exact Box faces, untapered New Body and Join Extrude caps and
straight sketch sides, and planar faces of immutable STEP imports. Extrude caps also identify their
outer boundary and holes by source curve IDs. Reordering profiles cannot substitute another
profile's cap. A thin extrude's caps are bounded by offset curves the sketch doesn't have, so a
wall built as one region is identified by the curves it follows instead (as its outer boundary,
with no holes). That is the identity the solid caps had when the wall follows the whole profile,
so a sketch on a cap keeps its face when the extrude is made thin, or solid again. A wall along
other curves, or one of several walls apart, is another face: it is attached by its history
name below, and a sketch made on the solid cap reports a lost attachment. A profile drawn over a face sketch's projected boundary (a rectangle
traced over the whole face, or a strip whose sides lie on the face's edges) is identified by
the drawn curves: where a drawn curve and a projected one cover the same edge, the drawn curve
is the source. Two drawn curves, or two projections, over one edge remain ambiguous. Curved surfaces cannot host a planar sketch through this workflow.

Every other flat face is attached by its exact history name (a `lineage` certificate) when it
has one: the persistent name the kernel's face history gives it, which says what made the face
and follows it through every later feature. That covers a Cut's walls and floor
(`F5:side:l1`, `F5:top`), a chamfer face (`F7:blend:…`), each copy a pattern or mirror makes
(`F4:top@F6.2`: the seed's face and the copy's instance), a side wall a Join merged from two
features (named after the first, answering to the other), the pieces a later feature split
a face into (all pieces of one name on one oriented plane are that face), the face Split Body
cuts with a plane (`F4:section` on each part: a construction plane, a sketch line or a plane given
outright, so a sketch on it follows the plane when a parameter moves it), and the faces of a
thickened surface: the face that was kept, the offset (`F3:inner:F2:side:l1`) and the walls on
the surface's edges (`F3:blend:…`). A sketch on any of those follows the thickness. A lineage face's
plane is its current exact plane, measured in a frame that depends on the plane alone: its
origin where the definition origin projects onto it, its x axis the first of the definition's
X, Y and Z axes not steep to it (`|cos| < 0.6`), projected. The construction certificates
above still come first; a lineage certificate is used where none names the face alone. Copies
carry their seed's construction certificate, so a sketch attached to the seed before it was
patterned keeps resolving to the seed: of the faces carrying one certificate, the producing
feature's own face is the one whose name is not a copy's.

Where the pieces of one name no longer share a plane (Offset Face raised one piece of a top a
cut had split), each piece is attached by its own name: the face's, qualified by the faces
around it (`F2:top#[F2:side:l4,F4:side:l4]`). The plastic features (Boss, Lip, Rest, Snap Fit)
keep the history of the body they go on and name their own faces by the part they make
(`F4:post:top`, `F5:rest:side:0`, `F6:arm:top`), so those take attachments too. A side an extrude grew to
overlap a flush body before joining it (its neighbours lengthened past the sketch's corners) is
still named by the sketch line it runs along, so a wall a Join merged from two extrudes' walls
attaches by the first one's name and answers to the other's. A side drawn on several sketch
curves at once (two squares drawn edge to edge leave two coincident lines along the shared edge;
several adjacent profiles extruded together leave one wall along collinear lines) is named by
every curve it lies on, the first in the sketch's order its name and the others aliases: the set
is fixed by the sketch, so one sketch extruded up and down and joined (a logo) takes an attached
sketch on every wall, not only on its tops and bottoms.

Faces whose history is not exact (faces a feature rebuilt without reporting its history),
ambiguous sources, and the pieces of a face split on purpose with Split Face (each piece is its
own face) require an explicit fixed plane instead. The error says why, and the Create Sketch dialog then offers **Use fixed plane**,
which places the sketch on that face's current plane without following later changes. A
nearby or parallel face is never used to repair a new attachment automatically.

The flat faces of a sheet metal body are the exception: they have no construction
certificate, so a sketch on one attaches to its panel by name (`sheet:<body>:<panel>:<side>`)
and finds it again after every rebuild, unfold and refold. The rims (the sheet's thickness
along a panel's straight edge) attach the same way, by the edge they run along
(`<panel>:rim.l3` for a base sketch line, `<flange>:rim.far`, `rim.side0`, `rim.side1`), and so
do the flat ends of a bend (`<flange>:rim.bend0`, `rim.bend1`); a rim's plane faces out of the
panel, its x along the edge and its y across the thickness. Extrude and Press Pull from a sheet
face use the same attachment. If that panel no longer exists the
sketch reports a lost attachment, as above. See the sheet metal guide.

## Create and repair

Select a supported planar face and create a sketch. The desktop selection supplies the exact
body and face index, including its occurrence path. Command callers can use
`sketch.create` with `plane: {face: [x,y,z], body: "world body key", index: face_index}`;
the point is in world coordinates. A point-only face request must find a unique supported
source. An unsupported or ambiguous request fails with an explanation instead of creating
an attachment with weaker identity.

A solid extrude's cap is identified by the curves around its profile, so editing that sketch's
outline (a line that now splits the profile, a side deleted and drawn again, a notch trimmed into
it) leaves no cap with the exact identity. The sketch then sits on the cap of the same extrude, at
the same end, made from the same sketch, on the same body, whose outline shares the most of the
stored curves, at least half of them (a tie is not guessed; a cap whose exact identity is still
on another body is never replaced). It warns, naming the face and the curves
that went or came, until the sketch is placed again with **Redefine Sketch Plane**.

When a source is suppressed, deleted, replaced, or loses its certificate otherwise, the sketch
reports a lost attachment, naming the face it was on (`the top cap of Extrude1 on `Body1``). Its saved plane is only a visible repair guide: it does not produce a
healthy solved sketch or let downstream features build at a substitute face.

Use **Redefine Sketch Plane** to select a new supported face explicitly. The draft can be
reviewed before applying it; cancellation leaves the existing attachment intact. Reattachment
keeps existing sketch geometry, IDs, and dimensions. When the selected source changes, its
previous automatically projected boundary becomes ordinary geometry on the new plane;
it is not replaced by a newly projected boundary. Rebinding to the same certified source
retains its live boundary. Manually created projection links retain their own sources.

Choose an explicit fixed plane to detach the sketch. This preserves its current local
geometry and dimensions and converts only its automatically projected face boundary to
ordinary geometry. Manual projection links remain attached to their own sources. A fresh
fixed-plane sketch has no live face auto-projection.

## Occurrences, pending positions, and files

The selected source occurrence and the sketch's author occurrence are separate identities.
Repeated instances therefore cannot silently switch an attachment to the first placement.
A missing author path also requires explicit repair. Reattachment and creation capture
pending positions only on the involved source and author paths. Failed commands restore
those pending positions; undo restores the previous attachment and placement state.

File format 11 introduced these optional attachments, which format 12 also preserves. Format 16
adds the `lineage` certificate (older readers ask for an update rather than meeting an unknown
kind); anchors saved with construction certificates resolve as before. Format 17 adds piece
names qualified by the faces around them and the sheet metal rim and bend-end names; 0.8.0
asks for an update when it opens a format 17 design. Existing designs with face references
without an attachment retain their legacy geometry and lookup behavior when opened; they
are not silently upgraded. Redefining such a sketch onto a supported selected face creates
the stronger attachment. Unsupported legacy geometry can instead be retained on an explicit
fixed plane. Imported attachments also bind the exact STEP payload digest, item entity, and
face entity: replacing the payload cannot reuse an old attachment merely because a face
index or shape looks the same.

## Profiles generated by face modelling commands

Extrude, Revolve, and Press Pull can generate an internal sketch from a selected planar face. Supported
sources use the same certified attachment and live boundary as an ordinary attached sketch.
For modified faces without a construction certificate, these commands retain their historical
named-face and linked-boundary behaviour through a private compatibility path. Initial capture
uses the selected body and face index. These helper sketches do not have the stronger saved
identity guarantee of a certified attachment. This compatibility path does not change the
requirements for creating an ordinary face-attached sketch.
