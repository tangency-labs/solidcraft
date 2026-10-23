# Completion evidence for port and wiring PRs

This checklist makes implementation gaps and useful follow-up tasks visible during
completion of the port. It complements AGENTS.md and the native comparison gate.
It does not add a second status system or change acceptance tolerances.

## PR evidence

Use a short table for the operations affected by the PR. Group identical paths;
there is no need to inventory unrelated functions on every change.

| Operation / upstream symbols | Implementation | Successful native comparison | Remaining gap |
| --- | --- | --- | --- |
| `<operation and function group>` | Complete body, or exact missing branches | Case IDs and observable results, or none | Dependency, missing body, runner/export gap, or none |

Include the tested commit, native summary lines and affected callers. Distinguish
these forms of evidence:

- A successful operation compared against the real native operation, including
  observable geometry, topology or state changes appropriate to that operation.
- A legitimate refusal, error or no-op compared against native behavior.
- A path blocked by an unported call or a harness limitation.

All are useful information. Only the first demonstrates the corresponding successful
operation. An operation that legitimately has no successful input needs an explanation,
not an invented success case. Status-only tests remain useful for status contracts;
use an observable result when claiming geometry or mutation coverage.

## Dependency stubs and missing continuations

An `Unported` call should occupy the place of the unavailable upstream call, with
the surrounding algorithm ported faithfully. Do not end a function at the first
missing dependency and silently omit the upstream continuation. A compiling function
with an early return is not proof that its complete body has been implemented.

Small incremental PRs remain welcome. If an owned body, branch or continuation is
still missing, name it in the evidence table and identify an independent follow-up
where possible. Do not infer implementation completeness from a green unit report
or count all of a unit's upstream functions as implemented without checking them.

Once a dependency becomes available:

1. Wire the actual service with the caller's real geometry, topology and context.
2. Exercise the previously blocked continuation and its relevant boundary branches.
3. Rerun affected callers, retaining existing case identities and comparisons.
4. Diagnose newly reached mismatches; do not hide them by broadening guards, adding
   deferrals or loosening tolerances.

## Turning harness gaps into contributor tasks

Before debugging or taking a task, search current open PRs for its symbols and paths.
The historical inventories in issues #210 and #294 are starting points; their counts
and ownership must be checked against current main.

For each available task, provide this information in an issue or related PR body:

```text
Unit / operation / symbols:
Current main SHA and reproducing command / case IDs:
Gap: missing body, missing dependency wiring, shim/export, or generator
Expected edit boundary and related active PRs:
Evidence needed to finish:
```

Useful independent tasks include constructing real B-reps instead of opaque handles,
preserving analytic geometry types in exports, replacing stale runner skips with
real calls, and generating existing entities so the intended operation is exercised.
Each task needs a checkable result, not just fewer deferred cases.

Keep legitimate empty-input and refusal tests. Add meaningful successful cases rather
than deleting negative cases to improve a coverage percentage. A broader native crash
guard must not be narrowed without evidence for the exact inputs it protects.

For runner or shim changes, show that both engines execute the intended operation
on equivalent inputs. Preserve previous output comparisons and add operation-specific
checks for the new results. Use the existing perturbation check where applicable.
A lower deferral count alone does not establish equivalence.

## Reporting scope

The table above is manual PR evidence today. Automated per-operation success-path
reporting would need a separate reviewed implementation defining operation identities,
success semantics and serialization for each runner; this document does not claim
that such a metric already exists.

The numeric gate and existing unit states remain unchanged. Any future change to
completion accounting or treatment of confirmed reference defects must be proposed
and implemented separately with auditable evidence.
