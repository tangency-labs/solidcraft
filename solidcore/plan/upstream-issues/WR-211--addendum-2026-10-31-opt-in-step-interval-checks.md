Updates: WR-211

# WR-211: finite STEP-mapping refusals are opt-in (#428, PR #686)

`Kernel::fixes.ellipse_step_interval_checks` is off by default. Enabling it
rejects inverted finite STEP intervals and empty/non-finite converted NURBS
intervals before the ellipse changes. A revolution also waits for its generator
to accept the edit before installing its new analytic domain. Non-finite source
intervals remain unconditionally refused; existing UB-250 surface-clamp guards
are unchanged.

The default preserves pinned C++ finite-input behavior, including its collapsed
NURBS intervals. This is needed by torus-recognition probes introduced on main:
the earlier unconditional refusal changed seven `issue1433_*` cases. The exact
unmodified base `901089a23` passes all 758 torus cases against the same reference,
whereas the unconditional PR has seven mismatches. The new switch follows the
repository's result-changing-fix policy instead of adding exemptions for them.

The original 20 ellipse fixtures remain ordinary default-policy comparisons.
The 111 exemptions previously added for the unconditional refusal are removed;
the three affected unit statuses return to main's states. Rust regressions check
both default compatibility and enabled refusal, preserved curve/domain data,
finite retry and revolution rollback in both UV orientations. General periodic
range extension remains outside this change.

Main's #1846 separately adds `ellipse_adjust_step_interval_keeps_turns`. The checks run after that mapping when both switches are enabled; `step_interval_checks_allow_turn_preserving_mapping` verifies full- and half-circle edits, endpoint geometry and a finite subsequent reparameterization. This policy remains a refusal safeguard, not a replacement for the turn-preserving mapping.
