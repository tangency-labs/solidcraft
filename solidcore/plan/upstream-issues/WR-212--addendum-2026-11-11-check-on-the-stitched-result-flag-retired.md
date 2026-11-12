Updates: WR-212

#### Updates: WR-212 (2026-11-11). The check is the default, on the stitched result; flag retired

- `UpstreamFixes::fillet_refuse_non_manifold_result` is retired. The public edge fillets
  (`fillet_edges`, `fillet_edges_per_edge` and `variable_radius_fillet` in
  `crates/solidcore/src/blend/fillet_executive/api.rs`) refuse a result that is not
  closed and manifold (`is_closed_manifold`: no shell vertices, every edge manifold,
  whatever the regions say) when the body was, and put the body back. The refusal is a
  typed blend failure (`BlendFailure::Failed`, status 1001) naming no entity; a cancel
  during the fillet stays `Cancelled`. The executive's error info no longer records it
  (the wrappers drop their session). Upstream returns success with the open body.
  `circular_fillet` and `chamfer_fillet` never had the check and still do not.
- **Where the check runs.** On the body after `StitchAndOrient`, no longer at the end of
  `DoFillet`. Before the stitch it refused `Fillet/box_diff_cyl_big_rad.smb` edge 12 at
  r 28, which the stitch closes (2026-10-26 addendum). `FilletSession::do_fillet` and the
  generic `FilletExecutive::do_fillet` return the filleting's status, as upstream; a caller
  that drives a session checks its own result, as the ops layer's result contract does.
  The offset's put-back of its blends (`OffsetFacesExecutive::reblend` in
  `crates/solidcore/src/offset/offset_faces.rs`) drives a session and stitches; it makes
  the same check after its stitch and refuses as `OffsetFacesRefusal::NotPutBack`, as it
  did with the flag, which SolidCraft's preset turned on.
- **Which bodies.** Only a closed and manifold body must stay one. A sheet, or solids that
  touch along an edge (the bore tangent to a cube's face in
  `crates/solidcore/tests/issue_453_bore_rim_smooth_end.rs`), is not closed and manifold
  before the fillet and is not checked. The opt-in check tested every edge of the
  result, so it refused every fillet of such a body.
- **Trigger with the default kernel:** the four vertical edges of a 14 x 6 x 10 post
  rounded r 3.5 (`crates/solidcore/tests/issue_1073_opposite_rounds_cross.rs`): the two
  rails on each 6 mm face cross, and the default kernel, which has no band test, returned
  success with a body that is not a closed manifold solid. It is now refused and the post
  is put back with its six faces. Two earlier triggers no longer reach the check: the
  thin wall (an upside-down U, 60 x 20 x 50 less a cavity that leaves a 3 thick top wall,
  both outside top edges rounded R18) is refused by the default kernel in
  `blend::fillet_geom::trim_original_surfaces`, and the R4 round over the hole of
  `fixtures/solidcraft/324-round-beside-through-hole.json` in keep features.
