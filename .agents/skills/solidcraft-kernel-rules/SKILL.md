---
name: solidcraft-kernel-rules
description: "Use when changing geometry, the kernel or solidcore, or any test or golden expectation: one implementation in solidcore, no new fix flags, valid input never becomes a refusal, and test changes only with a derivation."
---

# Kernel rules

The kernel is `solidcore/`. Where code goes is in `.agents/skills/solidcraft-layers/SKILL.md`; these are the rules for changing it.

## One implementation
- Every geometric job (an intersection, a fit, a mesh, a STEP reader, a blend) has exactly one implementation, in solidcore. Two implementations of one job is a bug.
- Before you write one, search solidcore for the existing one and improve it. Don't add a parallel version beside it.
- When two versions exist, the one that stays is chosen by measurement (results, accuracy, speed), keeping the best parts of each. The other is deleted in the same series.

## No fix flags
- A fix runs always. Don't add an off-by-default switch, a "legacy" mode or a flag that keeps the old wrong path alive.
- When a fix changes results, show which results move and why the new ones are right.

## Valid input never becomes a refusal
- If the old result was a valid solid, the change must still produce a valid result. A new refusal there is a regression.
- A clear, typed refusal that names the reason is right when the old result was invalid (open, non-manifold or self-intersecting), deleted data without saying so, or built nothing.
- Refuse with the reason, never with a bare status code, and never return a wrong body as success.

## Tests
- Never take an expected value from your own run, never loosen a bound to fit what the new code produces, never mark a test `#[ignore]`, and never move a known-failing test's inputs until it passes.
- Changing or deleting a line of an existing test or golden needs a `Test change: <reason>` line in the PR body. The reason derives why the old value was wrong: a certified bound, a rounding analysis, an enclosure, a closed-form answer. "Matches the new output" is not a reason.
- A bound made "only more conservative" still needs that derivation, and the goldens unchanged on top of the latest `main`.
- You may always add a new case beside the old one.
- A test fixed by your change: drop its line from `tests/KNOWN_FAILING_TESTS` in the same PR. Kernel cases go in `solidcore/crates/solidcore/tests/kernel_cases/` (`.agents/skills/solidcraft-tests/SKILL.md`).

## The PR
- A kernel result change is not a changed default: write `Changes a default: no`.
- Give expected and measured values, and the goldens or rig results before and after.
- Follow `solidcore/AGENTS.md` and its reference docs for the area.
