# sweep.topology_sweep — current p2 and HCR compatibility (PR #222)

Canonical main `e4dd81c4` is merged. The full main p5/p7 notes retain both sweep
API extensions. HCR global visibility now receives the owning mutable Kernel;
`GetShellVertices` invokes p2 with the actual owner and no attribute filter,
preserving the native vertex-list walk.

Eight native-feature Rust service regressions pass. They check one shell vertex
in owner 0 against two in owner 1 (including list order), successful shell/vertex
allocation and its owner/point, foreign-owner refusal, actual empty cleanup,
open-loop rejection by the live p7 constructor, copy-constructor deferral and
stitch-error context restoration. The cleanup fixture has no traversable shell
topology: its success does not certify nonempty deletion. The open line reaches
`loop does not close`, rather than the old unavailable topology prefix.

The native sweep record exports statuses/maps/decisions/tags/distances/frames,
not newly allocated vertex coordinates or full B-rep topology. Those owner/point
checks are Rust regressions. Complete construction, HCR, merge and nonempty
stitching still have their documented dependencies; merge p3/p4 remain paired
with the missing p2 merge constructor. Final native/CI evidence is recorded on
the tested PR head; no tolerance or absolute floor is relaxed.
