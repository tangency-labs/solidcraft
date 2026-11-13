Updates: WR-616

#### WR-616 (update, 2026-11-02). The fork's setback corner fill, `SmBlendSetback`, ported (no flag: nothing calls it yet)

The SolidCraft fork (tangency-labs/smlib PR #79: `870c970`, `beba428`, `3fdb76b`, `52da453`)
adds `SmBlendSetback`, a module for the setback corner whose fill meets both the eased rounds
and the planar faces to G1 or G2, which is what this record says SMLib has no way to do:

- `Distances`: each round's setback by the owner's rule, `max(1.5 r, least + 0.25 r)`, the
  least setback being where the rails of the rounds beside it cross on their common face;
- `EaseSurface`: the ease of #1376 (`sm_EaseSurface`), moved out of the executive unchanged;
- `FillHole`: the setback split, one setback patch per round (degree `2k + 1` across, its
  first `k + 1` rows the round's homogeneous jets at its end, leaving the round at the speed
  of its whole spoke, `52da453`) and an n-sided interior that is one polynomial map of a
  regular n-gon, split into n quadrilaterals at its centre. The face conditions hold
  exactly; least bending energy picks among the maps that satisfy them. A refusal's
  reason is handed back (`3fdb76b`);
- `Containment`: a certified bound on how far any patch stands past a face, from the control
  points (all weights positive). This bound is what WR-461's `N_FillNSidedHole` patch lacks.

**Rust port:** `crates/solidcore/src/blend/blend_setback.rs` (`distances`, `ease_surface`,
`fill_hole`, `containment`, with `Refusal` carrying a refusal's status and reason). The
executive's ease (`blend/fillet_executive/session/ease.rs`) now calls `ease_surface` there.

**No `UpstreamFixes` field.** Neither the fork's SMLib (at `5e8c9c1`, PR #79's merge into
`replay/1.0.1`) nor the kernel calls `Distances`, `FillHole` or `Containment`: only the
fork's capi tests do. So the new rule, the fill and the speed change alter no existing
result. The one change on an existing path is the move of `EaseSurface`, which is
verbatim (diffed against `e74bbd3` and `870c970^`; the only Rust-side difference is the
`operation` name in its errors, with the same statuses). The default stays byte-identical to the
pinned C++. The flag belongs to whichever change first routes a corner through `fill_hole`.

**Tests:** `crates/solidcore/tests/fork_870c970_blend_setback.rs` ports the fork's seven
`SETBACK_CASES` with its values and tolerances: the box corner's distances (3, 3, 3;
radii 2, 3, 4: 4.5, 4.75, 6) and its refusal; G1, G2 eased, G2 order-2 rounds, G2 G^n
section rounds and unequal G1 fills. All 18 seams of each reach the fill's order by the
graph grader, with normals within 2.5e-15 and the G2 curvature across within 1.9e-14.
The certified containment is at most 7.5e-16. The G2 fill of uneased rounds is refused,
with its reason. **Confirmed:** yes, against the fork's own C++ build (`5e8c9c1`,
the harness's flags). A throwaway driver ran the six fills on both sides (plus the same fills
with one round reversed along its rails, corner above, and one reversed across). Every
setback, eased round, G^n round, patch control point and weight, the containment bounds and the
refusal reason agree bit for bit (2063 dumped lines, 0 differing).
