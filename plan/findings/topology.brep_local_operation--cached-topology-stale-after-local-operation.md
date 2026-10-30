# topology.brep_local_operation: `local_operation` returned the B-rep with stale cached topology (2026-10-30)

**Unit:** `topology.brep_local_operation` (`topology::brep::local_operation::local_operation`,
upstream `SmBrep::LocalOperation`).

**Inputs:** tangency-labs/solidcore#1061's part: a 60 x 40 x 10 plate joined with an r10 boss at
(30, 20), the boss's top face (z = 20) replaced by the plane z = 22
(`crates/solidcore/tests/issue_1061_moved_boss_top_extends_cylinder.rs`, PR #1713).

**Values:** `topology::integrity::check` on the B-rep `local_operation` returned failed with
`NotFound` for `Face(10)` under "cached face shell": a face's cached [`Face::shell`] named a shell
the operation had replaced. C++ has no such field (upstream reads the shell through the face use).

**Cause:** the port caches some topology links on its records (`Edge::vertices`, `Face::shell`,
`Face::loops`, `Loop::face`, `Loop::edges`, `Shell::faces`); edits inside an operation keep only
the use links, and every public operation refreshes the cached fields before it returns
(`Brep::refresh_cached_topology`, as the Boolean entry points do). `local_operation` removes every
face and copies the offset B-rep's faces in (`move_faces`) and returned without that refresh.

**Fix:** `local_operation` calls `b.refresh_cached_topology()` after the faces are moved (the
refresh changes no result, only the cached fields). Test: `moves_the_top_of_a_box_and_keeps_it_a_solid`
now runs the public integrity check on the result.
