<!-- SPDX-License-Identifier: Apache-2.0 -->

# Failure codes and reproduction

Status: design, not built yet. The application side is RFC 0016 in SolidCraft's `docs/rfcs/`; the ticket side is `docs/failure-signatures.md` in tangency-labs/tickets.

When a kernel operation fails for a user, three things must reach whoever fixes it: **which failure it is** (so reports of the same failure are grouped without anyone reading them), **the call that failed** (so it replays on the kernel alone), and **the smallest geometry that still fails** (so the cause can be seen and a clean case written). This page is solidcore's part: a stable failure code on every ops error, the call's arguments with its bodies, a single-entry replay, and a reducer.

## 1. What exists today

| Piece | Where | What it gives |
|---|---|---|
| `ErrorKind` | `crates/solidcore/src/error.rs` | A stable numeric code per kind of failure: 1 to 9 general, 90 and up solidcore's own, 100 to 131 the blend classes, 200 and up offset. Codes never change meaning once released. |
| `SolidError::operation()` | same | The `&'static str` path of the routine that failed (`"blend::fillet_executive::…"`). |
| `SolidError::site()` | same, `Site` | The edge, face or vertex of the caller's body the failure is about, and the point, where the error names one. |
| ops `Error` | `crates/solidcore-ops/src/error.rs` | What applications see. `status()` maps every `SolidError` to it. |
| failure hook | `crates/solidcore-ops/src/session.rs`, `Failure` | The failed call's name, its error as text, and its input bodies as `.smb`. |
| kernel rig | `crates/solidcore/tests/kernel_cases/`, runner `crates/solidcore-ops/tests/kernel_cases.rs` | Operation-level cases as data: the calls (`op`, `inputs`, `params`, `outputs` or `error`), the operand records, the fix set; replayed on the kernel alone. `KERNEL_CASES_CASE` selects cases by name. |

## 2. What is missing

1. **The code does not survive `status()`.** Only `InvalidInput` keeps its operation (`Error::Refused`), and a few typed refusals keep their numbers. Everything else becomes `Error::Status(1001)`: the `ErrorKind` code and the operation are dropped, so an application can only show, and a tracker can only group by, the message text.
2. **The failure hook has no code and no arguments.** It has the bodies but not the radius, the picked edges or the sections, so the call cannot be replayed from it.
3. **The rig replays its corpus, not a file.** A case from a report has to be copied into an area and counted in `manifest.json` before the runner sees it.
4. **Many errors name no site.** The generic `Failed` kinds, the ones grouping needs most, carry no `Site`.
5. **Nothing shrinks a failing case.** There is no reducer and no report of the geometry at the failure.

## 3. Design

### 3.1 The failure code

ops gains one plain value, taken from the `SolidError` before `status()` maps it:

```rust
/// What failed, as stable data: the same failure always gives the same code.
pub struct FailureCode {
    /// The ops call that failed (the session's call name: "fillet", "boolean", ...).
    pub call: &'static str,
    /// `ErrorKind::code()` of the kernel's error; 0 for a refusal made in ops itself.
    pub kind: u16,
    /// `SolidError::operation()`: the kernel routine that failed.
    pub operation: &'static str,
}
```

- `Error::code(&self) -> Option<FailureCode>` reads it. The session sets it where it already has both (`run_with` holds the call name; `status()` holds the kernel error). The variants that lose the most (`Status`) carry it; the variants that already are their own code (`Argument`, `Cancelled`, `Reentered`) need none.
- `Failure` (the hook) gets `code: Option<FailureCode>`.
- Every field is a static string or an integer: no geometry, no names, no numbers from the model. Two reports of one failure have equal codes, compared as strings.
- `kind` is the stable part and the bucket. `operation` splits a coarse kind (6, `Failed`, "does not describe how yet") and follows the module path, so a refactor can move it: it is the second level of a signature, never its key.

### 3.2 Arguments with the bodies

`Failure` gets `params: serde_json::Value`: the call's arguments in the rig's `params` form (picks as topology indices, as `smlib::trace` writes them). With `call`, `params` and the input records, one failed call is one rig entry.

### 3.3 Sites on the generic failures

Each `Failed` error built where the routine knows its entity (an edge being blended, a face being trimmed) names it through `Site`. This is incremental: one family per PR, starting with the families that fail most in the rig's `known_failing.json`. A failure without a site still groups by its code; the reducer then starts from the call's picks.

### 3.4 Replaying one entry

The rig runner takes `KERNEL_CASES_ENTRY=<path>`: one entry file, its `<case>.bodies.json.gz` beside it, outside the corpus and outside the manifest. The run reports the outcome and the failing call's `FailureCode`, and checks that code against the entry's (`same failure`, `other failure`, `builds now`).

### 3.5 The reducer

`cargo xtask kernel-cases --reduce <entry>` writes `<case>-min.json` and its bodies. Each step replays and keeps a change only when the replay still fails with the **same code**; the code is what makes "still the same failure" a comparison, not a judgement.

1. **One call.** Replace every call before the failing one by the failing call's operand bodies as records. If that no longer fails, stop: the failure depends on the kernel's state across calls (its random generator, caches, the fillet step latch; #13672). Report that on the case.
2. **The neighbourhood.** Keep the faces at the error's `Site` (or the call's picks) and one ring of neighbours, remove the rest (`trim::remove_faces`), renumber the picks in `params`.
3. **Least faces.** Try removing each remaining face, each picked edge, each extra section; keep every removal that still fails with the same code. A removal that changes the failure (a solid become a sheet and refused for that) fails the check and is undone.
4. **Conditions.** On the minimal case, measure at the site with the kernel's own measures: the dihedral angle along the edge, the least radius of curvature of each face near it, the edge's length, the edge and vertex tolerances, gaps; and print them beside the call's parameters ("fillet radius 5.0; face 2 curves at radius 4.8 within 1 mm of the site; dihedral 178.3°").

The minimal case is input for a person or an agent, not a corpus entry: a rig case's expected values are closed-form, never a kernel's output (README, "Adding a case"). The conditions say what the clean, hand-built case must reproduce.

## 4. Order of work

| Step | Change | Depends on |
|---|---|---|
| 1 | `FailureCode`, `Error::code`, `Failure::code` (3.1) | none |
| 2 | `Failure::params` (3.2) | 1 |
| 3 | `KERNEL_CASES_ENTRY` single-entry replay with the code check (3.4) | 1 |
| 4 | the reducer, steps 1 to 3 (3.5) | 3 |
| 5 | the conditions report (3.5, step 4) | 4 |
| 6 | sites on the generic failures, family by family (3.3) | none; improves 4 |

## 5. Privacy

A case carries the user's geometry. The application asks before attaching it (RFC 0016). The reducer only removes geometry; its output is smaller than the report, still the user's, and stays with the ticket unless the reporter agreed to share it.

## If you're stuck

The code's fields come from `SolidError::kind`, `SolidError::operation` and `SolidError::site`: read their docs in `crates/solidcore/src/error.rs`. The entry format is the rig's README (`crates/solidcore/tests/kernel_cases/README.md`). If a failure replays differently in a fresh process, it is state-dependent (3.5, step 1): say so on the ticket and do not reduce it.
