# A one-ULP face domain difference leaves a sliver span on one of four symmetric rounds (issue #472)

`nurbs.b_spline_surface_n_lib_p2` (`TrimWithDomain`), reached from `SmFace::ShrinkGeometry`
when `SmFilletExecutive::DoFilleting` shrinks the new fillet faces.

Inputs: SolidCraft rig case shell/3576 (tangency-labs/solidcraft, branch `kernel-smlib`):
`SmApiCreateTaperExtrude` of the 120x80 rectangle centred on the origin, height 40, draft
5 degrees, both caps; then `SmFilletExecutive` (circular sections, constant radius 4,
`SetFilletParameters(..., -1, -1, -1, -1, 1, 1.0)`, `DoFillet`) on the four sloping
corner edges. Reproduced with an `SM_API` program against the reference build
(`c9c5979`, x86-64 Linux, g++), then traced in a private instrumented copy.

Each round surface is built in three C0 pieces, u in [0, 20.1525] (start extension),
[20.1525, 60.4575] (the edge) and [60.4575, 80.61] (end extension). Two rounds,
(+x,-y) and (-x,+y), have identical surfaces with an interior knot
k = 20.152507010543033. `CalculateUVDomainFromUVTrimCurves` gives their faces
u_min = 20.152507010543033 (on k) and 20.15250701054303 (one ULP below k).
`TrimWithDomain` then:
- on the knot: snaps u_min to k; u knots `20.15250701 (x4) 60.45751479 (x3) 60.60117578 (x4)`;
- one ULP below: takes the "space trim boundary more than tol away from knot" branch and
  sets u_min = k - 10 * SM_EFF_ZERO_SQRT * (1 + |u|) = 20.15229549. Its u knots are
  `20.15229549 (x4) 20.15250701 (x3) 60.45751479 (x3) 60.60117578 (x4)`, with a 2.1e-4
  sliver span. This is the span that makes the later free-form offset diverge (#471).

Cause: the snap rule is discontinuous at u == knot. A boundary at or just outside a knot
is snapped onto it, while a boundary up to `SM_EFF_ZERO_SQRT * (1 + |u|)` inside it is
pushed away by ten times that, so it creates a span of exactly the size the rule exists
to prevent. A rounding error in the face domain picks the branch.

Fix (documented deviation behind an option whose default is upstream):
`TrimOptions::snap_roundoff_to_knots` (`BSplineSurface::trim_with_domain_with`,
`Surface::trim_with_domain_with`, `topology::face::p1::shrink_geometry_with`) snaps a
boundary that lies at most `SM_EFF_ZERO_PARAM` inside a knot onto it. That is the
tolerance `TrimWithDomain` already uses for its own containment test. The requested
domain passed to `sm_CheckTrimBoundaries` takes the knot too, so the `[B306]` widening
does not move it out to the previous knot. Both u and v, both ends. The fillet
executive's `ExecutiveAccess::shrink_face` is documented to shrink with the option on.
Checked on the instrumented C++ with the same rule: all four rounds get the 11-knot
structure with no sliver, and the inward shell of the rounded box by -2 with the top
open (`SmApiShellBrepFull(b, -2, TRUE, TRUE, TRUE, {top})`), which fails with 1001
upstream, gives a manifold solid with 19 faces. The +2 shell is unchanged (manifold,
19 faces).

Why an option: the unit's generated trim cases deliberately place bounds 1e-12 and
1e-9 from knots, inside the new window, so the faithful default keeps them comparable
with the C++. With the default, every path compares as before and no case is dropped.

Also seen, not changed: the (+x,+y)/(-x,-y) rounds start at u = 20.15253238 and the other
two at 20.15250701 (the start extensions differ by 2.5e-5). The knot structure is the same,
so it is harmless.

Validation limits: the fillet cannot run end to end in Rust yet
(`blend.fillet_executive_p1`, `CreateFilletCorners`, is todo, and no in-crate
`ExecutiveAccess` implements `shrink_face`). Unit tests in
`nurbs::b_spline_surface_n_lib::p2` cover the issue's own numbers (knot and one-ULP-below
bound: upstream sliver of 2.115e-4, none with the option), the upper and v boundaries,
and unchanged results for bounds farther than `SM_EFF_ZERO_PARAM` from a knot.

### Sweep/merge snapshots: live topology and face-use links

Units: boolean.merge_p2, boolean.merge_p5, topology.brep_p10.
The `other_wire_classify_op{1,4,5,6}` cases exposed a snapshot mismatch: C++
exports 96 topology integers; raw Rust arenas included 103 after MakeEdgeTopology.
C++ deletes detached objects while Rust retains their slots. Both merge runners
now share the complete reachable-graph serializer, preserving every live record.
Face-use links use `shell::faceuse_sibling_links`, including its arena-list fallback;
an absent optional backing field is not itself a null native link. No cases,
rejection rules or tolerances change. This correction does not qualify full
curve-copy geometry; WR-168 remains a separate existing deviation.
