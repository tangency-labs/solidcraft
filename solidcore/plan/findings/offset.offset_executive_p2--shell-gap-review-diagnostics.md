# Shell failure diagnostics retain typed causes and input sites

Addendum to `offset.offset_executive_p2--shell-gap-failures-keep-their-causes.md`.

The S0790 regression now requires both `stage_failed` and the cause `the joined offset
edge could not be split at the gap bridge joint`. The earlier assertions also accepted
an error from a missing face surface, before the gap closer ran. The construction and
its dimensions remain the same; no geometry predicate or tolerance changes.

A second transport loss affected typed refusals. `OffsetFailure::ExactCurveUncut` can
carry the clipping error that stopped it. Converting it to `OffsetRefusal::ExactCurveUncut`
kept the type but discarded that nested error. Ops now supplies the typed refusal and
complete diagnostic together. The application retains both, while phrase refusals keep
the complete diagnostic in their phrase and `WallFolds { largest }` keeps its specialised
answer. Kernel-constructor tests live in ops' existing offset tests; application tests
exercise the mapping with ops data, and S0790 exercises the complete application path.

The gap closer's `Failed` cause and outer `StageFailed` now carry a `Site`. Its input
edge and face come only from the executive's provenance: a unique original edge-use
source (including a mapped mate), and a unique original face-use source. Tests use
intentionally different input and intermediate IDs, and ambiguous provenance names
no input entity. Known gap joints retain their exact points. At step corners where
only intermediate topology is available, the site keeps the point; an intermediate
edge ID must not be presented as an input edge. These diagnostic lookups do not change
the operation's outcome.

The original nine added non-public solidcore paths were kernel error constructors in
the application tests: four in the phrase/class test and five in the substage test.
Moving kernel error construction and diagnostic extraction behind ops removes those
paths instead of raising the application's shrink-only allowance. No golden values or
kernel-case expectations change.
