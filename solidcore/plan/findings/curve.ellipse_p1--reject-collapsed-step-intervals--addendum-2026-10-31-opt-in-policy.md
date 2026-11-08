# curve.ellipse_p1: STEP-interval checks follow the opt-in policy (2026-10-31)

Updates: plan/findings/curve.ellipse_p1--reject-collapsed-step-intervals.md

Units: `curve.ellipse_p1`, `surface.surf_of_revolution_p1/p2/p3`,
`surface.surf_of_extrusion_p1`, `surface.torus`; PR #686 / #428 / WR-211.

The previous finding and its 111 comparison differences describe the earlier
unconditional refusal. Current behavior uses the default-off
`Kernel::fixes.ellipse_step_interval_checks` switch. Non-finite source intervals
are always refused. With the switch enabled, inverted or collapsed converted
intervals are refused before changing the ellipse, and the revolution's domain
is installed only after its generator accepts the edit.

Fresh validation on `daee09af84` found seven additional `surface.torus`
differences in main's `issue1433_*` recognition fixtures (four translated full
tori, the thin tube and the two thin-sheet cases). For example, origin
`(90,-40,500)`, axes `(0,1,0)` / `(-1,0,0)`, radii 10 and 3, scale 1, has
`values.out[78] = 0` in C++ versus `.25` with the unconditional refusal.
The exact unmodified main base `901089a23`, built with the same official Rust
1.98.0 toolchain, prescribed difftest profile, Apple Clang reference pinned at
`3fd5b95` and 16 seeds, passes all 758 torus cases. Its maximum relative error is
`1.8260316904540807e-11`, within the unchanged `1e-9` tolerance. The unconditional
PR fails seven of the same inputs. The torus implementation, runner and fixture
bytes are identical; this is a behavioral interaction of the unconditional
ellipse refusal, not an unrelated baseline failure.

No new mismatch entry, guard or tolerance is added for these cases. The default
now follows C++; the enabled behavior retains the safe-refusal regressions and
supported retry checks. All 111 prior exemptions are removed from their three
per-unit files, and main's unit metadata is retained. Final validation is recorded
in the PR description once the revised head has run.
