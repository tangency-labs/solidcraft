# General: WR-464 addendum: the fork's own test needs a test-only surface subclass (2026-10-31)

Updates: plan/findings/general--flag-pin-wr464-missing-surface-inert-by-construction.md

The fork's capi test (758829c) makes the missing offset with `NoOffsetCylinder`, a test-only `SmCylinder` subclass whose `CreateOffsetSurface` returns no surface. The port's closed `Surface` enum cannot express that, so the test cannot be ported. Reverting the hunk on the fork does not link either: it removes `OffsetCollapses`, which the test calls.
