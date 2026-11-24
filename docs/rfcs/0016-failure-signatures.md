# RFC 0016: failure signatures and reproducible reports

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-16 |
| Repositories | SolidCraft (`crates/`), solidcore (vendored at `solidcore/`), tangency-labs/tickets |
| Kernel side | `solidcore/docs/failure-codes-and-reproduction.md` |
| Ticket side | `docs/failure-signatures.md` in tangency-labs/tickets |
| Discussion | the pull request that adds this file, then the tracking issues |

Reports of a failed operation reach tickets.solidcraft.io as text. The tracker groups them by stripping numbers, names and paths out of the message with regular expressions (`signature.ts`), and a developer reproduces one by opening the reporter's design, if it was shared. This RFC makes every report of a kernel failure carry **a stable code** (so identical failures group by comparison, with no reading and no language model) and **the failed kernel call as a replayable case** (so it reproduces on the kernel alone), and adds a reducer that shrinks the case to the geometry that still fails.

## Goals

- Two reports of the same failure get the same signature; two different failures do not, whatever the design, the numbers or the names.
- A report replays on the kernel alone, at the commit it ran on, with one command.
- A failing case shrinks automatically to its smallest still-failing geometry, with the conditions at the failure measured.
- Nothing about the user's design leaves their computer unless they agree.

## 1. The signature

`<command> · <ops call> · <kind> · <operation>`, for example `solid.fillet · fillet · 108 · blend::fillet_executive::…`:

| Part | From | Stable? |
|---|---|---|
| command | SolidCraft's command id | yes |
| ops call | the session's call name (`solidcore-ops`) | yes |
| kind | `ErrorKind::code()`, numbered, never changes meaning once released | yes: the bucket |
| operation | `SolidError::operation()`, the kernel routine | follows module paths: the second level |

Every part is a static string or an integer. solidcore's design (`failure-codes-and-reproduction.md`, 3.1) adds `FailureCode { call, kind, operation }` to ops errors and the failure hook; today `status()` drops it.

## 2. What changes in SolidCraft

1. **Keep the code.** A feature's result keeps the `FailureCode` next to its error text (`error_code`, in the model's result and in `diagnostics`). Today the result holds a `String` and the type is gone before a report is built.
2. **Trace the failing feature on report.** When the user reports a failed feature, the engine rebuilds that one feature with `smlib::trace` on (`trace::start()`, `trace::finish()`), the tracer that `cargo xtask kernel-cases --export` already uses, and writes the events as a rig entry: `calls`, `<case>.bodies.json.gz`, the fix set, and the solidcore and SolidCraft commits.
3. **Carry the code in the trace.** `trace::Event::Call`'s error keeps the `FailureCode` beside the message, so the entry names its failing call's code.
4. **One trace per run.** The trace log is a process global (`static LOG`). With kernel calls running concurrently (#17489), traces from two threads interleave: the log becomes per run (the rebuild that started it), not per process.
5. **Send it.** The report's `repro.failing` gains `code` (the four parts) and, when the user agrees, `kernel_case` (the entry and its bodies).
6. **Ask first.** The report form asks before attaching the failing call's bodies, as it asks for the design, and says what they are: the geometry the failing operation took, not the whole design.

## 3. What changes in solidcore

In `solidcore/docs/failure-codes-and-reproduction.md`: `FailureCode` on ops errors and the hook, the call's arguments with its bodies, sites on the generic failures, single-entry replay (`KERNEL_CASES_ENTRY`), the reducer (`cargo xtask kernel-cases --reduce`) and its conditions report.

## 4. What changes in tickets

In tickets' `docs/failure-signatures.md`: `signatureOf` uses `failing.code` when present and falls back to the regex class for older clients and crashes; kernel cases are stored as private attachments; a worker replays and reduces each new case at the ticket's solidcore commit and stores the minimal case and its conditions on the ticket and its group.

## 5. Order

| Step | Repository | Change | Depends on |
|---|---|---|---|
| 1 | solidcore | `FailureCode` on ops errors and the hook | none |
| 2 | SolidCraft | keep the code in feature results and diagnostics | 1 |
| 3 | tickets | signature from `failing.code` | 2 (falls back without it) |
| 4 | SolidCraft | trace the reported feature, per-run trace log, attach the entry with consent | 1 |
| 5 | solidcore | single-entry replay | 1 |
| 6 | tickets | store kernel cases; replay worker | 4, 5 |
| 7 | solidcore | the reducer and its conditions report | 5 |
| 8 | tickets | reduce on arrival, show the minimal case and conditions per group | 6, 7 |

Steps 1 to 3 give exact grouping on their own; 4 to 8 add reproduction and reduction.

## 6. Limits

- **State.** The kernel keeps state between calls (its random generator, caches; #13672). A case replayed from records can fail differently or not at all. That is detected (the replay's code differs) and marked on the ticket; it is not reproduced until a session starts from a known state.
- **Partial sites.** Until the generic failures name their sites, the reducer starts from the call's picks, which is slower and less local.
- **Old clients and crashes.** Reports without a code keep today's regex signature; a panic has no kernel error and keeps the crash report's own replay (`solidcraft-cli crashes replay`).

## 7. Alternatives considered

- **Better regexes, or a language model, over the message.** Messages carry numbers, names and wording that change with every fix; the code is already in the kernel, so reading it back out of text is the wrong direction.
- **Hash the message after normalising.** Same problem: the hash moves when a message is reworded, and two different failures with one generic message ("SMLib loft: invalid argument") share it.
- **Attach the whole design instead of a kernel case.** It needs the application to reproduce, is larger and more private, and cannot be reduced on the kernel alone.
