# The rigor dial

Status: draft for review. The kernel type, the design record and booleans are in. Accuracy and
effort are recorded but no family reads them yet, and the slider is only sketched here.

## Goal

Every modelling operation gets one dial, from "fast, a preview" to "slow, proved". It goes in
the tool's options panel, and its value is saved with the operation. A design has a default
that its operations inherit, and any operation can override it. The dial sets four knobs, and
each knob can still be set on its own:

| Knob | Low end | High end |
|---|---|---|
| **Certification** | off: nothing tested | Closed, then Proved: valid and facing out |
| **Accuracy** | loose tolerance, coarse chord, few samples | tight tolerance, fine chord, many samples |
| **Effort** | iteration and subdivision limits scaled down | limits scaled up |
| **Time** | a short wall-clock budget | a long one |

Running out of time is a typed refusal, "ran out of time". It never gives a different answer.

Designs saved before this change build exactly as they did. A reopened old design never changes
its geometry.

## The kernel type: `solidcore_ops::Rigor`

One app-agnostic type, documented in `solidcore-ops/src/rigor.rs`. The application stores it
and passes it back. It never computes one.

```rust
pub struct Rigor {
    pub certify: Certify,          // Off | Closed | Proved
    pub accuracy: Accuracy,        // tolerance, chord, samples: Option each (None = the call's own)
    pub effort: Effort,            // percent: Option<u32> of the call's own limits
    pub time: Option<Duration>,    // wall-clock budget; None = none
}
impl Rigor {
    pub const LEGACY: Rigor;       // Closed, every knob None, no budget: the calls as they were
    pub fn dial(d: f64) -> Result<Rigor>;   // 0..=1, Error::Argument outside or NaN
    pub fn stop(d: f64) -> Result<Stop>;    // the same, with the stop's name
    pub fn check(&self) -> Result<()>;      // refuses a knob no call can run with
}
pub fn within<R>(rigor: &Rigor, f: impl FnOnce() -> R) -> R;  // scope, like cancel::within
pub fn current() -> Option<Rigor>;
pub fn checked<T>(r: Result<T>) -> Result<T>;  // Cancelled from a spent budget -> OutOfTime
```

### The 0..1 mapping is a table

`Rigor::dial` picks the last of five stops that the value reaches. It uses no formula. A table
gives the same knobs to the bit on every platform, and ops does no float arithmetic to get them
(the crate denies `clippy::float_arithmetic`). A slider snaps to the five stops.

| Dial from | Name | Certify | Tolerance | Chord | Samples/span | Effort | Time |
|---|---|---|---|---|---|---|---|
| 0.00 | Preview | Off | 1e-4 | 0.05 | 8 | 25 % | 2 s |
| 0.25 | Fast | Closed | 5e-5 | 0.02 | 12 | 50 % | 10 s |
| 0.50 | Standard | Closed | 1e-5 | 0.01 | 16 | 100 % | 30 s |
| 0.75 | Careful | Proved | 5e-6 | 0.005 | 24 | 200 % | 120 s |
| 1.00 | Proof | Proved | 1e-6 | 0.001 | 32 | 400 % | 600 s |

Lengths are in model units (mm in SolidCraft). A test (`the_stops_rise_in_every_knob`) checks
that every knob rises, or stays the same, from one stop to the next.

**Legacy is not a stop.** `Rigor::LEGACY` means "each call's own values". Today those values
live in many places, and no single stop equals them. A design that records no rigor builds
with `LEGACY`. A call given `LEGACY`, or given no scope at all, runs exactly as before.
`within(&LEGACY, f)` opens no scope.

### How a family reads it

The application runs an operation's calls inside `rigor::within(&rigor, ..)`, the same way it
runs them inside a cancel token. The time budget covers every call in the scope. Each family
reads the knobs it supports and ignores the others.

| Knob | Read by, in this PR | Next |
|---|---|---|
| Certification | booleans (`boolean`, `boolean_under`, `boolean_certified`) | fillet/chamfer (`Blend::contract`), sweeps, offsets: they already take a `Contract` |
| Time | every call in the scope; the session returns `OutOfTime` for any call the budget stopped | SolidCraft's wrapper needs its own `OutOfTime` arm (see open question 1) |
| Accuracy | none (recorded only) | fits: sweep/loft sampling and chord, blend certificate tolerance |
| Effort | none (recorded only) | certificate halvings (`nurbs::certificate::MAX_HALVINGS`), offset reach halvings, solver iterations |

`Certify::Off` on a boolean skips the operand tests and accepts any result that has faces. A
family with no Off mode of its own runs it as `Contract::Closed` (`Certify::contract`).

### The time budget

`within` starts a deadline: a `CancelToken` that a watch thread cancels when the budget runs
out, joined with any token the application already has (`cancel::within`). Kernel loops already
poll the token. The session (`session::run_with`) handles the stop the same way as any other
cancel: it rolls back the call's edits, drops a kernel the call stopped part way, runs no failure
callback and leaves no failure detail. The only difference is the error value. At both places a
call can stop (refused before it runs, and stopped part way), the session passes `Cancelled`
through `rigor::checked`, which gives `Error::OutOfTime { budget }` when this scope's deadline
fired and the application did not cancel the work itself. Outside every rigor scope the value is
exactly `Cancelled`, and a stop the user starts stays `Cancelled`. Two session tests check both
places: `outside_a_rigor_a_stop_is_cancelled_at_both_sites` and
`within_a_rigor_a_spent_budget_is_out_of_time_at_both_sites`. The session's module doc states the
rule under **Stops**.

- A zero budget is spent before the first call, which gives a deterministic test.
- Where no thread can start (wasm without threads), the budget is not enforced. The call runs to
  the end, which gives the answer it would have given with no budget.

## The design record

The design-format change is one pending file, `crates/doc/src/format/pending/operation-rigor.rs`.
It needs no upgrade step. A design gains an optional `rigor` key:

```json
"rigor": {
  "default": {
    "set":   { "dial": 0.6 },
    "built": { "certify": "closed", "tolerance": 1e-5, "chord": 0.01, "samples": 16,
               "effort_percent": 100, "time_ms": 30000 }
  },
  "ops": {
    "14": {
      "set":   { "dial": 0.3, "certify": "proved", "time_ms": 1500 },
      "built": { "certify": "proved", "tolerance": 5e-5, "chord": 0.02, "samples": 12,
                 "effort_percent": 50, "time_ms": 1500 }
    }
  }
}
```

- `set` is what the user asked for: the dial, plus any knob set on its own over the dial.
- `built` is what `set` resolved to at the moment it was set. **The operation is built from
  `built` alone.**
- `ops` is keyed by feature id. **An operation with no entry inherits the default**, and its
  built value is `default.built`. An entry is an override.
- With no `default`, the default is legacy. With no `rigor` key at all (every design saved
  before this change), every operation inherits legacy.

The records live in a map on the design rather than on `Feature`. `Feature` is built as a
struct literal in about 40 places, and the map keeps this change in new files (see open
question 4).

### Inherit and override

| Action | Effect |
|---|---|
| `Document::set_rigor_default(Some(set))` | Resolves `set` and stores it as the default. Returns the features whose rigor changed: the inheriting ones, and only when `built` changed. |
| `Document::set_rigor_default(None)` | Back to legacy. |
| `Document::set_rigor_override(id, Some(set))` | Feature `id` gets its own record. Later default changes leave it alone. |
| `Document::set_rigor_override(id, None)` | Feature `id` inherits again. |

A refused setting (dial outside 0..1, NaN, a zero or negative tolerance or chord, zero samples,
0 % effort) changes nothing.

### Rebuilding when the default changes

A feature's fingerprint includes its `built` rigor, and only when that rigor is not legacy. When
the default changes, the inheriting features' fingerprints change and they rebuild. Overriding
features keep the same fingerprint and reuse their results. As with any edit, features after
the first changed one rebuild too, because the fingerprint chain covers everything before them.
In `a_default_change_rebuilds_only_what_inherits_it`, an overriding first feature reuses its
result (the same `Arc`), and the inheriting second and third features rebuild.

At legacy the fingerprint takes nothing. So a design without `rigor` fingerprints exactly as
before, and its saved model cache stays valid. Setting an explicit legacy default rebuilds
nothing (`a_design_with_no_rigor_builds_as_before`).

## Legacy fallback

A design with no `rigor` key:

1. reads with every feature inheriting a legacy default;
2. evaluates each feature with no rigor scope, so every ops call is the call it was;
3. fingerprints and reuses its saved model as before;
4. saves no `rigor` key until the user moves a dial.

A release before this change opens a design that has `rigor` the same way it opens any newer
format: it warns about the format number, keeps the key it can't read, and builds every
operation as legacy.

## Reproducibility

- **The dial is a table.** Every platform gets the same knobs.
- **`built` is saved, `set` is not re-resolved.** If a later kernel changes the table, saved
  designs don't change. Their operations are built from the saved `built`. Only an edit (moving
  the dial, changing the default) resolves `set` again, and that edit is the user's change
  (`the_saved_built_value_is_what_builds`).
- **Time decides only whether a result comes, never what it is.** A cancelled call returns no
  result, even when the kernel finished first, and leaves its inputs as they came. So a budget
  with room to spare gives the unbudgeted result to the bit (`a_budget_with_room_changes_no_result`).
  A design built under a budget can still build on a slower machine and fail there with "ran out
  of time". That shows up as a refusal, not as different faces.
- **Legacy is bit-identical.** `legacy_booleans_are_the_plain_calls_to_the_bit` compares `.smb`
  bytes of six booleans (planar, cylindrical, spherical; union, subtract, intersect): the plain
  call, `boolean_under(.., &LEGACY)` and the call inside a legacy scope give the same bytes.

## UI sketch

The slider goes in each tool's options panel on the right, below the tool's own inputs, the same
way the Line tool shows its options. It never goes in Preferences.

```
┌ Fillet ─────────────────────────┐
│ Radius        [ 2 mm        ]   │
│ Edges         3 selected        │
│ ...                             │
│ ─ Rigor ─────────────────────── │
│ (•) Design default  ( ) This op │
│ Preview ──●────────────── Proof │   ← disabled while "Design default"; shows the default
│   Fast                          │   ← stop name under the thumb
│ ▸ Knobs                         │   ← collapsed: certify / tolerance / chord /
│                                 │     samples / effort / time, each "dial" or a value
└─────────────────────────────────┘
```

- Picking **This op** writes an override from the slider. Picking **Design default** removes it.
- For a design with no rigor recorded, the slider shows **As built (legacy)**, with no thumb
  position, until the user moves it.
- The design default sits on the design's root row in the browser (Design → Rigor…), using the
  same control. It is not a preference.
- Like any edit, the value goes in the tool's command parameters, so scripts and MCP set it the
  same way (`rigor: {dial: 0.75}` or `rigor: "default"`).

New UI strings will go in `assets/locales/pending/rigor-dial.json` when the panel lands. That
UI change waits for the owner's review with a screenshot.

## What running out of time looks like

The feature fails like any other failed feature: a red timeline marker, and the feature's error
in the tooltip and the problems list.

> Fillet3: ran out of time: the operation is allowed 2000 ms

The fix goes next to the message: "Raise this operation's time budget, or move its dial up."
Nothing after the feature builds on a half result, because no result exists.
`Error::OutOfTime { budget }` is typed, so the panel can offer a "Retry with ×4 time" button.
A user's own Stop stays a stop and is not reported as a failure.

## Open questions for jyviko

1. **SolidCraft's wrapper needs an `OutOfTime` arm.** ops now returns `OutOfTime` from the
   session for every family. SolidCraft's ops wrapper (`call::error` in
   `crates/smlib/src/core/call.rs`) maps `Cancelled` to its own `Cancelled(call)` and skips the
   failure dump. Today any other variant, `OutOfTime` included, is dumped and worded as a kernel
   failure. **Before any SolidCraft call uses a budget, `call::error` needs an `OutOfTime` arm**
   that words it as "ran out of time" and does not dump it. This PR does not add that arm,
   because `call.rs` is in open work. For now, the app's feature test reads the catch-all wording
   (`SmApiBrepCopy: ran out of time: …`). No saved design uses a budget until the panel lands,
   because legacy has none.
2. **Is `Certify::Off` wanted?** Off accepts any boolean result with faces. That is fast, but it
   can let a non-closed result through without saying so. One option: keep Off only for preview
   evaluations (sketch-live, drag), and make the lowest saved stop Closed.
3. **The table values.** Should the Standard stop be exactly today's values? That would make
   legacy a stop and not a separate value. Today's values differ by family, so I kept legacy
   separate.
4. **Records on `Feature` or on the design?** The map keeps this PR small. Once `Feature` has a
   constructor, the record could move onto it, and a format step could carry the map over.
   Deleting a feature currently leaves its map entry orphaned. It is harmless, but should be
   pruned.
5. **Effort as one percentage or per limit?** `Effort { percent }` scales every limit. The
   alternative is named limits per family: halvings, iterations, stations.
6. **Accuracy and the zone tolerance.** Should `accuracy.tolerance` set `Kernel::tolerances`
   (zone, intersection, approximation) for the call, or only the fit tolerances?
7. **A deterministic work budget.** `CancelToken::after_polls` already counts kernel polls.
   Effort could include a poll budget, a "ran out of work" refusal that comes out the same on
   every machine, either next to the wall clock or in its place for CI.
8. **Threads inside a family.** The scope belongs to one thread, like the cancel token's. Any
   family that fans out must hand `rigor::current()` to its threads. Is that acceptable, or
   should `Rigor` be an explicit argument on every `_with` call?

## What this PR has

- `solidcore-ops`: `rigor.rs` (the type, the table, the scope, the deadline, `checked`),
  `Error::OutOfTime`, the session returning it at both stop sites (with its module-doc rule and
  two session tests), booleans reading the certification, and tests in `rigor/tests.rs`.
- `crates/doc`: `rigor.rs` (records, resolve, inherit and override, default change), the
  `Document::rigor` field, the feature fingerprint, evaluation inside the feature's rigor, the
  pending format file, and tests in `rigor/tests.rs`.
- Not included: the panel UI, commands and schemas for setting rigor, SolidCraft's wrapper arm
  for `OutOfTime` (open question 1), and the families other than booleans reading certification,
  accuracy and effort.
