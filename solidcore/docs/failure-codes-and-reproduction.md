<!-- SPDX-License-Identifier: Apache-2.0 -->

# Failure codes and reproduction

Status: steps 1 (3.1, the failure code) and 3 (3.4, single-entry replay) are built; the rest is design. The application side is RFC 0016 in SolidCraft's `docs/rfcs/`; the ticket side is `docs/failure-signatures.md` in tangency-labs/tickets.

When a kernel operation fails for a user, three things must reach whoever fixes it: **which failure it is** (so reports of the same failure are grouped without anyone reading them), **the call that failed** (so it replays on the kernel alone), and **the smallest geometry that still fails** (so the cause can be seen and a clean case written). This page is solidcore's part: a stable failure code on every ops error, the call's arguments with its bodies, a single-entry replay, and a reducer.

## 1. What exists today

| Piece | Where | What it gives |
|---|---|---|
| `ErrorKind` | `crates/solidcore/src/error.rs` | A stable numeric code per kind of failure: 1 to 9 general, 90 and up solidcore's own, 100 to 132 the blend classes, 200 and up offset. Codes never change meaning once released. |
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

- `session::take_failure_code() -> Option<FailureCode>` reads it after a failed call, on the thread that made it, beside `session::take_failure_detail()` (the kernel's words), and taken like it. The session sets it in `run_with`, which holds the call name: a call that fails with a kernel error has that error's kind and operation; a refusal the call's answer carries has those of the kernel error it is, as ops typed it (each place ops types a kernel error, `status()` and the blend, round and offset refusals, notes it for the call; the refusal must be of the variant the note became, so a point or an index ops adds afterwards keeps it, and an attempt the call tries and gives up puts the note back); a failure ops decided itself (an argument, a result its own checks refuse, a caught panic) is kind `FailureCode::OPS` (0) with the `Error`'s variant name (`Error::name()`) as its operation. A cancellation is no failure and has none, nor has an argument an operation refuses before it starts a call. It is beside `Error`, not inside it: `Error` is a plain value applications compare and match by its variants, and every variant would have to carry it.
- `Failure` (the hook) gets `code: FailureCode`, the same code.
- Its `Display` is `call · kind · operation`.
- Every field is a static string or an integer: no geometry, no names, no numbers from the model. Two reports of one failure have equal codes, compared as strings.
- `kind` is the stable part and the bucket. `operation` splits a coarse kind (6, `Failed`, "does not describe how yet") and follows the module path, so a refactor can move it: it is the second level of a signature, never its key.

### 3.2 Arguments with the bodies

`Failure` gets `params: serde_json::Value`: the call's arguments in the rig's `params` form (picks as topology indices, as `smlib::trace` writes them). With `call`, `params` and the input records, one failed call is one rig entry.

### 3.3 Sites on the generic failures

Each `Failed` error built where the routine knows its entity (an edge being blended, a face being trimmed) names it through `Site`. This is incremental: one family per PR, starting with the families that fail most in the rig's `known_failing.json`. A failure without a site still groups by its code; the reducer then starts from the call's picks.

### 3.4 Replaying one entry

The rig runner takes `KERNEL_CASES_ENTRY=<path>`: one entry file, its `<case>.bodies.json.gz` beside it, outside the corpus and outside the manifest. The run reports the outcome and the failing call's `FailureCode`, and checks that code against the entry's (`same failure`, `other failure`, `builds now`).

Built: `KERNEL_CASES_ENTRY=<entry.json> cargo test -p solidcore-ops --test kernel_cases -- kernel_case_entry` prints one JSON line, `{"case", "outcome", "code", "expected", "note"}`. The expected code is the `code` of the entry's last failing call that is not `continued`. The outcome is `same failure`, `other failure`, `builds now` or `unsupported`. The test fails unless it is `same failure`, or `KERNEL_CASES_REPORT_ONLY` is set; `KERNEL_CASES_REPORT` writes the line to a file too.

### 3.5 The reducer

`cargo xtask kernel-cases --reduce <entry>` writes `<case>-min.json` and its bodies. Each step replays and keeps a change only when the replay still fails with the **same code**; the code is what makes "still the same failure" a comparison, not a judgement.

1. **One call.** Replace every call before the failing one by the failing call's operand bodies as records. If that no longer fails, stop: the failure depends on the kernel's state across calls (its random generator, caches; #13672). Report that on the case.
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

## 6. Converting a failure site

This is the guide for the typed-errors work. Follow it when you convert the sites that build a failure in a file of `crates/solidcore` or `crates/solidcore-ops`.

### The goal and the counts

Every failure ends up as one of three things:
- **a named cause**: a variant of the area's failure enum that says what happened, with its numbers;
- **a refusal**: a fact of the input, so the way out is a different input. Where a check measures a size, the refusal carries the size that would pass;
- **a defect**: the area's `Defect { why }`, a failure that is the kernel's fault.

Three kinds of site are not there yet. Tests count them per file, and each count only goes down:
- **phrase causes**: variants that carry only a sentence, such as `SurfaceFailure::Failed("...")`, and the helpers that build them (`SolidError::surface_failed(`). The blend's counts are pin files in `src/error/pins/phrase/` (`src/error/phrase_sites.rs`). Each other area's list is in its own error file's tests (`src/error/<area>.rs`). The generic `Reason::Failed` list is in `src/error.rs`'s tests.
- **bare statuses**: `SolidError::sm_err(`, `null_pointer(`, `Upstream {` and `nl_ser(`. The lists are in each area's error file (`assert_waiting`).
- **unreviewed stage failures**: an area's `StageFailed`, and the helpers that build one (`SolidError::curve_stage_failed(`). The counts are pin files in `src/error/pins/stage/` (`src/error/stage_sites.rs`).

A call of a local wrapper counts once for each call, and so does each use of its name as a value (`ok_or_else(fail)`). `src/error/site_scan.rs` says exactly what counts. A PR that converts sites lowers its files' counts in the same PR. A file whose count reaches 0 loses its line. The test failure prints the list as found.

The phrase-cause and stage-failure counts are one file per library file: `src/error/pins/<kind>/<path>.pin`, where `<path>` is the file's path from `crates/` (`pins/phrase/solidcore/src/blend/hcr.rs.pin` holds the count for `solidcore/src/blend/hcr.rs`). Write the new count into the pin, delete the pin when the count reaches 0, and create one if code moves into a new file. The test failure names each pin to change. Two PRs that convert sites in different files touch different pins, so they never conflict.

### 1. Keep the status

This rule comes first. A conversion keeps the numeric status the site reports today. The goldens compare the status exactly, so a changed status fails them.

- Stay on the typed record: `Failure::new(OP, cause)`, or the area's constructor (`SolidError::curve(OP, cause)`).
- Each cause has a default status. When the site's status is not the default, keep it with `.with_status(status)`.
- Do not convert a site to `SolidError::InvalidInput` or `SolidError::PortInvariant`. They report no status.
- A change of status is a separate PR. That PR re-blesses the goldens and says why.

### 2. A missing thing

A `Missing` site is a defect unless the missing thing came from the caller.
- The kernel built the thing, and it is not there: `Defect { why }`, keeping the status (`SM_ERR_NULL_POINTER` through `with_status`).
- The caller supplied the thing, and it is not there: a named cause in the area's enum whose class is `InvalidInput`. `CurveFailure::PointSolveOptionMissing` is the model.

### 3. A solve that does not converge

- A solve that stops with its numbers: a named cause that carries them (the residual, the tolerance, the iterations). It is a defect. `BlendFailure::StationDidNotConverge` is the model.
- A refusal only where a separate check measures the fit. Then the refusal carries the fit.
- A drop or an intersection that stops with no numbers: `Defect { why }` only when the site establishes that the stop is the kernel's fault (the inputs were proved fit, and the step still stopped).
- Where the area's contract already names a refusal for the situation, the site keeps that refusal, or builds it. For code that calls the `ssi` crate, the surface-surface intersection's contract is one: a contact it cannot prove is refused by name, as `Tangent` (`docs/ssi.md` §1 and its refusals, `crates/ssi/src/lib.rs`). Such a contact is never a `Defect`.

### 4. What a `why` is

A `why` names the step that failed. It also says why that is the kernel's fault and not the geometry's. Copying the old phrase is not a `why`.

- Bad: `"the drop failed"`. It names no step and gives no reason.
- Good: `"the drop of the rail's end onto its own support found no foot, though the end was built on that support"`.

If you cannot write the second half, the site may be a refusal. Find the check that would show it.

### 5. Wrappers and shared helpers

A local wrapper or a shared helper is split per site when its file is converted. Each call gets its own outcome. A helper with a misleading name, such as a local `sm_err` that builds something else, is renamed in the same PR.

Before you convert the sites beneath a wrapper that replaces their errors with a phrase, fix the wrapper so it passes the cause through. Otherwise the class you choose at a site never reaches the caller.

### 6. A caught error passes its cause on

A site that catches another routine's error chains that error as its cause, and takes its class from it. It never replaces a typed cause with `Defect` or `StageFailed`.

```rust
let curves = intersect(a, b).map_err(|e| Failure::new(OP, cause).caused_by(e).into_error())?;
```

A catch-all arm must not merge "the routine returned an error" with "the routine found nothing": the first passes the error on as above, the second is rule 7.

### 7. An empty search is not a missing pointer

A search that can find nothing on valid geometry is a refusal or a named cause. Examples: no crossing, no intersection curve near the points, a rail that does not meet an edge. This holds even where the ported code treated the empty result as a null pointer.

### 8. A kind the routine cannot handle

A surface or curve kind the routine cannot handle, such as one with no B-spline form, is a named "unsupported kind" cause. The cause carries the entity. It is not a defect.

### 9. The caller's own input

A check on the caller's own argument is an invalid-input cause in the area's enum. So is a check on the caller's body as supplied, such as an edge with no curve or a face with no surface. Rule 1 still holds: the cause stays on the typed record.

### 10. Control flow is not a failure

Some errors are caught by every caller and turned into another path. Such an error is control flow. Return a value instead, such as an `Option` or an enum, and write no reason for it.

```rust
fn foot(..) -> Option<Point2>   // not: fn foot(..) -> Result<Point2>, with every caller matching Err
```

### 11. Unreachable sites are deleted

A site that cannot be reached is deleted, not converted. Examples: it repeats a check that already passed, or it follows an unconditional early return. The PR says why the site is unreachable.

### 12. One condition, one class

The same condition gets the same class everywhere in a file.

### 13. What the PR shows

- For each site, or each group of identical sites: the outcome chosen and the evidence. "No site in this file decides a refusal" alone is not evidence.
- The tests that assert an old phrase are updated in the same PR.

The reviewer checks four things:
- the status is preserved at every site;
- each count is lowered by exactly the sites converted;
- each `why` meets rule 4;
- no new phrase cause, bare status or `StageFailed` site.

### 14. What does not change

- `PortInvariant` is not a conversion target.
- Class 127 (`ErrorKind::StageFailed`) and its name `stage_failed` do not change in this work. A `Defect { why }` has that class, so a converted defect reads exactly as the stage failure it replaces, except for its message.

## If you're stuck

The code's fields come from `SolidError::kind`, `SolidError::operation` and `SolidError::site`: read their docs in `crates/solidcore/src/error.rs`. The entry format is the rig's README (`crates/solidcore/tests/kernel_cases/README.md`). If a failure replays differently in a fresh process, it is state-dependent (3.5, step 1): say so on the ticket and do not reduce it.
