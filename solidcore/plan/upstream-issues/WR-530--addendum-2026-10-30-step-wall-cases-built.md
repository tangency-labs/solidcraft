#### WR-530 addendum (2026-10-30). The fork's own step-wall cases are built
Updates: WR-530 (and its 2026-10-29 step-walls addendum)

- **What changed:** the 2026-10-29 addendum listed two of the fork's cases as open in the port: the split block's piece x > 30 under the imprinting box, refused by `extend_trim_edges` (1001), and the boss wall moved off its round, stopped by `close_loop_gap` with 1043.
- **Cause:** both were faults in the tests, not the port. The tests gave the moved face a surface built afresh, which has a different parameterization from the face's own surface:
  - a plane with its origin at x = 30, where the piece's own plane has its origin at x = 0;
  - an r11 cylinder, whose u is arc length, so the r10 face's u range covers only 10/11 of it.

  The local operation lifts a moved face's edges onto its new surface by their UV, so the edges landed elsewhere. The step edge came out at x = 60, and the boss's r11 edge as a 328 degree arc. Breakpoint traces of `is_step_across_edge` and `close_loop_gap` showed both.
- **Now:** the tests move each face by its own surface offset (`create_offset_surface`), as the fork's `smlib_offset_faces` does, and both cases are built:
  - the piece x > 30 up 1: 49200;
  - the boss wall out 1: plus pi * 21 * 8.

  Their tests in `tests/wr_530_local_operation_step_walls.rs` are no longer ignored.
