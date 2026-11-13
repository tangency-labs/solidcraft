Updates: WR-650

#### WR-650 (update, 2026-11-03). A trace's extension stops where it closes back onto the traced curve

Extending a surface of revolution round its turn, past the closure, lets two offsets meet in a closed curve along any edge, which does not repeat its UV (tangency-labs/solidcore#2500).

- **Defect:** under SolidCraft's fixes, #1945's round of a bar's top front edge, run over a concave round's torus up to the boss, meets the torus's offset in a closed oval. The extension traced back from the End's section ran round the oval past the trace's own span: the end-point and loop checks (`sm_IsPointOnCurve`) compare UV too. The fillet surface covered its span twice, the End's rail ends dropped onto the second sheet, and its cap edge on the boss failed (1001, `CalcFilletIntSideFace`). A fixed length (5 x the offset, upstream's bound along arcs) built it but cut short extensions that wr685's cliff overflow, wr697's closed edge and wr698's reopened round need.
- **Change (with `offset_extension_as_itself`):** `SurfaceIntersector::extension_stops_on_closure` (set by `c3_make_intersector`): an extension past the end without an end point, or back from the start, stops where its span comes back onto the traced curve's other end in 3D (`is_point_on_tsect_span_3d`, once it has left that point), and keeps half of the gap it ran over, the other half being the other end's, as this record's extension keeps half the gap of a turn either end. A trace that never closes is unchanged.
- **Test:** `crates/solidcore/tests/issue_1945_rail_uv_trim.rs`.
