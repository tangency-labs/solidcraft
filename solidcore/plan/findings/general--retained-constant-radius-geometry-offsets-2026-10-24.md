# Retained constant-radius geometry offsets (2026-10-24)

The real box session reached `SmFilletGeom::GetOffsetSurface` while native calculated
both first-corner vertices. The context-free solver getter could not resolve the
selected fillet geometry. A borrowed constant-radius rail context now reads and
mutates that actual geometry's offsets, including nested initial-value extension
state and Jacobian offset resets. The concrete solver and its separate offsets
remain retained; selected geometry is restored after success or failure. No surface
swap, selected-FG clearing, cloned solver table or invented offset identity is used.
The six real-box checkpoints now give five exact value passes, zero failures and
one later full-API deferral (`blend.fillet_executive_p1`); maximum relative error is
zero. This proves direct first-corner vertex geometry, not a full fillet result.

The ordinary geometry is `SM_FG_DEFAULT`. The contextual tangent-rollover guess in
`SmConstantRadiusFS::LoadInitialValues` (`SmFilletStandardSolver.cpp:493–537`) still
needs actual pseudo-B-rep vertex-use context; that genuine branch remains explicitly
Unported in the borrowed provider. Other concrete solver kinds keep their existing
missing-context behavior. Existing context-free APIs and fixture coverage remain.
