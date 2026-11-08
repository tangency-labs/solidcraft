# heal.stitch_p2: region nesting tolerance divergence resolved (harness owner naming) (2026-10-27)

Updates: plan/findings/heal.stitch_p2--region-nesting-ray-fire-raises-other-edge-tolerances.md

- **Root cause:** not the ray-fire classification. On `cov_cube_gap_at_edge_tol_nest`
  both sides glue the cube's edges with region nesting on. The glue that closes the
  cube splits its region (`SmBrep::GlueEdgesGeneral` -> `TestClosureSplitRegion` ->
  `SplitClosed` -> `FindAndSetInfiniteRegion(pNewShell, ..., &sBrepBBox)` ->
  `SmShell::RayFire`). In C++ that shell ray hits a face at t = 0.94997 and its face
  classification makes the UV trim curves that raise six edge tolerances, before
  `DoStitching`'s own `FindAndSetInfiniteRegion`. Rust returned an internal error
  (`topology::brep_cache`, "edge owner is not in source B-rep") from all three shell
  ray attempts. It then dropped the shell and box (the native recursive retry) and
  fired an unclipped B-rep ray that found nothing. So the six raises waited for
  `DoStitching`'s ray. With different tolerances, that final ray picked a different
  round-0 target edge (C++ aims at the x=0/z=1 edge; its hit raises the other two
  tolerances) from a different box corner.
- **Why the lookup failed:** the heal runners import the B-rep with every curve and
  surface owned by its arena identity (`topology_brep_p1::import`), but leave the
  B-rep's `NamingScheme` `Unknown` and record no geometry owner keys. `SplitClosed`'s
  query names geometry under the glue's notification identities
  (`ArenaNotifyState`, `notify_ref`), overridden only by owner keys recorded on the
  B-rep (`BrepQuery::with_proven_owners`). So no edge matched its curve's owner.
  `DoStitching`'s own query (`BrepQuery::of_brep`, arena for `Unknown`) was unaffected.
- **Fix:** the heal.stitch_p1/p2 runners call `Brep::use_arena_naming()` after the
  import (as the boolean.merge_p3 and fillet end-to-end runners already do). That
  records the arena owner of every edge and face. The shell ray now fires from the
  same start (1.5000497693916079, ...) as C++ and finds the same single hit. The
  final ray then matches too: 3 solutions, from (1.5000948635587494, ...). Library
  code is unchanged.
- **Probe method:** private copies of `SmStitch.cpp`, `SmBrep.cpp` and `SmShell.cpp`,
  linked ahead of the prebuilt `libSMLib.a`, printing each edge's tolerance and UV trim
  curves before and after each ray fire; matching temporary `eprintln!` on the Rust
  side. None of this is committed.
- **Result:** all 22 known mismatches pass (9 in heal.stitch_p1, 13 in heal.stitch_p2,
  including cube4's split-edge interval start), so both known-mismatch files are removed:
  heal.stitch_p1 451/451, heal.stitch_p2 839/839.
- **Latent, not fixed here:** `topology::brep::p9::find_infinite_region` treats any
  error from `SmShell::RayFire` as an ordinary failed ray, so an internal lookup error
  silently changes which ray is fired. That matches upstream's "anything but
  `SM_SUCCESS`" test, but it hides an owner-naming mismatch. Any B-rep whose geometry
  owners are neither its glue env's `key_of` names nor recorded owner keys would
  diverge the same way, inside the library as well as in a harness.
