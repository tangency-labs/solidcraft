Updates: WR-604

#### WR-604 addendum (2026-10-30): opt-in error propagation

`Kernel::fixes.fillet_trim_status` enables first-error propagation for the primary
rails, both other-rail lists, center line, intersections and surface trim. It is
false by default, matching pinned C++ for defined failed-trim inputs. Successful
prefix edits and parameter snapping remain in either mode. PR #674 retains the
native fixtures and tests both modes; see the follow-up finding for validation.
