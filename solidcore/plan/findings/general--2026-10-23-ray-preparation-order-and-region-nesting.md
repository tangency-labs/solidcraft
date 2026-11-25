# 2026-10-23: Ray preparation order and region-nesting identities (#613)

Expanded primitive comparisons exposed nine flat-pyramid ray discrepancies and
missing stored UV trims on a tall translated pyramid (`gen1142`, now fixed as
`regression_pyramid_tall_translated`). Native `SmShell::RayFire` creates trims during
one solve; restarting after preparation can revisit earlier candidates with larger
tolerances. Production shell rays now use one mutable solve with exact evaluation
copies of curves/surfaces and live topology reads. Face classification consults the
prepared trim cache before the vertex/loop fallback, as native [B537] does.

The tall pyramid's final `SmStitch::DoStitching` also calls B-rep ray fire through
`FindAndSetInfiniteRegion`. Arena-default query keys omitted geometry constructed
under notification keys. Stitching now forwards those identities explicitly and
region nesting reuses the mutable ray adapter. Constructors accepting caller
notifications use them for all neighbouring faces. This also fixes the cylindrical
box's per-edge stored-trim counts (`primitive.primitive_creation_p1 / primitive_3`).
These paths retain numeric operation order and the relative 1e-9 comparison gate.

The generic read-only shell-world fallback remains a bounded preparation/restart
adapter: each requested edge use or face is attempted once, but complete traversals
can be quadratic and may revisit raised tolerances. Production `BrepQuery` shell
rays do not use that fallback. Other read-only entrypoints claimed by #619 remain
outside this change.
