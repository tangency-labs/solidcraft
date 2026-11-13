Updates: WR-697

#### WR-697 (update, 2026-11-04). The blend corner is the default; `fillet_blend_corner` is removed

The blend corner (`SmBlendCorner`: the Turn, Join, Ball, End, Setback and Mixed events, smlib#79,
#82, #104, #113, #124, #125, and solidcore's own corner work under the flag) is no longer
opt-in: `UpstreamFixes::fillet_blend_corner` is removed and every place that read it takes the
flag's side (tangency-labs/solidcore#2400, the owner's decision of 2026-11-04). Nothing keeps the
old path, and no preference does: a saved design rebuilds with the blend corner.

- **Always on now:** `FilletCorner::create`'s classification (the Join, the End, the Turn, the
  Mixed corner; the Ball and the Setback in the session); the End's rail end on the end face's
  surface (`CalcRailIntEndSurface`) and its edge geometry; the trace run on to the edge's ends
  (`c2_trace_to_edge_ends`); a fillet face keeping its own UV trims (`MakeFaceWithCurves`,
  32fd80a); the rail edge use on the fillet's own face (794946e); the leaking trim as an error
  (d657afa) and the multi-face trim edges marked before the trims; the side edge continued across
  a seam's vertex (63c7962); the refusals that keep the corner's reason (smlib#124, 41a39ec); the
  tangent rollover's ends (#2406); the ball's triangle on a collapsed offset (#2403) and the torus
  read as a quadric; `sm_CreateUVCurve`'s degenerate sides; the horn torus kept by
  `SmTorus::RebuildSTEPFromNURBParameters` (af7a4d9); the ball patch read from a file; the fillet
  vertex contact bounded by the body's resolution (#1999); `FlushCurve`'s short last span merged
  only when the merged span keeps the trace's accuracy; the open setbacks and each round's side
  of the body in `TrimAtCreases` (smlib#125); a setback asked where none is built refused.
- **WR-600's flag goes too:** `fillet_nx1_tangent_side_edge_same_convexity` was read only through
  `tangent_seam_ends`, which the blend corner made true, so it no longer changed anything. The
  tangent side edge's convexity and the tangent seam's tangent end are always on (see
  `WR-600--addendum-2026-11-04-flag-removed.md`).
- **The default kernel** is the blend corner with every other fix off. Its rounds' sections are
  the polynomial arcs within 0.05% of the radius (WR-708), so a Mitre turn's exact plane section
  meets them only to that accuracy; the closed forms hold with the companion fixes
  (`fillet_exact_arc_sections`, `fillet_exact_round_classes`), as SolidCraft runs. Tests of other
  fixes whose cases go through a corner run under every fix where their expected values are the
  closed forms (`issue_1060_offset_mitred_rounded_box_top`, `issue_416_fillet_seam`,
  `wr603_trace_from_tangent_point`, `wr642_mitred_concave_face_corners`).
- **Tests:** tests that set the flag drop the setting; tests that compared the flag on and off
  keep the flag-on half; tests that asserted the old corners' results on the default kernel (the
  pinned refusals, volumes and marks the blend corner changes) are removed with the behaviour
  they pinned. The setback tests that set a setback on a three-round corner with
  `set_corner_setback` alone mark the vertex with `add_set_back_vertex` first, as SolidCraft
  does, so the corner is the N x N one whose setback path is kept (#2365); their values are
  unchanged.
