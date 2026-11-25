<!-- SPDX-License-Identifier: Apache-2.0 -->

# Booleans

This page describes how solidcore computes the union, difference and intersection of two bodies:
the theory the code rests on, the steps and where each lives, what is decided on the exact surfaces
and what is not yet proved, the refusals, and what is open. It is written for a reader who knows
boundary evaluation and is new to this code, and for anyone stuck on a boolean that drops or keeps
the wrong material. Numbers in brackets are references (end of page). Every statement about the code
names the file it was checked in; paths are relative to `crates/`.

The boolean today is the port: SMLib's `SmMerge` and `SmTopologyIntersector` carried over unit by
unit (`solidcore/src/boolean/`), with the fork's fixes and solidcore's own behind `Kernel::fixes`
flags, all off by default. Its units are checked against their goldens (`plan/units/boolean.*.toml`).
What the boolean takes from surface/surface intersection is in [ssi.md](ssi.md); this page starts
where its §4.6 stops.

Contents:
1. [Purpose and the one rule](#1-purpose-and-the-one-rule)
2. [Scope](#2-scope)
3. [The pipeline](#3-the-pipeline)
4. [Classification](#4-classification)
5. [Coincident and near-coincident faces](#5-coincident-and-near-coincident-faces)
6. [Tangent contacts](#6-tangent-contacts)
7. [Definitions through trimming](#7-definitions-through-trimming)
8. [Failure modes and refusals](#8-failure-modes-and-refusals)
9. [Invariants](#9-invariants)
10. [Status](#10-status)
11. [If you're stuck](#11-if-youre-stuck)
12. [References](#12-references)

---

## 1. Purpose and the one rule

**A boolean's result is the regularized set operation on the two solids, decided on their exact
surfaces, or a typed refusal.** A solid is the closure of its interior; the regularized operation
(A op B taken on interiors, then closed) drops the dangling faces, edges and points an ordinary set
operation leaves where the two boundaries touch [1, 2]. The boundary of the result is made of pieces
of the two boundaries, each kept or dropped by where it lies against the other solid: inside, outside
or on [3]. Boundary evaluation is that classification, and the boolean is right exactly when every
piece's class is.

The rule follows the contract of [definitions-and-certificates.md](definitions-and-certificates.md):
a face's surface is its definition S, and its net B is only a certified approximation within ε. So:
- **Classification reads S, never a mesh.** Surface hits and explicitly surface-solved points are
  roots on the surfaces themselves. `classify_remaining` may use an evaluated operand-edge point
  only as a ray origin; that origin is not itself a surface solve. A display mesh decides nothing
  ([layers.md](layers.md) §3).
- **A region is dropped or kept only by its class.** No piece is removed because it is small,
  because a sample missed it, or because a tolerance was widened.
- **A case the code cannot decide is refused by name**, never guessed.

The rule is the target. §4 and §10 say where the port meets it: points are solved on S, and the
search boxes are grown by each definition's ε; the decisions themselves (which side of a face, which
sector of an edge) are made by Newton roots and angle thresholds, not yet proved.

## 2. Scope

**In.** The two-body booleans `boolean::api_brep::{boolean, boolean_union, boolean_difference,
boolean_intersection, boolean_with_options}` and the manifold engine under them
(`boolean/merge/p4.rs`, `manifold_boolean`); the topology intersector
(`boolean/topology_intersector`); the point classification the classify step calls
(`query/brep.rs`, `BrepQuery::point_3d_classify`; `intersect/point_class.rs`); and the ops entry
points (`solidcore-ops/src/boolean.rs`).

**Out**, each with its own page:
- Surface/surface intersection, its curves and their certification: [ssi.md](ssi.md).
- What a definition is and how a net is certified: [definitions-and-certificates.md](definitions-and-certificates.md).
- List and tree booleans, the non-manifold boolean and the planar 2D boolean (`boolean/merge/p3.rs`,
  `primitive/primitive_creation`), which reuse the same engine and are not described here.
- Validity and tolerances of a body: [validity-and-tolerances.md](validity-and-tolerances.md).

## 3. The pipeline

![The boolean pipeline as a flow: ops' boolean() hands the operands to step 1, Intersect (face pairs by SSI and the coincidence check); then step 2, Imprint (the curves become edges in both bodies); then step 3, Classify (each face inside, outside or on the other body); then step 4, Select (keep or delete by the operation); then step 5, Merge (copy, delete and merge regions); Merge hands the body to ops' finish(), which returns the result or a refusal.](booleans/pipeline.svg)

*Figure 1. Look at the line between kernel and ops: the five steps all run in the kernel; ops only
hands the operands in and checks the result's contract.*

The problem: given two closed bodies A (the primary) and B (the other), make the boundary of
A op B. Every boundary-evaluation method has the same parts [2, 7]: intersect the boundaries, split
each boundary along the intersection, classify each piece against the other solid, select the pieces
the operation keeps, and glue them into one body. The port does them in that order, in one call,
`manifold_boolean_translated` (`solidcore/src/boolean/merge/p4.rs`), after moving both bodies by a
temporary origin shift (`merge/p1.rs`, `temporary_brep_transform`) that it undoes on every exit.

| step | what it does | code | layer |
|---|---|---|---|
| 1 intersect | every face pair whose boxes meet: a stored intersection, else the coincidence check, else SSI; the curves classified on both faces | `TopologyIntersector::intersect_topology` (`topology_intersector/p2.rs`), `face_intersect` (`topology/face/p3.rs`) | kernel |
| 2 imprint | the kept curve intervals and points become proto edges and vertices, are resolved against each other and imprinted as edges in both bodies; the two copies of each are mated | `intersect_insert_relate`, `resolve_intersections`, `imprint_intersections`, `set_relationships` (`topology_intersector/p1.rs`, `p2.rs`) | kernel |
| 3 classify | each face of each body: inside, outside or on the other, at the common edges by radial sectors, elsewhere by a ray | `classify_edge_faces`, `classify_remaining` (`merge/p4.rs`) | kernel |
| 4 select | keep or delete each face from its class and the operation | `region_keep`, `coincident_keep`, `repair_lists` (`merge/p4.rs`) | kernel |
| 5 merge | copy B's kept faces into A, delete the dropped ones, set each region solid or void, find the infinite region, remove redundant edges and vertices | `make_other_faces`, `delete_faces`, `result_solid`, `boolean_post_process` (`merge/p4.rs`, `merge/p1.rs`) | kernel |
| contract | faces left, and solid operands give a closed, outward solid (touching along edges or at vertices allowed) | `finish` (`solidcore-ops/src/body.rs`), `boolean` (`solidcore-ops/src/boolean.rs`) | ops |

Between steps 2 and 3, `mark_common` (`merge/p4.rs`) closes intersection loops that end at a common
vertex used by one common edge (`close_intersection_loop`) and, with more than one such vertex,
closes the gaps (`fix_gaps`); it repeats at most ten times. A pass goes on past each closure that
deleted no topology and gave the vertex a second common edge, so ten passes are not a limit of ten
gaps; a closure that leaves the vertex a loop end still ends the pass. A loop end is closed first along
an edge to another loop end, which closes the gap between them rather than branching the loop at a
vertex it already passes through. The classify and select steps also repeat
(at most ten times) when a coincidence repair changes the topology (§5).

After the cleanup of step 5, `place_vertices_on_face_corners` (`merge/p1.rs`) moves a vertex that lies
more than the intersection tolerance off one of its faces to the corner its faces define, only within
half its tolerance and only when the point leaves every face gap under the intersection tolerance and
every gap to an edge end within half its tolerance (measured at the point before the move). The point is
the corner when every face holds it, and otherwise the one that minimises the largest gap to the faces
(the midpoint of the corner and the nearest point of a face it lies beside, such as a round's patch ending at
its tangent line); otherwise the vertex stays. It runs after the cleanup because
the faces and edges the vertex keeps are final there.

Ops never computes a geometric quantity here: `boolean` takes the operands, calls
`api_brep::boolean`, and `finish` asks the kernel whether the result is a solid
(`is_outward_touching_solids`, `solidcore/src/topology/brep.rs`). Which `UpstreamFixes` run is the
application's choice (`solidcore-ops/src/session.rs`, `set_fixes`); ops' default is none.

## 4. Classification

![A box A and a circle B in section. At P, where B's boundary crosses A's right face, the dashed directions of B's two faces bound a sector, and A's face below P lies in it: inside B. From Q on A's left face a dashed ray runs in +x to its nearest hit H on B's exact surface, and the sector at H puts Q outside B.](booleans/classify.svg)

*Figure 2. Look at the two ways a face gets its class: at P from the sector of B's faces around the
common edge, at Q from the first hit of a ray on S.*

### 4.1 What is classified

After the imprint, every face of A lies wholly inside, outside or on B, and the other way round
(Tilove's set membership classification [3], at face granularity). The port does not classify each
face separately. It classifies a face at a common edge, then floods: `traverser::collect_faces`
gathers every face reachable from it without crossing a marked (common) edge, and they all take its
class. That is sound because a connected set of faces that meets no common edge cannot cross B's
boundary. A face set that touches no common edge at all (a face of A far from B, or a body wholly
inside the other) is classified once, by one point.

### 4.2 At a common edge: radial sectors

`classify_edge_faces` (`merge/p4.rs`) walks each common edge and its mate in both directions. For a
face of one body on the edge, `edge::find_radial_sector_between` (`topology/edge.rs`) finds which
sector of the other body's faces around the edge contains it, at up to ten points along the edge
until one classifies uniquely. Each test is `query::edge::radial_sector_classify_between`
(`query/edge.rs`): the binormals of the three faces (the direction into each face, square to the
edge) are compared, and when any two are within 10° the faces are stepped off the edge at growing
distances (the two edges' tolerances, twice that, then growing with how fast the step-off points
separate) until the sector test is unambiguous. This is the radial-edge form of Mäntylä's
neighbourhood classification [4, 6]. One thing differs from upstream. A point where every
candidate classifies from the binormals at the edge (all three at least 10° apart) is looked for
first, in the same order of points, and the faces are stepped off only where the edge has none
(`radial_sector_classify_at_edge`, `query/edge.rs`): near tangent, the steps read the faces where
they part, and a stretch of face within the tolerance of the other can lead them past the face to
where the surfaces cross back. Where two equal holes cross a hair off each other's axes, an edge
round the tip of one hole's lens in the other's wall has its midpoint in a neck 0.018 mm wide, the
walls within 1e-5 of each other, and read there the face lying in the material was put in the
holes' space; a point nearer the edge's ends, where the walls cross at a clear angle (36° at the
ends), decides at the edge. A second thing differs: the steps grow by a secant aimed where the faces
are twice the tolerance apart. Aimed at the tolerance itself, as upstream's was, faces parting at a
small angle (a ball dipped 2e-5 into a face leaves it at 0.094°) settled exactly that far apart,
and the unchanging distance read as a coincidence. The sector's region gives the class:
`region_keep` reads its void flag.

A sector whose faces coincide is the coincident case of §5.

### 4.3 Away from the edges: a ray

`classify_remaining` (`merge/p4.rs`) takes a point on an edge of the face (at 0.45678 of the first
edge's interval for A's faces, 0.456789 of each edge's in turn for B's) and classifies it against the
other body with `p1::classify_point_editing_disabled`, which is
`BrepQuery::point_3d_classify` (`query/brep.rs`) without the boundary solve:
- a body with one region answers without a ray;
- otherwise a ray is fired along +x, then −x, then ±y, then ±z, until one finds a region
  (`CLASSIFY_RAYS`). `bp2::ray_fire_with_bbox_and_solver` (`topology/brep/p2.rs`) solves the ray
  against the body's cache trees (`SolverOperation::Rayfire`), takes the closest hit and, among hits
  at the same distance, the one of lowest dimension (a vertex before an edge before a face), and
  classifies the start point by stepping back from the hit along the ray (`local_curve_sector`). A hit
  closer than `EFF_ZERO (1 + max |start|)` fails that ray, and the next direction is tried.

**What reads S.** The hits are roots on the surfaces themselves: a procedural blend is evaluated as
its net only for the solve's rough guess, and the local solve meets its tolerance on S
(`surface/surface_point_solve/p2.rs`). The cache trees the global solve searches are built on the
nets, with each box grown by its definition's certified ε (`surface/surface_cache/p3.rs`,
`definition_bound`), so no hit on S is lost to a box that holds only B.

**What is not proved.** Ray casting is the standard parity test on a closed boundary [5, 7], and it
is exact only when the ray meets the boundary transversally and away from edges. The port does not
prove that: a ray grazing a face, or a hit on an edge or vertex, is resolved by the lowest-dimension
rule and the local sector, and the sector test of §4.2 is decided by angle thresholds (10°, 0.5°) on
step-off points. Neither is a certificate in the sense of
[definitions-and-certificates.md](definitions-and-certificates.md) §4. A classification proved
with exact predicates [11, 12] or with interval enclosures of the sector test is open work.

### 4.4 Select

`region_keep` and `coincident_keep` (`merge/p4.rs`) are the regularized operations [1]:

| operation | a face of A is kept when it lies | a face of B is kept when it lies | a coincident pair is kept when |
|---|---|---|---|
| union | outside B | outside A | the faces bound material on the same side |
| intersection | inside B | inside A | the same side |
| difference A − B | outside B | inside A | opposite sides |

A coincident pair on opposite sides of a union, or the same side of a difference, is deleted: those
are the faces a regularized operation removes. The result's regions take their solid or void flag
from the source regions they came from (`result_solid`): union solid if either was, intersection if
both were, difference if A's was and B's was not. A region with no record of its sources (no face of
B was copied into it, because B's faces all lie on its boundary, as when a union fills a void
exactly) that a deleted face joins to another such region takes the class a deleted face has on both
sides: solid for a union, void for an intersection or a difference (`deleted_face_sides`).

## 5. Coincident and near-coincident faces

**Coincident faces.** Two faces on the same surface share a region, and its boundary is their
intersection. In step 1, `intersect_topology` runs the coincidence check on each face pair before
SSI (`topology_intersector/p2.rs`); a coincident pair gives copies of both faces' edge curves trimmed
to each other instead of an SSI curve (`face_intersect`, `topology/face/p3.rs`). In step 3, a radial
sector that finds a coincident face use hands the pair to `fix_face_coincidence`, which runs
`check_face_coincidence`, `TopologyIntersector::merge_coincident_surfaces` and
`finish_face_coincidence` (`merge/p1.rs`, `topology_intersector/p3.rs`): every unmated edge of one
face is merged into both until the two faces' edges are all mated, at most
`MAX_COINCIDENT_SURFACE_PASSES` passes. If that changed the topology, the classify step starts again.

**Coincident edges between related vertices.** Before that imprint, `relate_coincident_edges`
(`merge/coincident_edges.rs`, called from `fix_face_coincidence` once the check asks for a merge)
relates an unrelated edge of one face to the one unrelated edge of the other that joins the vertices
related to its two ends, when `certified_line_gap` proves the two curves within the larger of their
two edge tolerances of each other over both intervals (a Hausdorff bound with every rounding
counted; one of the two must be a line, the other is taken through its certain hull points).
Otherwise the imprint copies each onto the other face as a second edge between the same two
vertices, and the two zero-area faces between the copies keep the result from closing (an arc that
grazes a coplanar box face, `boolean_api.rs` `an_arc_grazing_a_coplanar_box_face_joins`). Shared
ends alone relate nothing: two arcs that join the same vertices may bow apart, and a line and an
arc that bow apart by more than the tolerance stay two edges and are imprinted. An edge with more
than one such partner is left to the imprint.

**Vertex placed by its best certified edge crossing.** An intersection curve that ends on an edge
places the boolean vertex there; several curves can end on one edge near one point, and the first
one found placed it upstream. A later crossing with a smaller misplacement bound along the edge
(the points' gap over the sine of the curve's angle to the edge) moves the vertex definition only
when two certificates hold over the stretch of the edge between the first placement and the new
one, each a bound over the whole piece in outward-rounded interval arithmetic
(`curve_piece_gap_bound`, `query/curve_surface_gap.rs`): the edge stays within the definition's
tolerance of the other object (face, edge or vertex), and of the curve whose crossing placed it
first. A grazing first crossing runs along the edge and certifies the stretch it covers; a steep one
leaves the edge at once and certifies only a stretch within the tolerance. Every candidate is
certified from the first placement, so the result does not depend on the order the crossings arrive
in. A surface without a NURBS net (an offset surface) or a curve kind without NURBS data gives no
certificate, and the vertex keeps its first placement (`VertexDefinition::place_by_crossing`,
`topology/proto_topology/p1/proto.rs`).

**How coincidence is decided.** `Surface::coincidence_check` (`surface/surface/p8.rs`) is exact only
for the plane: a plane is parallel within the angle that moves its far end by the tolerance and
passes through the other's origin (`surface/plane/p1.rs`). For other surfaces the base check is
sampled: a 6 × 6 grid of points dropped onto the other for whole surfaces, and for an overlap a
boundary curve of one meeting one of the other where the surfaces agree to second order, or the
middle of one dropping onto the other. A sampled check is not a proof; deciding coincidence from the
definitions (two quadrics by their coefficients, a definition against its own copy by identity, a
B-spline pair from the composed implicit field as `ssi/src/coincide.rs` does) is open work. Only the merge gate decides from definitions: `faces_coincide` (`merge/p1.rs`) decides cone (a cylinder is a cone of semi-angle 0), sphere and torus pairs by `Surface::same_analytic_surface`. Two cones or two spheres that are not one surface (axes or centres apart) still count as one over the two faces' boxes when an enclosed bound keeps them within the tolerance there: the box is halved across the axes until a Lipschitz bound on their separation decides each part that can hold either surface (`revolved_within`, `surface/surface/p8.rs`; two arcs through the same corners about centres 8.4e-5 apart stay within 1e-10 over the sliver they share). The intersector's coincidence check is still the sampled one. The sector test also tells two planes apart from their faces (`faces_on_other_surfaces`, `query/face/p1.rs`): two planes closer than 0.001° at a common edge are not coincident when a point of either face is proved to lie farther than the two faces' tolerance band from the other's plane (walls through a common edge at a tiny angle, within tolerance near it and apart beyond). The gap is read on each edge curve dropped onto its face's own plane, since the exact edge lies on the plane within the edge's tolerance τ_e of the curve while the curve may stand τ_e + τ_f off it; the band grows by τ_e times the sine of the planes' angle for that. The proof is an enclosure, never samples: a line's ends, and on a B-spline or conic edge the ends of its Bézier pieces, halved by de Casteljau while their hull leaves the band. Two planes, two different kinds (tangent, they can stay within the tolerance over a patch) and any pair with an offset, B-spline, extrusion or revolution keep the sector's answer.

**Near-coincident strips.** Two planar faces crossing at a small angle θ meet in a line fixed only
to tol / sin θ. The crossing is no event when, on each side of it, one of the two faces lies over its
whole boundary within the intersection tolerance of the other face's plane. Then the two faces lie
within that tolerance of each other wherever both exist beside the crossing (the distance to a plane
is affine, and its maximum over a compact face is reached on the boundary): one coincident pair. The
certificate takes every edge of the face, inner loops included, through its hull points (a line's
ends, a B-spline's control points, with their coordinate errors). Each hull point's slack adds its
offset from the face's own plane and the edge's tolerance times the planes' sine (`boundary_slack`),
since the face is the region its trims bound in its own plane. A boundary that bows outside the band,
a hole or notch edge past it, or an edge with no certain hull, so no face bounds that side, keeps the
crossing as an exit. The rule needs each face to cross the line: an edge end strictly on either side
of the other plane, beyond the same slack. Two faces meeting only along their boundaries, as at a
shared edge, are intersected as upstream does. Upstream still makes
the crossing an edge, whose ends are picked per body and never related, and `make_faceuse` then fails
(`SM_ERR`). The port decides the rule from the planes and the faces' boundaries before the face pair is
intersected, and makes no curve, edge definition or vertex definition for such a crossing
(`topology_intersector/coincident_strip.rs`, `crossing_inside_coincident_pair`; test
`solidcore/tests/wr_614_coincident_strip_crossing.rs`). The rule is for planes only and the default.

Two decisions it relies on are made once, before intersecting, for line edges. A vertex of one
operand within the intersection tolerance of a line edge of the other, off its ends and off its line
by more than rounding (a near miss), is an event there: the edge is split at the vertex's drop and
the two are one proto vertex (`split_edges_at_vertices`, `topology_intersector/p2.rs`). A vertex on
the line to rounding is its own drop and stays with upstream's vertex/edge classification. Two line edges whose ends then lie within
their intersection tolerance of each other, end for end, are one coincident interval (points at the
same fraction along both are an affine distance apart, so the ends bound it), and a point where the
two cross gets no vertex definition (`coincident_line_edges`;
`ProtoTopologyManager::set_coincident_edges`). An interval of a straight intersection curve whose
ends lie on one straight face edge is classified on that edge when both ends lie within the
intersection tolerance of its line (`CurveClassification::classify_edge_interval`). Such a certified
run is coincident, not "maybe": upstream's `AreCoincidentMaybe` would remove it as mislabelled when
it is short beside its edge, and the two edges were then never made one.

**One point per proto vertex.** Each body classifies a curve end within its own band, so a point
1.4e-5 from a vertex of each body classifies onto both even when the two vertices are 2.8e-5 apart,
beyond the 2e-5 they may differ by. `find_or_create_proto_vertex_pcs` (`proto_topology/p3.rs`)
makes no proto vertex for two classifications onto vertices farther apart than the two vertices'
intersection tolerance; upstream made one with that distance as its tolerance and merged the two
vertices. The rule is for vertices only: an edge's or a face's classified point follows parameters
that may not be settled yet (`ResolveOverlappingProtoEdges` re-parametrises overlapping edges, whose
first points can lie 18 mm apart in an outset of `Boolean/extrudeHoles1.smb`), so their distance
proves no separation.

**One point per crossing of an edge with a face.** A face pair's curve ends where its own face's
surface meets the other face. At an edge with a loose tolerance τ the two faces' surfaces lie up to
τ off the edge's curve, so their two curves end at two points of the edge, up to τ / sin φ apart
along it, φ the angle between the edge and the face it crosses, whatever the angle at which the
curves meet the edge (an imported edge of tolerance 4.0e-3 crossed at 1.3° to 1.8°: the ends 0.057
apart). Both are the one crossing of that edge with that face. A proto vertex's ball cannot take
both in: the capture distance stays below half the smallest distance between two vertices of either
operand, so that no ball reaches two vertices the body keeps distinct (`intersect_insert_relate`,
`topology_intersector/p2.rs`), and the two points lie farther apart than such vertices do. So the
lookup of `find_or_create_proto_vertex_pcs` (`find_proto_vertex_holding`, `proto_topology/p3.rs`)
holds a point in a proto vertex by its ball or by its crossing: the proto vertex has a definition on
the same edge and the same face, and the edge between that definition's parameter and the point's
is proved inside the band of the face (`edge_piece_in_face_band`). The band is the tolerance that
puts a point of the face on the edge: the zone of what was classified with the edge's zone, or with
the gap at which either of the two points was taken to be on the edge where that is wider (a point
the face's trims put there, below), up to the most a local tolerance may be, 1000 times the zone
([validity-and-tolerances.md](validity-and-tolerances.md) §5.1). Inside the band the edge lies in
the face, and where on that piece it passes through is not determined. The proof is a bound, not
samples: a part of the piece is within half the sum of its two ends' distances to the surface and
its length (an enclosed upper bound, `Curve::length_bound`), parts not proved are halved (ten times
at most), and a piece not proved stays two points. The edge must also cross the face at both
points: the sine of its tangent against the face's normal at least 0.017, the angle below which
the coincidence check takes a curve for running in a surface (`COINCIDENT_TANGENT_SINE`). Two
points where the edge is tangent to the face are the ends of a stretch the two share, and stay
two (an edge of tolerance 7.6e-3 running 4.5e-3 off a face: two curve ends 0.019 apart on it,
their distances from the face 4e-5 apart). With two vertices for one crossing the intersection
loop stayed open between them.

**A curve end on the face's boundary.** An interval of a curve inside a face used to take an end
the boundary intersections left unclassified for a point inside the face
(`ClassifyFaceInterval`). Such an end is now classified by the face's trims first
(`classify_face_interval_on`, `intersect/curve_class/p7.rs`), and stays on the edge or vertex that
finds. An intersection curve that ends on its surface's natural boundary ends on the face's edge
there; where that edge's 3D curve lies farther from the surface than its tolerance (3.3e-3 against
2.8e-3 on an imported edge) the curve/edge intersection finds no crossing, and the end, taken for
inside the face, became a vertex short of the edge with the loop open beside it.

**A curve that runs along an edge without crossing it.** An interval between two points on a face's boundary edge that runs within the band of that edge is on the edge, not inside the face (WR-305). A curved interval runs along the edge when its ends are farther apart than the band and the whole curve over it and the edge piece between its ends' drops are proved within the band of each other on enclosures, the pair halved where the bound is too coarse (`curve_runs_along`); on a closed edge the pair may run through the edge's seam, and a piece whose ends are one point runs along it only all the way round. A curved interval with an end in the ball of one of the edge's vertices, past the band but certified within that vertex's tolerance as a whole (typically a round's arc leaving a straight edge tangentially at its end), raises the edge's tolerance to its certified distance (cascading to the edge's vertices) and is then on the edge (`EdgeToleranceRaise`). The fork read the interval's midpoint alone, which also took a curve bowing far out of the band between its ends, and a piece about as short as the band that crosses the edge (a sphere clipping a box corner by about the tolerance, whose arc on a side face cuts across the corner: taken as on the edge, the union failed to relate the operands' entities). An end the edge intersections find no crossing for (an arc 1.4e-11 from another arc, ending 2.8e-5 short of that arc's corner) used to stay unclassified, so the interval was imprinted as a second edge beside the first; now such an end counts as on the edge when it lies within the band of it and clear of its vertices, the curve is tangent to the edge (within the angle tolerance) at both ends of the interval, and the whole curve over the interval is proved within the band of the edge on control points: the curve piece's control points against the chord of its ends, those ends against the edge piece's ends, and the edge piece's control points against its own chord, rounding counted (`piece_within_band`); the end is then classified to the edge with its parameter on it (`CurveClassification::classify_edge_interval`, `intersect/curve_class/p3.rs`). Without the tangency (a curve that crosses the edge at the interval's classified end and stops a hair past it, whose short piece lies within the band whatever its angle) or the proof (another curve kind, or a curve that bows out of the band between its ends) the end stays unclassified. An interval whose ends both classify onto the edge is decided by the run test alone (a straight piece along a straight edge by its ends, a curved one by `curve_runs_along`).

**One image for each vertex.** Where two faces of one operand that are not adjacent come within the
tolerance of each other, as the two faces of a wedge do near its knife edge, the operand is thinner
than the tolerance there, and a vertex of the other operand lying on that stretch is on both faces.
Each face pair through the vertex gives its proto vertex a definition, and each definition imprints
its own vertex in the thin operand, the two a few tolerances apart. `set_relationships`
(`topology_intersector/p2.rs`, `combine_images_of_one_vertex`) combines the second image into the
first (`CombineCoincidentVertices`) when each relation would hold on its own and the two images
share no edge, so the mate maps stay one-to-one; upstream dropped the second relation and left its
vertex without a mate. A face of the other operand lying in that stretch can then be coincident,
within the tolerance, with a face of the thin operand that was already classified at another edge,
or that the coincidence repair (`FixFoundFaceCoincidence`) will not pair with it. When the two
surfaces are of different analytic kinds, they coincide only over such a tangent patch, whose
boundary the imprint need not draw in the other face. When the whole face lies within the tolerance
of the other face's surface, the face is the patch, a copy within the tolerance of that part of the
other face, and once every common edge has been visited it is classified as one
(`resolve_tangent_patches`, `merge/p4.rs`, for union, difference and intersection). That the whole
face lies there is proved, not sampled, and only for a face on a cylinder or cone against a plane
(`cone_face_gap_to_plane`): the plane's distance over the cone is linear along each generator and a
sinusoid round the axis, so its extremes over a box of angle and height are found exactly, and the
box holds the face because it holds the convex hulls of the face's edges (their ends for a line,
their Bezier control points for a B-spline) and the face's one loop does not wind round the axis. The
rounding of that calculation is bounded too (the comment on `cone_box_gap`). The face must also lie
within the other face's edges, not only on its plane: its vertices and three points along each of its
edges must classify to the other face (`points_in_face`). That part is sampled, not proved. Any
other pair of kinds, and a face that leaves the plane or the other face anywhere, is only possibly a
copy and goes to the ray. Where the operation keeps no copy of coincident faces (a
difference of faces facing the same way), the patch goes. Where it keeps one, the copy is the other
face's when that face is kept, so the patch goes, and the patch's when the other face is deleted.
This replaces a ray cast from a point within the tolerance of the other boundary, whose answer there
can go either way. The patches are visited again until a pass classifies none, so a patch whose
other face is classified by a later patch follows it; one whose other face is still unclassified
then is left to the ray. Two faces of one kind are left as before.

This is the problem Segal's tolerant modeller and Jackson's local tolerances address [9, 10]: two
entities within tolerance are one, and a decision across that band has to be made once and kept by
every later step. The port's strip rule is one such decision; a general one for curved faces within
tolerance is open.

**The coincidence rule.** Whether two places are one (a point on a side of a surface's
domain, a curve along an edge, two points one vertex) is decided by one tolerance:
`coincidence_tol_3d` (`geom/tol.rs`), the larger of the contact tolerance (`contact_tol_3d`, §6)
and the two objects' zone tolerances, read by `within_coincidence_tol` at the tolerance or a
rounding of the copies past it (`γ_32` of the coordinates' size, as `within_contact_tol` reads a
contact). A zone tolerance is how far an object's stored geometry may stand from what it stands
for, so a place within it of an object is on the object by the object's own statement; two
places farther apart than both tolerances are on neither by either, and nothing makes them one.
Upstream reads such places at the intersection tolerance, the two zone tolerances' sum, twice
the zone tolerance where they are equal: one place up to 2e-5 at the default 1e-5, where the
contact rule (§6) takes circles overlapping by more than 1e-5 for two crossings, two places.
In that band, a groove more than 1e-5 and up to 2e-5 deep cut into a block face (a 50 mm
block less a box reaching `depth` past its face, 10 wide) comes out as the block unchanged,
and a boss overhanging a cut ball's rim by as much is dropped by the union.

The sites move to the rule one at a time (§10, "The coincidence rule in migration"). Reading it:
- the side test of an edge use (`is_edgeuse_side_in_surface`, `topology/edgeuse/p2.rs`; the
  check's Loop test 2): a sample of the edge, dropped onto the surface, lies on a non-periodic
  side of the surface's domain within the coincidence tolerance of the surface with itself
  (its zone tolerance, at least the contact tolerance), where upstream took twice the zone
  tolerance. A notch twice the zone tolerance deep has edges, where its walls meet the
  faces across it, that leave the side square to it; at the intersection tolerance every
  point of them lay on it, across the side's direction, and the check refused the body
  (`solidcore-ops` `heal.rs`, `a_block_notched_twice_the_tolerance_deep_is_valid`);
- two points one proto vertex (`find_proto_vertex_within` and `DeclaredContact::overlaps`,
  `proto_topology/p1/proto.rs`; "One point or two" below): two points, or their uncertainty
  regions, are one within the larger of their tolerances (radii), where upstream took their
  sum. A point classified on a vertex is that vertex's point, so its proto vertex also holds
  the ball of the vertex's own point (`find_or_create_proto_vertex_crossing`,
  `proto_topology/p3.rs`): a later point within the vertex's tolerance of it is at that proto
  vertex though it lies farther from the average the pair was placed at;
- a point of a curve on the vertex where a seam ends on a boundary of material (the seam stretch
  of `edge_solutions`, `intersect/curve_class/p9.rs`; "Seams and poles" below), with the proto
  vertices: within the larger tolerance, not the band of both.
- When two source edges produce overlapping proto edges, `resolve_overlapping_proto_edges`
  (`topology/proto_topology/p3.rs`) compares their curves over the trimmed intervals.
  For this search, each edge's zone is capped at its owning B-rep's tolerance, with the
  manager's effective tolerance as a floor and as the bound for an unresolved owner.
  Earlier vertex merges can raise an edge's zone, so those raises must not decide a new
  shared interval. `coincidence_reach_3d` includes model contact and rounding at the trimmed
  curves' scale. Both faces then classify the first edge's curve with its true zone
  tolerance, using the manager's tolerance as a lower bound.
- The ordinary curve/edge classifier can prove an entire curve clear of a boundary even
  inside the intersection search band. The semi-axis radii bound every point on an ellipse.
  Projecting a positive-weight B-spline's control hull onto the normal of an ellipse's
  plane bounds every point on the spline. A surface–surface intersection also lies on
  both of its defining surfaces. If either surface is provably clear of the trim, the
  intersection is clear of it too. A gap past the local coincidence reach rules out a
  shared edge. This prevents two concentric circles from sharing only part of their
  circumference when their actual separation is constant.
- A seam can end on a smooth closed material edge at a vertex that marks the start of the
  edge's parameter interval. Before ruling out a snap to that vertex, the classifier
  checks both the vertex and its complete material edge. An oblique cut can cross the
  edge elsewhere even though it misses the vertex. When both are proved clear, the
  curve intersector keeps its transverse crossing at the solved parameter.
  Where several material edges meet, their crossing regions still determine how a corner
  participates in the Boolean. A vertex without a complete material edge proved clear
  retains the existing tolerant classification. Explicit gap repairs retain their allowance.
- On a planar face, the control points of a rational circular arc can lie outside the
  face's parameter domain even when the complete circle lies inside it. Rotating a
  cylinder's seam moves these controls. If the projected ellipse fits within both
  parameter bounds, the face keeps its exact rational trim. Its annular area then
  agrees with the two spatial boundary circles at every seam orientation.

Still on the intersection tolerance:
- an intersection curve along an edge (`edge_solutions`, `intersect/curve_class/p9.rs`).
  Where the curve/edge classifier lacks a separation proof, a curve that stays within the
  sum of the two zone tolerances can be classified along the edge. For a rectangular groove
  this still puts the groove's floor on the edge of the block and loses the groove.
- a vertex of one operand on a line edge of the other (`split_edges_at_vertices`,
  `topology_intersector/p2.rs`), the two vertices of a pair that a proto vertex relates
  (`find_or_create_proto_vertex_crossing`, `proto_topology/p3.rs`) and the combine radius of
  two exactly placed definitions (`EXACT_TOL_FACTOR`, `proto_topology/p2.rs`): moving the
  first with the proto vertices turns the unions of two boxes whose faces cross at
  atan(0.1), 8e-7 and 1.5e-5 off (`wr_614_coincident_strip_crossing.rs`,
  `wr_619_union_solid_leak.rs`), into refusals, so these move with the curve along an edge;
- an interval of a curve along an edge (`classify_edge_interval`,
  `intersect/curve_class/p3.rs`): the boss over the rim;
- the check's Surface test 6 (an open surface's opposite domain sides partially coincident:
  a notch's wall more than 1e-5 and under 2e-5 wide fails it) and the healer that reads an imported body (`Body::from_record`
  untrusted), whose healing changes the topology of a block notched more than 1e-5 and under
  2e-5 deep, so the import is refused;
- the side test's own degenerate edge (a sample box under twice the zone tolerance).

**One point or two.** A proto vertex holds the points the two operands' intersections give for one
place. Each point is known to its tolerance (a proto vertex's, or the one a proto vertex made of
the point would get: the largest of its deviation, the intersector's tolerance and its objects' zone
tolerances). By the coincidence rule (above), two points whose distance exceeds the larger of their
tolerances (and the contact tolerance) are on neither's ball, two points, and no proto vertex may
hold both: `find_proto_vertex_within` (`proto_topology/p1/proto.rs`) takes a proto vertex only
within that tolerance (`within_coincidence_tol`), the distance shrunk by its rounding (`γ_4`), a NaN
one apart. Upstream's sum made two points with tolerances of 1e-5 one point up to 2e-5 apart: the
corners of a groove 1.5e-5 deep were one vertex. A proto vertex made of a point on a vertex (a pair
of operand vertices, or a vertex and an edge or a face) holds the ball of that vertex's own point
as well as the ball about the average it is placed at: a vertex says every point within its
tolerance is on it. A cylinder's seam vertex 1e-5 off a bar's edge, made one with its drop on the
edge (placed midway), is 1e-5 from where the seam crosses the bar's face beside the edge, and
1.12e-5 from the midway point: the crossing is on the seam vertex, so it is the same proto vertex
(`solidcore-ops` `boolean.rs`,
`a_cut_whose_seam_edge_ends_within_tolerance_across_the_seam_is_valid`). The capture distance (0.01, or half the nearest vertex pair of either operand) only says
where to look. Upstream merges everything within it, and points 1.4e-4 to 1.7e-4 apart (a box edge
crossing a 0.3 mm sphere twice, each crossing fixed to 5e-6) became one vertex: the arc between them
a loop, the boolean the sphere alone or "no radial sector of the edge takes the new edge use".

A point's tolerance is how well it is known across the curves that fix it. Along an edge that a
curve crosses at angle θ, the crossing is known only to tolerance / sin θ: a curve moved by the
tolerance moves its crossing that far along the edge (the bound of the near-coincident strips
above). So a point that a curve fixes on an edge has an uncertainty region, the segment tolerance /
sin θ either way along the edge, widened by the tolerance (`DeclaredContact::of_crossing`). θ is
enclosed over the whole reach, not read at the point (`Curve::crossing_reach`, `curve/proximity.rs`):
the tangents of a curve piece lie in the cone of the differences of its Bézier control points, so
over a parameter window about the point θ is at least the angle between the two curves' cones'
axes less both half angles, and the window is widened until tolerance / sin θ lies inside what
both windows cover. For two edges crossing, the two curves are the edges. Where no window proves a
reach (a curve grazing or running along its edge: an arc of radius 2 crossing at 0.16° turns by
more than that within its first-order reach of 3.6e-3) the region reaches as far as the capture
distance looks. The segment runs along the edge's tangent and is widened by how far the edge
strays from that line within the reach, enclosed by the tangent cone of a window on each side of
the point (`Curve::stray_from_line`); an edge with no such enclosure gives the ball of the capture
distance. A point that no curve fixes but that lies on an edge of one operand and in a face of
the other (a face pair's single point, a degenerate interval) is where the edge crosses the face's
surface, known along the edge to tolerance / sin θ of that angle, which nothing encloses there: its
region reaches as far as the capture distance looks. Two drills 1e-9 apart, the lower one 1e-9
through the plate's bottom: its rim crosses the bottom at 2.8e-5 rad, twice, 1.4e-4 apart, and both
crossings are one point with the end of the line where its wall touches the bottom, as for drills a
hair inside. Any other point with no crossing angle has the ball of its tolerance. Two points
are one when their regions are not proved apart (`DeclaredContact::overlaps`: the gap between the
two segments' projections onto the direction of their closest pair, less its rounding, which no
pair of their points is closer than even where the near-parallel solve picks the wrong pair, within
the larger radius by the coincidence rule), and
each proto vertex keeps the regions of the points it holds, also when it is combined into another. A
point goes to the closest proto vertex it is not proved apart from, so a point whose region also
reaches a second proto vertex (one made after it, or merely farther) leaves two proto vertices that
nothing proves two places: once every face pair's proto vertices are made, each such pair is
combined (`combine_proto_vertices_not_proved_apart`), and a proto edge between them becomes a
self-loop that is dropped within its ends' reach or refused (WR-214). A box corner 1.58e-5 inside a
0.5 mm sphere (tess0033) crosses the sphere along its three edges 2.2e-5, 2.3e-5 and 9.9e-5 from the
corner; the far crossing, on an edge 15° off the sphere, is fixed only to 1.2e-4 along it by one of
its arcs, which reaches the corner: one point, the box and the sphere meeting there. A wall/round curve
along a slab's edge at 5° and a rod/round curve across it at 40° end 9.3e-5 apart at 1e-5: one
point (1e-5 / sin 5° = 1.1e-4). The two crossings of the sphere's circles by the box edge, 1.5e-4
apart, are crossed at sines of 0.26 and more: 3.9e-5 each, two points.

For a chart vertex on a smooth closed material edge, overlapping crossing regions can
reach the old rim even when the new intersection lies on a different face. During lookup
and coalescence the Boolean checks the fixed vertex and its whole material rim against
the opposite operand's defining plane, sphere or cylinder. If both are proved farther
than the local coincidence reach, the two crossings retain separate vertices. A coaxial
cylinder can thereby be distinguished from an older circular rim, while a plane that
actually crosses the rim can still meet it near the seam. The check uses the face recorded
by the classification and preserves the allowance for raised local tolerances.

Two points farther apart than that are one only where a rule proves them one contact point, and the
rule declares it before any proto vertex of a face pair is made (`DeclaredContact`,
`set_declared_contacts`): every point within a radius of a segment, whatever their distance. The
strip is such a proof. Two planes crossing at θ stay within tolerance of each other across a strip
about the crossing line, bounded by an edge of either face lying within tolerance of the other plane
(the gap between two planes is linear across it, zero on the crossing): the crossing is fixed only
to that strip, so at each end of it the crossing's end and the bounding edges' ends are one contact
point, the segment across the strip widened by the tolerance within which the edges cover the
crossing and by how far their ends lie off the edges' offsets (`strip_contacts`,
`coincident_strip.rs`; a half strip where only one edge bounds it). The strip ends at the
innermost edge on each side, each proved on the other face's plane by its hull; an edge farther
out bounds nothing beside the crossing. A
crossing inside a coincident pair (near-coincident strips, above) makes no curve and so declares
nothing; the contacts are those of the crossings that stay, read from the edge-on-face curve
intervals beside them with each curve bounded over its certified hull. To declare them before those
proto vertices exist, `intersect_topology` first intersects every face pair, then makes their proto
topology in the same order (shell vertices and wire edges before them, as upstream).

**What a vertex stands for.** The points a proto vertex takes for one become one vertex in each
body, placed at one of them, and that vertex stands for all of them: a curve that fixed another of
the points still ends at that point, not at the vertex's. An edge's end has to lie within half its
vertex's tolerance ([validity-and-tolerances.md](validity-and-tolerances.md) §5.2), so the vertex's
tolerance is at least twice the distance from its point to the end of each curve that ends at it:
a distance between two computed points, exact to rounding, whatever the points lie on (a
construction's bound, §8 there). After the imprint each vertex a definition made is given that
tolerance (`certified_vertex_gaps`, `topology_intersector/p1.rs`), and only while its ball stays
clear of the far vertex of each of its edges, that vertex taken at its own grown tolerance (or at
the tolerance a growing edge of it passes on to its ends, whichever is larger): a vertex cannot
stand for both ends of an edge. The tolerance a growing edge passes on is judged the same way at
each end it raises and at every vertex that end becomes one with. The two operands' vertices of one definition become one, which
takes the larger tolerance of the two, so each ball is judged at that one, about its own point,
and the two are covered together or not at all. For a definition placed exactly (each source a
vertex, a line edge or a plane face) the gap is all there is to cover, so its edges grow to their
own ends' gaps too and a ball that is not clear is refused (`MergedVertexReaches`). Any other
vertex keeps its tolerance where its ball is not clear (`covers_staying_clear`): the points of
its proto vertex may lie as far apart as their regions reach (a box edge grazing a unit sphere
at 0.17° crosses it twice, 5.8e-3 apart, one point by its reach of 3.4e-3), and an edge that
ends at another of them is not at this vertex for a tolerance to say so. Upstream grows a vertex
to its distance from its edges' curves, not from their ends, and leaves an intersection curve's
end where it is. Two vertices of a 0.68 mm sphere united with a box (`ub175` of
`boolean.api_brep`) show both. A box corner 9.8e-6 outside the sphere is taken for the nearest
point of the circle the box's bottom face cuts, 9.97e-6 away; it kept the 1.95e-5 that covers
its 9.77e-6 from the corner's edge lines, 2e-7 short of the corner at half. And the sphere's seam
meets a box edge that runs 4.2e-6 inside the sphere, 2.17e-5 below where the circle of the box's
next face crosses the seam: the two crossings are one point (tolerances of 1.4e-5 and 1e-5), the
vertex lies at the lower one (1.2e-5 in the union), and the circle's two halves end 2.17e-5 above it
(Level2 "Edge/Vertex gap", twice). The corner now has 2.0e-5 and the seam vertex 4.3e-5, 7.5e-5
from its neighbours.

**One edge or two.** The same holds one dimension up. An intersection curve that runs between two
vertices of a face which already has an edge between those two vertices is that edge when the two
are proved one curve: every point of each within the sum of their tolerances (the larger of the
face's and the curve's, plus the edge's, read at most the B-rep's) of the other, over their whole
lengths, or within a vertex's tolerance of that vertex, which holds both ends. An edge's tolerance
grows as its vertices take in points, so read as grown it made the answer depend on the order of
the merges before (a sheet shelled by exactly two tolerances lost one of its four alike side walls).
A vertex's tolerance grows the same way, as the curves that end at it are imprinted, so each of the
two vertices is read with the ball that covers the asked curve's own end there, twice its distance
from the vertex's point (what the vertex has once that curve is an edge: "What a vertex stands
for"), where that is more than its stored tolerance and the ball stays clear of the vertex's far
vertices, the curve's other end among them (two balls that hold a whole edge between them would
prove any curve along it). A curve the proof cannot enclose (a composite curve) is no edge. `SmEdgeDefinition::Imprint` then takes the
edge and makes no second one beside it, in either operand (`face_edge_proved_to_be`,
`proto_topology/p2.rs`; `find_edge_proved_along`, `geom/tools.rs`). The proof is an enclosure, never
samples (`Curve::proved_together`, `curve/proximity.rs`): each curve is cut into pieces, a piece is
enclosed by its chord and the largest distance of its Bézier control points from it (read off the
knot span by blossoming, with their rounding), and a piece of one curve lies within its own radius,
plus the distance of its chord's ends from the facing chord, plus that chord's radius, of the other
curve; a piece that fails is halved, and what is not proved within a fixed number of pieces is two
edges, as upstream makes them. Upstream always makes the second edge: the face then has coincident
edges (`SmFace` tests 33 and 34). Two equal tubes meeting end to end are the case: the seat tube's
end circle crosses the top tube's wall 2.9e-4 from its own seam vertex, itself 8.6e-5 from the
corner where the two end circles meet. The three are three points (1e-5 each), and between the seam
vertex and the crossing the wall's cut by the seat's end plane, an ellipse of radii 71 and 16, runs
4e-9 inside the circle: the circle's edge. The capture distance used to make the three one vertex,
which hid the question. A sliver between two such edges is no solid: two boxes overlapping in a
wedge 1e-4 across and 5e-6 thick intersect in nothing (kept as two edges, the wedge was a body of
three faces that failed its check). Nor is a lens between two such curves a face. In `ub175` the box
edge runs 1.5e-4 inside the sphere, 4.2e-6 deep, and the circle the box's next face cuts rises
2.13e-5 above it at the sphere's seam, where the seam vertex splits both: each half of the circle
lies in the vertex's ball (4.3e-5) or within 1.6e-5 of its half of the edge, and is that edge.
Read with the tolerance the vertex had when the circle was asked (1e-5), the two halves were
second edges and the two slivers of the box face between them (5.5e-10 each) faces of the
intersection and the difference, whose edges the face check takes for coincident.

**An edge in a face, off it by more than the zone tolerance.** An edge of one operand can lie in
a face of the other within its own tolerance but further than the system zone tolerance from it:
a revolved tool's rim (radius 2.1000073) in a 2.1 mm hole's wall, the revolve axis 1.2e-5 off the
hole's, so the rim is 4.6e-6 to 1.9e-5 from the wall (#17527). Both of the tool's faces at the rim
meet the wall along it, each in a curve of its own (the disc's plane in the wall's circle, the
cone's in a marched curve), and their proto edges overlap. `ResolveOverlappingProtoEdges` replaces
the overlapping ones with the rim's own curve classified against the wall. Upstream classifies it
within the proto topology's tolerance, half the zone tolerance, where the rim lies in the wall
nowhere: the overlapping pieces were deleted, nothing replaced them, and the cut failed in
`make_faceuse`. The rim is now classified within its own tolerance where that is the larger, as
the face pairs classified it. The replacement then holds every piece of the rim in the wall, so
a piece of the same rim and wall that the overlap test (ends dropped within the zone tolerance)
did not find goes with the group too; left beside its replacement, it was imprinted twice and the
rim was related to both copies ("another entity is already related to the other one";
`absorb_pieces_of_edge_in_face`, `proto_topology/p3.rs`). A rim further off than its tolerance
somewhere is still not one edge with the wall: with the axis 1.3e-5 off, the cut leaves a sliver
face that fails the check.

**Seams and poles.** A tolerance band says where a boundary of material may lie. A seam (an edge
with the same face on both sides: `face_on_both_sides`, `topology/edge.rs`) and a pole (a vertex
all of whose edges are such seams: `inside_face`, `topology/vertex.rs`) bound no material: they
are where the surface's parametrisation ends or degenerates, and the face goes on across them. So
their tolerance adds nothing to a contact: a curve meets a seam edge or a pole only where the
curve's own uncertainty region reaches it (`SourceReach` and `in_reach_region`,
`intersect/curve_class/p1.rs`; `edge_solutions`, `p9.rs`; `intersect_vertices_in`, `p5.rs`). For the
same reason no other face pair meets a curve that runs along a seam: the face on the seam's
other side is the face itself. Such a run, proved within the intersection tolerance of the seam
(`Curve::proved_near`), is kept as on the seam rather than removed as mislabelled
(`are_coincident_maybe`, `intersect/curve_class/p7.rs`), and its end is where it leaves the seam,
known to its tolerance, not a crossing of the seam (`runs_along_seam`,
`proto_topology/p3.rs`): a drill whose wall passed within the tolerance of a cut ball's rim
vertex on the seam was left open there (tangency-labs/solidcore#3339).

The region depends on what fixes the curve. An edge's curve is given: it is known to its
tolerance. An intersection curve of two surfaces is known across itself only to
tolerance / sin θ, θ the angle between the two surfaces along it: a surface moved by the tolerance
moves the curve that far within the other (the bound of the near-coincident strips, and one
dimension up from the crossing of two curves under "One point or two"). Along itself the curve is
known to its tolerance: the angle of its own two surfaces says nothing about where it ends, so a
curve that stops short of a seam is short of it by its own tolerance. A point is outside the region
only where that is proved, each distance less the rounding of the difference it is read from.

θ is enclosed over everywhere the exact curve can lie, not read at points (`intersection_reach`,
`surface/normal_cone.rs`): the curve is cut into pieces, each inside a ball (its hull); for a
ball, the normals of a plane, cylinder, cone, sphere or ring torus at its points inside lie in a
cone given in closed form (a sphere's within `asin(ρ / |m - c|)` of the direction from its centre
`c` to the ball's centre `m`, `ρ` the ball's radius), and θ is at least the angle between the two
cones' axes less both half angles. The balls are widened by the reach they prove until that reach
lies inside them. Where no angle is proved (the surfaces tangent or nearly so somewhere along the
curve, a surface of another kind, a curve without a hull) the curve may lie anywhere across itself
within the band of both tolerances, as at a boundary edge. That band is also the most a proved
reach counts: the seam edge that then stands for the curve has to hold it within both tolerances.
A classification reads the reach of its two surfaces (`SetIntersectData`) the first time it meets
a seam or a pole (`source_reach_in`); the face intersection sets it for its curves
(`intersection_curve_reach`, `topology/face/p3.rs`).

A face without a seam has no pole either: a pole's edges are all seams of the face. The face
intersection finds once whether each face has a seam edge (`face_has_seam`, `query/face/p2.rs`)
and keeps the answer while the face's B-rep is unchanged (`FaceIntersectCaches`); on a face
without one, its curves' classifications ask no edge whether it is a seam and no vertex whether
it is a pole (`face_known_seam_free`, `intersect/curve_class/p1.rs`). Asking for every curve
grew with the curves times the face's edges, the square of a face that meets as many faces as it
has edges (`WorkCounters::seam_tests`, counted in
`a_plate_joined_onto_its_top_tests_each_face_for_a_seam_once`).

A stretch of the curve along a seam is what the curve/edge intersector finds within the reach. It
reports a stretch only where all of it lies within the distance it is given, and the seam's end is
no pole where the seam ends on a boundary edge: that vertex bounds material, and a point is on it
by the coincidence rule (§5, "The coincidence rule": within the larger of the two tolerances and
the contact tolerance). So a stretch the intersector finds within the band also stands where an
enclosure (`Curve::proved_near`) proves every point of it within the reach of the seam or on such a
vertex (`BoundaryEdge::seam` carries the two). Upstream's band of both tolerances about the vertex
let a curve leaving a cylinder's seam run "along" it to a point 2.2e-5 to 2.4e-5 from the seam's
end vertex, which the proto vertices hold apart from the vertex: the bar the seam vertex lies that
far off (`a_cut_whose_seam_edge_ends_within_tolerance_across_the_seam_is_valid`) came out empty.

"One edge or two" takes a seam for an intersection curve by the same reach
(`find_edge_proved_along`, given the crossing face's surface), and the loop check takes an edge as
running along a periodic side of its face's domain only within the edge's own tolerance
(`is_edgeuse_side_in_surface`). `HasSeamProblem` (face test 36) classifies the face's own seam
curve against its edges within both tolerances, as upstream does (`SourceReach::Boundary`), so it
reports an edge beside the seam inside that band as a seam it nearly misses.

Two cases fix the two ends. A box face 1.97e-5 beside the seam plane of a 0.3 mm sphere (`gen0150`
of `boolean.api_brep`) cuts it at a right angle: the circle is known to 1.05e-5 (enclosed; 1e-5
exactly) and is an edge of its own beside the seam, passing the pole. A second face cuts a shallow
cap at about 13°: no reach within the band is proved for that circle, but it ends 1.99e-5 short of
the seam, along itself, and so does not reach it. With the 2e-5 band the first
circle had half its length classified on the seam and its top on the pole, and the slab between
the seam plane and the face went with the wrong side (union 4.2e-3 off, intersection and
difference 4e-2). Two tubes of radius 16 joined end to end with 1e-5 between them
(`wr575_tube_ends_join_adds_no_check_failure_at_any_gap`): a short intersection curve of two
surfaces that cross at a right angle (reach 1e-5) starts on a tube's seam (1e-13) and ends 1.03e-5
from it, beside the vertex where the seam ends. It lies within the reach but for its last stretch,
and that within the band of the vertex: the seam's piece, as upstream has it within 2e-5. Held to
1e-5 by the intersector alone it met the seam in one point only, and the union was refused ("the
related vertex is not an end of the edge use").

## 6. Tangent contacts

Where the two boundaries touch without crossing (a cylinder on a plane, a sphere in a cone, two boxes
sharing an edge) n1 × n2 = 0 along the contact, SSI has no marching direction there
([ssi.md](ssi.md) §4.4), and the regularized result keeps no contact face, edge or point that bounds
no material.

What the port does today:
- **Touch points.** A degenerate SSI curve is classified at its point on both faces and kept as a
  point when it is on or inside both (`face_intersect`); `intersect_topology` imprints it only where
  it classifies to imprintable topology in both faces (`topology_intersector/p2.rs`).
- **Contact curves inside a face.** A union, intersection or difference removes the zero-area
  inner loops a contact curve leaves and splits shells joined only through them (`merge/p4.rs`;
  `topology/brep/p3.rs`, `remove_slit_loops_where_solids_touch`). The parts of a split shell
  are regrouped by nesting: parts apart from each other bound solids of their own (a cylinder
  resting on a box, united), and a void part lying inside an outer part bounds the same solid
  (a cavity cut that touches the body's outside along a line: one solid with a void).
- **Solids touching along an edge or at a vertex** are a valid result: ops checks the result with
  `is_outward_touching_solids` (§3).
- **The sector test near tangency** steps off the edge and compares binormals until they separate
  (§4.2); where they never do within the face, it returns the coincident or zero-sector class.
  Tangent faces part as the square of the step over their relative radius, so fixed steps and
  angle thresholds read the faces of a large enough part as parallel. Two readings therefore step
  on, three times further each round, until the faces are more than the tolerance apart: a sector
  whose own two faces look parallel (a zero sector), and a classified face that looks coincident
  with a face the definitions show to lie on another surface (`query/face/p1.rs`,
  `faces_on_other_surfaces`: two different analytic kinds, or two quadrics of one kind that
  `Surface::same_analytic_surface` tells apart). A tangent pair with no definitions to compare (a
  B-spline on a plane) is still read as coincident once its relative radius passes about 1.7e7
  times the sum of the two edges' tolerances, 330 mm at 1e-5 each (`query/edge.rs`, the 0.01°
  rule over eight rounds).
- **Circles a hair less than tangent.** Two coplanar circles that overlap past touching by δ
  cross at two points `2√(2δ r1 r2 / (r1 ± r2))` apart (`2√(rδ)` for equal radii from
  outside), far beyond δ, while the circles stay within δ of each other between them; each
  crossing is fixed along the wall only to the tolerance over the crossing angle, so the two
  cannot be told apart. `circle_coplanar_circle_intersect_in_contact`
  (`geom/geom_utility/p1.rs`) takes an overlap up to a contact tolerance of its own as one
  touching point midway between their nearest points, apart from the near-miss band in which
  circles that far apart touch. On the boolean's path every site that decides the contact
  passes the same contact tolerance, `contact_tol_3d` (`geom/tol.rs`: the system zone
  tolerance, 1e-5 by default, not the edges' and faces' grown tolerances, so the same holes
  give the same topology whatever came before; the touching point lies δ/2 from each operand,
  within the approximation tolerance the intersection curves through it are held to): the wall
  intersection of parallel cylinders that both bound faces (`surface/cone/p3.rs`), the
  circles of an edge and a face's intersection curve (`curve/ellipse/p2.rs` through
  `intersect/curve_class/p9.rs`) and the curve/surface classifier's osculating circles where
  an edge meets a face (`surface/surface/p4.rs`). Two holes a hair less than tangent then
  touch along one line and at one point on each rim. Bare curves and surfaces keep upstream's
  zero: their radius-relative bands are a search heuristic, not a modelling tolerance. Holes
  overlapping by more than the zone tolerance and less than the intersection tolerance (twice
  it) cross in two lines at every site and are still refused as not a solid, as before; with
  the intersection tolerance as the contact tolerance they came out a valid solid that kept the
  second tool whole, as a void.
  Two more sites read the same contact where a surface intersection meets such a lens. The
  surface intersection of a curved face with a wall standing on the other face's flat (a
  ball cut flat, or a half torus, under a boss a hair past its rim) finds the lens where it
  meets the faces' boundaries, at its two crossings, as isolated points, each on an edge of
  both faces; with no curve through it, a point classified onto an edge of each operand is
  the crossing of those two edges, and where they are circles in one plane that touch by the
  contact rule it is their touching point (`touch_of_edges`, `proto_topology/p3.rs`, before
  the proto vertex is looked for). And a flat face's domain is the box of its trims, so a
  side of it runs along the face's round edge where that edge reaches the box: the plane's
  section of a cylinder or cone standing on the face, a circle a hair past the edge, was
  cut at the side, its two ends the lens's half length off the touch; it now touches that
  side when it overlaps it by up to the contact tolerance, where both surfaces are
  faces' (`Plane::make_new_plane_ellipse_in_contact`, `surface/plane/p2.rs`, from
  `surface/cone/p3.rs`). The overlap is the cap's exact depth past the side, the ellipse's
  support distance along the side's normal less the side's offset from the centre, never
  the sagitta of a circle of the smaller radius, which takes a long ellipse's cap near its
  major vertex for about a hundredth of its depth at radii 100 and 1. Either left the crossings as vertices of their own beside the
  touch, or curves ending there, so the union of such a boss failed or kept a vertex a
  crossing away (`boolean_api.rs`, `a_boss_a_hair_past_a_cut_balls_rim_joins`;
  `solidcore-ops` `boolean.rs`, the half torus's seam).
  Parallel cones (drill points) are met by the marching intersection and are not decided this
  way, nor are coplanar faces bounded by such circles.
- **A contact at the tolerance.** Each site decides the contact from its own copy of the two
  curves (an edge's circle, a face pair's intersection curve, an osculating circle), the same
  curves by different constructions, a few roundings of their coordinates apart. A boss placed
  exactly the tolerance over a rim (stress corpus S0051) read its overlap as 9.99999999962e-6 at
  some sites and 1.0000000000673e-5 at others: a touch at one, two crossings 0.0148 apart at
  another, and the merge then related the operands' edges across the lens both ways. So the
  contact is read once, by one predicate, `within_contact_tol` (`geom/tol.rs`): a distance is
  within the contact tolerance unless proved past it by more than the rounding the copies can
  differ by (γ₃₂ of the largest coordinate), as two points are one unless proved apart ("One
  point or two", §5). Every site that decides a contact reads it: the circle/circle and
  line/circle intersections in contact (`geom/geom_utility/p1.rs`), and through them the walls
  of parallel cylinders, the edge/curve circles, the classifier's osculating circles and the
  touching point of two edges (`touch_of_edges`); the plane's section touching a side of its
  domain (`surface/plane/p2.rs`); and the plane's tangent line on a cylinder or a cone's
  generator (`surface/cone/p3.rs`, with the faces' zone tolerance). A contact at the tolerance
  is a touch at every site; one proved past it crosses at every site. What remains open (§10):
  circles overlapping by more than the contact tolerance and up to the intersection tolerance
  cross at every site, as above, and two such holes are refused as not a solid; and at a small
  zone tolerance the marching intersection of a wall with the other face can trace a curve
  inside the lens, from the touch to a crossing along both edges (a boss 5e-8 past a rim at a
  zone tolerance of 1e-7), which the contact does not end.
- **A curve ending tangent to an edge.** A curve that touches an edge in contact of high order
  (where a wall's intersection with a top tangent to the floor ends on the floor's arc, apart from
  it as the fourth power of the distance) stays within the scaled zero of the edge for a while
  beside the touch, and the curve/curve intersection can report a range there with both ends
  exact. Upstream keeps such a range as a coincidence and the curve is cut at its far end, which
  is no event. `drop_ranges_of_tangencies` (`curve/curve/p8.rs`) drops a range whose gap grows
  faster than linearly from a touching point beyond one of its ends (a point solution, or where
  the two curves' interval ends meet) at which the curves are tangent, when the range stops short
  of the touch and its far end, exact, lies off the ends of both curves: the touch is the
  intersection. The growth is read at the range's two exact ends, not between the range and the
  touch, where a fitted intersection curve strays by up to its fit tolerance, more than the flank's
  gap. A coincidence through the touch reaches it, and one that ends where a curve ends
  is not a flank; along a coincidence the gap is rounding and shows no growth either way. The
  range is dropped before point contacts move to the curve ends (`move_contacts_to_curve_ends`),
  which leaves a point whose stretch to the end crosses a range where it is: the flank's points
  on the far side of the range then move to the touch.
- **A line a hair inside a circle.** A line in a circle's plane, δ inside the circle, crosses
  it at two points `2√(2rδ)` apart and stays within δ of it between them, as two circles do.
  `line_coplanar_circle_intersect_distinct` (`geom/geom_utility/p1.rs`) takes an overlap below
  its contact tolerance as one touching point at the chord's midpoint, unless the whole circle
  lies within that tolerance of the line. `Ellipse::intersect_with_line` (`curve/ellipse/p2.rs`)
  moves that point to an end of the arc or the line that lies on the stretch between the two
  crossings, so an arc that starts on the line there touches it at its start. The two sites
  above that pass `contact_tol_3d` for circles pass it when one of the two is a line: an edge
  against a face's intersection curve
  (`curve/ellipse/p2.rs` through `intersect/curve_class/p9.rs`), and the curve/surface
  classifier's line against an osculating circle where an edge meets a face
  (`surface/surface/p4.rs`). A flat side a hair inside a round tangent to it then touches the
  round's end circle at one point. A line tilted out of the circle's plane keeps its crossings:
  it meets the circle where it pierces the plane, and the chord's midpoint lies off the line.
  The other line/circle callers keep upstream's zero. The touching point is found only where
  it lies on both curves' intervals.
  The wall such an arc sweeps reads the contact the same way against the plane of that
  flat side (`surface/cone/p3.rs`, with `contact_tol_3d` as the rim does, where either
  surface is a face's): the plane, a hair inside
  the wall, crosses it in two lines and stays within that hair of it between them, and its
  tangent line is off a face that starts between them. Such a face touches the plane along
  the end of its sweep that lies between the tangent line and the crossing, one tangent
  line, where it used to keep the crossing a little further on. The face's edge at that end
  lies on the plane by every tolerance, so the crossing was a second line of the same
  contact: the flat side's edge was split at both, and the piece of intersection between
  them had no vertex to end on (a plate cut by notches whose entry arcs are tangent to its
  side). The touch is taken only along an end that the requested part of the wall holds and
  where its line meets the plane's own patch; else the crossing stays.

**The refusal that names a tangent contact.** The kernel's boolean has none today. The `ssi` crate
refuses a contact it cannot prove with `Error::Tangent`, code `BOOLEAN_TANGENT`, carrying where it
is (`ssi/src/lib.rs`), but nothing in `crates/solidcore` calls it yet ([ssi.md](ssi.md) §9). In the
port, a tangent contact the intersector misses shows up later as a misclassified face set or an
`SM_ERR` from the merge (§8). Wiring a named tangent refusal into the boolean, with the contact's
place, is open work.

## 7. Definitions through trimming

A boolean cuts faces; it must not change what their surfaces are. "The definition is a set's"
([definitions-and-certificates.md](definitions-and-certificates.md) §3.3): a piece of S trimmed by
an edge is still on S, so a trimmed face keeps its definition.

**What keeps it.** A face split by an imprinted edge gets a copy of its surface from
`Surface::copy_and_add_analytics` (`topology/brep/p9.rs`, `make_edge_in_face_with_vu_with_curve_identity`), and a face
copied from B into A does too (`merge/p2.rs`, `make_faceuse`). That copy carries the definition
(`surface/surface/p2.rs`, `carry_definition`): a `ProceduralBlend` is copied whole, and a
`KeptDefinition` (an exact round, a corner, a draft) rides on the copy. A sweep's definition is kept
only on a B-spline piece, since it is read point for point on its net. The test
`a_boolean_cuts_a_reopened_round_on_its_definition` (`solidcore/tests/wr698_gn_round_is_its_definition.rs`)
cuts a cylinder through a round, once as built and once rebuilt from its record, and checks the same
volume, the same edges on the round, and every such edge on S within its own tolerance.

**Where exact edges still get fitted trims.**
- A curve from a closed form (a line, circle or ellipse, [ssi.md](ssi.md) §5) has no UV curves; its
  trims are made by dropping the 3D curve onto each surface (`Surface::drop_curve`,
  `intersect/surface_drop_lib`), which traces and fits a cubic within the drop tolerance. So an exact
  edge can carry fitted trims. The exact pcurve of a plane section on a cylinder
  (`surface_drop_lib/cylinder_section.rs`) replaces one class of them; sphere, cone and torus
  sections are open.
- A traced SSI curve's UV curves are cubic Hermite interpolants, checked at samples and not certified
  ([ssi.md](ssi.md) §6.1); the imprint passes a copy of them to `MakeEdgeInFace`
  (`topology_intersector/p1.rs`, "Intersection trims").
- A face B gives to A is built without B's own trims; they are copied over where the new edge
  runs on a copy of B's edge curve over the same interval (`merge/p2.rs`), and the other edge
  uses get new ones. The copy is made only onto an edge `MergeEdge` copied from B's own edge
  (an edge the intersection made keeps its dropped trim, unless B's trim ends at a
  singularity of the surface, where the drop finds no parameter), only when the new edge's
  tolerance is at least B's, never for a line edge on a plane (`Plane::drop_curve` is exact
  there), and only when the surface copy keeps B's parameterisation.

## 8. Failure modes and refusals

**What a misclassification does.** A class is given to a whole flooded face set (§4.1), so one
wrong decision moves whole material: a set of A's faces called inside B in a union is deleted and
the result loses that part of A, or keeps a face that should have gone and is no longer closed. That
is why a classification bug looks like "half the part vanished" or "an inner wall stayed", not like a
small error.

| mode | what goes wrong | port today | open |
|---|---|---|---|
| SSI misses a contact or a branch | no common edge where the boundaries meet; a face set is flooded across the true crossing | wrong class for the whole set; [ssi.md](ssi.md) §7's gaps | a proved SSI ([ssi.md](ssi.md) §4.2) wired into the boolean |
| ray grazes a face or hits an edge | the parity of the crossing is ambiguous | lowest-dimension hit and its local sector; a hit at the start fails that ray, and the next axis is tried; no ray finds a region: `SM_ERR` | a certified classification (§4.3) |
| faces within 10° at a common edge | the binormals do not separate the sector | stepped off the edge at growing distances; 0.5° is taken as coincident | an enclosure of the sector test |
| coincident faces not recognised | the coincidence check is sampled off the plane | the pair goes to SSI with parallel normals | coincidence from the definitions (§5) |
| near-coincident planar strip | a crossing fixed only to tol / sin θ, its ends unrelated | upstream's `make_faceuse` `SM_ERR`; the crossing is no event, decided before the face pair is intersected | curved strips; offsets where the crossing's distance to an event equals the tolerance |
| an intersection loop stays open across a cut | two curve ends on an edge with a loose tolerance lie apart (each face's curve ends where its own surface meets the other face, up to tol / sin φ along the edge), or one ends inside a face; the face is not cut through and the flood crosses the gap | the two causes found are closed before the imprint: one proto vertex per crossing of an edge with a face, and a curve end classified by the face's trims (§5); a loop still open is classified as before, wrong for the set flooded across the gap | a refusal by name for a loop left open across a cut; a crossing whose piece of the edge is not proved inside the band |
| an imprint fails half way | both bodies are partly imprinted | the imprint's error is returned, not overwritten (`topology_intersector/p2.rs`) | |
| union deletes a face between the outside and a solid | the solid merges into the infinite region | after post-processing, a union is refused with `SM_ERR` naming the face (`merge/p4.rs`) if the solid side of any such face (the uses of that solid's faces that stay) lies in no solid region, even when another solid region is left; when none of those uses is left, it is refused if no solid region is left | a leak whose solid's remaining face uses all end in solid regions is kept without a check that the union is right (no known input records one) |
| result not a solid | an open or inward body from solid operands | ops refuses with `Error::NotSolid` under the closed contract, or `Error::NotSolidBecause` naming the test under the proved contract | |
| a join of many bodies fails one pairwise union | a pair the level scheme cannot classify (`compute_point_parameters` finds no single nearest point on a face's surface, `union_all`) refuses the whole join; the cause is unresolved, as for any pairing | the typed refusal (`Error::NotOneFoot`, its count carried) is retried on fresh copies one body at a time against the result so far, a mitigation that builds the join without classifying that pair; a pair that keeps failing fails the join, and any other failure keeps its own error (`core/boolean.rs`, `union_all_fifteen_chain` in `core/boolean_tests.rs`) | a kernel fix for the classification itself |
| result empty | no faces left | ops returns `None` (`boolean`) or `Error::Empty` (`union_all`, `imprint`) | |

**Refusals.** The kernel refuses with `SolidError` statuses: `SM_ERR` (a failed classification, a
face list past `MAX_FACE_LIST`, a coincident merge past its passes, the leak above, an invalid input
to `Slice`), `InvalidInput` for what upstream dereferences null, and `Unported` for a callee not yet
ported, which always propagates (`ignore_soft`, `topology_intersector/p1.rs`). Many upstream
statuses are ignored as upstream ignores them, listed in each file's Deviations; a refusal that
names its cause and place, as the blend's do, is the target and not yet the boolean's.

## 9. Invariants

A reviewer can check each of these on a change to the boolean.

1. Every surface hit or explicitly surface-solved decision point is a root on the surfaces'
   definitions S. An evaluated operand-edge ray origin is only a seed and need not itself be a
   root; a net answers only a guess or a box, and a box is grown by ε.
2. No display mesh or tessellation is read by any step.
3. A face's class is inside, outside or on, from a sector at a common edge, a ray, or a coincident
   pair; nothing is kept or dropped for any other reason.
4. Faces connected without crossing a common edge have one class.
5. The keep rules are the regularized operations of §4.4, and a region's solid flag follows
   `result_solid`, or `deleted_face_sides` for a region joined with no record of its sources.
6. A trimmed or copied face keeps its surface's definition (§7); a change that drops one is a bug.
7. Two bodies' imprinted edges are mated pairwise; unpaired common objects are `InvalidInput`
   (`pairs`, `merge/p4.rs`).
8. Every loop is bounded: the ten-pass loops of `mark_common` and the classify step,
   `MAX_FACE_LIST`, `MAX_EDGE_LIST`, `MAX_COINCIDENT_SURFACE_PASSES`; no input-dependent recursion.
9. The same input gives the same output, whatever the thread count: mates are `BTreeMap`s over
   handles, never address order (`topology_intersector/p1.rs`).
10. A fix changes the default result and lists what changed; a change that moves a `boolean.*`
    golden shows each moved case with numbers and pictures and why the new result is right.
11. A change's tests check exact volumes, face, edge and vertex counts, and distances to S; never
    timings.

## 10. Status

**Built.**
- The port of `SmMerge` and `SmTopologyIntersector`: units `boolean.api_brep`, `boolean.merge_p1` to
  `_p5`, `boolean.topology_intersector_p1` to `_p3`, `query.brep` and `intersect.point_class`, each
  checked against its golden (`plan/units/`).
- Point solves on S with ε-grown cache boxes (§4.3).
- Definitions carried through face splits and copies (§7), tested on a round.
- The ops entry points with the result contract (`solidcore-ops/src/boolean.rs`).
- The rig: 130 boolean cases in `solidcore/tests/kernel_cases/booleans/`, two listed in
  `kernel_cases_known_failing/` (the two 3012 sliver cases: the kernel absorbs the 1e-4 wide faces, so it
  returns 8 faces, 18 edges and 12 vertices instead of 10, 24 and 16; #15669).
- No opt-in fixes are left for the cases in §5, §6 and §8. Always on: the operand's exact trims on
  a face B gives to A (WR-361), the union's solid-leak refusal (WR-619), a boolean
  vertex placed again by a later edge crossing with a smaller misplacement bound (gap over sine) only
  where a certificate proves the move stays one point (WR-410, §5), the outer shell first, the clean-up of slits and the split of a coincident
  definition across a split point (WR-533). Every boolean, 3D included, skips the face pairs
  whose boxes lie apart, and a new planar face leaves in place the inner loops whose boxes lie
  outside its outer loop's box (WR-527), always.

**Open.**
- Retiring those flags: each fix on by default with its old path deleted (AGENTS.md §4b).
- A certified classification: the ray's parity and the radial sector test proved with exact
  predicates or enclosures, not thresholds (§4.3).
- Coincidence decided from the definitions rather than by samples (§5), and a strip rule for curved
  faces within tolerance.
- A named tangent refusal in the boolean, with where (§6).
- The `ssi` crate's proved intersection wired into the boolean ([ssi.md](ssi.md) §9).
- Exact pcurves for sphere, cone and torus sections, so that exact edges stop carrying fitted trims
  (§7).
- Refusals that name the cause and the place in the body, in place of bare `SM_ERR`.
- Circles overlapping past touching by more than the contact tolerance and up to the
  intersection tolerance cross at every site, and two such holes are refused as not a solid
  (§6, "Circles a hair less than tangent").
- A curve the marching intersection traces inside a lens of two circles in contact, from the
  touch to a crossing along both edges (a boss 5e-8 past a rim at a zone tolerance of 1e-7),
  is kept as a curve; the contact makes it no event (§6).
- The coincidence rule in migration (§5, "The coincidence rule"): the side test of an edge use,
  the proto vertices (`find_proto_vertex_within`, `DeclaredContact::overlaps`) and a seam's end
  vertex in the seam stretch of `edge_solutions` read `coincidence_tol_3d`; the intersection
  curve along an edge (`edge_solutions`), a vertex on a line edge (`split_edges_at_vertices`),
  the vertex pair of a proto vertex, the exactly placed combine radius, the interval along an
  edge (`classify_edge_interval`), the
  check's Surface test 6 and the import healer still read the intersection tolerance. Until
  they move, a groove more than 1e-5 and up to 2e-5 deep (at the default 1e-5) is lost by the
  difference and a boss overhanging a rim by as much is dropped by the union. The intersection curve, the
  proto vertices and the side test together restore the groove; the interval along an edge
  restores the boss.

## 11. If you're stuck

- **Find the step first.** Run the boolean with `MergeOptions::imprinting` (or
  `BooleanOperation::Imprint`, ops' `imprint`) and look at the imprinted edges. If they are wrong or
  missing, the bug is in step 1 or 2: reproduce the face pair at the surface level as
  [ssi.md](ssi.md) §10 says. If they are right, it is classification or select.
- **Look at the classes.** `BooleanOperation::ImprintClassify` (or
  `MergeOptions::imprint_and_classify_faces`) stops after select and labels each face kept or deleted
  (`label_classification`, `merge/p4.rs`). Draw the body with `cargo run --release -p diagnostics --
  FILE.smb --out DIR --highlight <faces>` and find the face set whose class is wrong.
- **Then find which test gave it.** A set at a common edge took its class from a radial sector:
  check the binormal angles at that edge (§4.2), and whether the faces there are tangent or
  coincident. A set with no common edge took it from a ray: check whether the ray from the point on
  its first edge grazes a face or runs through an edge of the other body (§4.3).
- **Missing material** is a face set called inside (union) or outside (intersection) wrongly, or a
  coincident pair taken for opposite sides. **An open result** is a face kept on one side and
  dropped on the other, usually a common edge one body has and the other does not.
- **Check the flags.** The fixes are off by default; SolidCraft names its own. A result that differs
  between the two is usually a flag (`UpstreamFixes::NAMES`).
- **Check tolerances last.** A near-tangent or near-coincident pair within the zone tolerance is §5
  or §6, not a classification bug; the answer is a decision made once, never a widened tolerance.
- **Read the literature before designing.** Requicha and Voelcker [2] for the algorithm,
  Mäntylä [4] for neighbourhood classification, Hoffmann, Hopcroft and Karasick [8] and Segal [9]
  for robustness, Keyser et al. [13] for exact boundary evaluation on curved solids.
- **Fix the cause.** No second ray direction tried because the first answer looked wrong, no retry
  with a looser tolerance. A failure several cases share has one cause.

## 12. References

1. A. A. G. Requicha. Representations for rigid solids: theory, methods, and systems. *ACM Computing
   Surveys* 12(4) (1980) 437–464. doi:10.1145/356827.356833
2. A. A. G. Requicha, H. B. Voelcker. Boolean operations in solid modeling: boundary evaluation and
   merging algorithms. *Proceedings of the IEEE* 73(1) (1985) 30–44. doi:10.1109/PROC.1985.13108
3. R. B. Tilove. Set membership classification: a unified approach to geometric intersection
   problems. *IEEE Transactions on Computers* C-29(10) (1980) 874–883. doi:10.1109/TC.1980.1675470
4. M. Mäntylä. Boolean operations of 2-manifolds through vertex neighborhood classification. *ACM
   Transactions on Graphics* 5(1) (1986) 1–29. doi:10.1145/7529.7530
5. M. Mäntylä. *An Introduction to Solid Modeling*. Computer Science Press (1988).
6. K. J. Weiler. *Topological Structures for Geometric Modeling*. PhD thesis, Rensselaer Polytechnic
   Institute (1986).
7. C. M. Hoffmann. *Geometric and Solid Modeling: An Introduction*. Morgan Kaufmann (1989).
8. C. M. Hoffmann, J. E. Hopcroft, M. S. Karasick. Robust set operations on polyhedral solids. *IEEE
   Computer Graphics and Applications* 9(6) (1989) 50–59. doi:10.1109/38.41469
9. M. Segal. Using tolerances to guarantee valid polyhedral modeling results. *Computer Graphics*
   (SIGGRAPH '90) 24(4) (1990) 105–114. doi:10.1145/97880.97891
10. D. J. Jackson. Boundary representation modelling with local tolerances. *ACM Symposium on Solid
    Modeling and Applications* (1995).
11. J. R. Shewchuk. Adaptive precision floating-point arithmetic and fast robust geometric
    predicates. *Discrete & Computational Geometry* 18(3) (1997) 305–363. doi:10.1007/PL00009321
12. C. K. Yap. Towards exact geometric computation. *Computational Geometry* 7(1–2) (1997) 3–23.
    doi:10.1016/0925-7721(95)00040-2
13. J. Keyser, T. Culver, M. Foskey, S. Krishnan, D. Manocha. ESOLID: a system for exact boundary
    evaluation. *Computer-Aided Design* 36(2) (2004) 175–193.
