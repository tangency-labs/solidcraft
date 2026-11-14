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
closes the gaps (`fix_gaps`); it repeats at most ten times. The classify and select steps also repeat
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
neighbourhood classification [4, 6]. The sector's region gives the class: `region_keep` reads its
void flag.

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

This is the problem Segal's tolerant modeller and Jackson's local tolerances address [9, 10]: two
entities within tolerance are one, and a decision across that band has to be made once and kept by
every later step. The port's strip rule is one such decision; a general one for curved faces within
tolerance is open.

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
  `topology/brep/p3.rs`, `remove_slit_loops_where_solids_touch`).
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

The blend definitions themselves are opt-in (`fillet_round_as_definition`); without it a round is a
bare net and there is no definition to keep.

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
- A face B gives to A is built without B's own trims and gets new ones, unless
  `keep_operand_uv_trims` copies them where the new edge runs on a copy of B's edge curve
  (`merge/p2.rs`).

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
| an imprint fails half way | both bodies are partly imprinted | the imprint's error is returned, not overwritten (`topology_intersector/p2.rs`) | |
| union deletes a face between the outside and a solid | the solid merges into the infinite region | with `boolean_union_refuses_solid_leak`, refused with `SM_ERR` naming the face (`merge/p4.rs`) | on by default |
| result not a solid | an open or inward body from solid operands | ops refuses with `Error::NotSolid` | |
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
  `known_failing.json` (the two 3012 sliver cases: the kernel absorbs the 1e-4 wide faces, so it
  returns 8 faces, 18 edges and 12 vertices instead of 10, 24 and 16; #15669).
- Opt-in fixes for the cases in §5, §6 and §8: `boolean_union_refuses_solid_leak`,
  `boolean_outer_shell_first`, `boolean_cleanup_removes_slits`, `keep_operand_uv_trims`,
  `boolean_vertex_by_best_crossing`, `boolean_splits_definition_across_split`,
  `boolean_skips_faces_apart` (`solidcore/src/kernel.rs`).

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
