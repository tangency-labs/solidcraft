# mass.integrator: each iso-u piece's end is evaluated from its own knot span (2026-11-05)

- **Units:** `mass.integrator` (34 known mismatches re-valued), `blend.fillet_executive_p3`
  (14 re-valued). `boolean.api_brep`, `topology.face_p2`, `offset.offset_executive_p2`: no
  value moved. Counts unchanged in every unit (same 624 known mismatches, same cases).
- **Change:** WR-670 (#2091) becomes the only path: `UpstreamFixes::precise_properties_span_end_from_below`
  is deleted (kernel field, difftest name map, `fixes.json`, the #2260 list, the #2091 tests).
  `OdeArea::evaluate` integrates each iso-u piece `[a, b]` (between the surface's v knots,
  WR-494) with its end `b` evaluated from the span below (`bVFromLeft = FALSE`); a piece
  descending to `b` (the curve point below the base, one piece) keeps the span above.
- **Root cause:** the integral over a piece is of the integrand's restriction to the piece's
  knot span; its value at the piece's end is the one-sided limit from inside. Upstream
  evaluates there with `bVFromLeft = TRUE`, the span above. Where the surface's v derivatives
  jump at the knot, the piece's last sample is the next span's value: at a C0 knot (#1435's
  rational patch, double v knot 0.5) the steps shrink toward the knot and underflow (status
  1001) at requested accuracies 1e-9 and tighter on the whole face; at #2091's G2-not-C2 knots
  each piece costs about 5x the evaluations.
- **Confirmed:** `multispan_face_holds_tight_accuracies`
  (`crates/solidcore/tests/issue_2091_precise_volume_spans.rs`): the whole #1435 face, every
  property (flags 63), against a tensor Gauss-Legendre reference (24 points a knot span in each
  direction, agreeing with 32 points to 1e-14; no node on a knot). With the end from the span
  above: status 1001 at 1e-9. From the piece's own span: every component group holds, worst
  (volume second moments) 2.2e-11 at 1e-9, 2.2e-12 at 1e-10, 2.2e-13 at 1e-11 (error over the
  group's largest magnitude); area 1.2e-14, 1.7e-15, 1.1e-15.

## The moved cases (Rust old -> new)

Every move is far inside the requested accuracy; the C++ side is unchanged.

- `issue1435_multispan_full_*` area (reference 11.774753793233748, Gauss-Legendre):
  eps 1e-3 flags1 11.774779172613957 -> 11.77465618008263 (2.2e-6 -> 8.3e-6 off, asked 1e-3);
  eps 1e-3 flags63 11.774752414334227 -> 11.774752410269487 (1.2e-7 off);
  eps 1e-6 flags1 / query2 / query4 11.774753501933382 -> 11.774753489900778 (2.6e-8 off);
  eps 1e-6 flags63 11.774753791494568 -> 11.774753791492468 (1.5e-10 off);
  eps 1e-8 flags1 11.774753790940473 -> 11.774753790890767 (2.0e-10 off);
  query3 11.77475375578176 -> 11.77475375528007 (3.2e-9 off).
  eps 1e-8 flags63 `out[14]` (volume first moment x; reference -0.06854952434836367, signed
  by the orientation) -0.0685495243146485 -> -0.06854952431479716 (3.4e-11 absolute,
  2.5e-11 of the group's largest magnitude 1.343).
- `issue1435_multispan_sub_*` (no closed reference here): moves of 3.2e-6 (eps 1e-3 flags1),
  8.9e-10 (1e-3 flags63), 1.3e-9 (1e-6 flags1, query2, query4), 7.5e-13 (1e-6 flags63),
  2.7e-12 (1e-8 flags1), 1.5e-15 (1e-8 flags63), 1.2e-11 (query3), relative.
- `issue1435_multispan_huge_*` 4.9e-9 (eps 1e-4), 2.2e-14 (1e-8); `tiny_*` 3.3e-10 (1e-4),
  6.1e-11 (1e-8), relative.
- `blend.fillet_executive_p3`: 13 bottle cases 33.11819077304608 -> 33.11819078265202
  (2.9e-10 relative, asked 1e-4); `source-file-box_w_slot-all-edges` 5.91438986260717 ->
  5.9143898616289645 (1.7e-10).
- `blend.fillet_executive_p3` `source-file-corner_box-edge9` still fails with the same values
  (`result_geometry[212]` cpp 9.528318886810855e-1, rust 9.528316525973994e-1): not an
  integration case.
