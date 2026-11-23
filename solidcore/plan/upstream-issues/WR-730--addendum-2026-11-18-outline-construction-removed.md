Updates: WR-730

#### WR-730 (addendum 2026-11-18): the outline construction is removed; the overflow is trimmed at its crease

- **Severity:** Wrong result (the opt-in construction; the default is unchanged)
- **Area:** Blends
- **Status in the port:** the flag `UpstreamFixes::fillet_round_ends_on_outline` is gone, with `crate::blend::fillet_outline` and `FilletSession::end_round_on_outline`. A round bigger than the wall it rolls on is built on the wall continued past the rim and trimmed at its crease, as it was with the flag off.
- **Why:** the design (`docs/generic-blend.md` §3.4) makes the crease trim the overflow and names the round that keeps the edge "a possible future option, never the default". The flag was the only way to that construction, and its fixed path was wrong where it was not the subject of its own tests:
  - SolidCraft's tests with closed forms of the crease trim moved: `fillet_over_a_cliff` 721.832 against 693.205, `a_round_wider_than_its_faces_overflows_onto_its_tangent_sides` 6125.719 against 6107.529, `taper_follows_the_room_along_the_edge` 4210.926 against 4195.579;
  - `hostile_inputs_are_errors` built a round larger than the cube where a refusal is expected;
  - 4 overflow tests that build with it off were refused with status 1001.
  SolidCraft never turned it on (`HELD_BACK`), so nothing that ships changes.
- **Tests:** `crates/solidcore/tests/wr730_round_ends_on_outline.rs` became `round_past_its_wall_on_the_continued_wall.rs`: the plate (r 20, 50, 90) and the disc on the continued wall to 1e-9 against their closed forms, the hemispherical end, the round breaking into a hollow refused, and the round ending on a curved face refused. `a_round_a_hair_past_its_wall_is_left_as_it_is` (rig 5158, r 10.01) is deleted: it asserted only that the flag left that case unchanged, and the rig's `fillets/5158` still covers it. `solidcore-ops`' `issue_2845_stepped_wall.rs` asserts the crease trim (`trimmed_at_its_crease`, its other closed form) for the wall alone past its width, as it already did for the recorded wall: 9337.675944094, 9296.076593956, 9205.017441031 at r 4.5, 5, 6.
- **Left open:** the round that ends on the body's outline (#2541's request) is not built.
