<!-- SPDX-License-Identifier: Apache-2.0 -->

# Kernel rig corpus

This folder holds the kernel's operation-level cases as data, one entry each, by area: every case of SolidCraft's kernel rig (`tests/kernel-cases/` there, which takes no new cases), and every kernel case added since. An entry holds the operation that made the case's result, its operand bodies as B-rep records (`solidcore-record`), its parameters, the fix set and the expected values. It's the input of solidcore's operation-level gate (#2394). The gate checks whole fillets, shells, booleans, sweeps and threads against exact or closed-form values.

The runner that replays the entries lands with `solidcore-ops`. The operations are SolidCraft's compositions of kernel calls (picks, per-edge fillet sessions, setbacks, the overflow retry, the stitch, error mapping), and their one home is `solidcore-ops` (`docs/layers.md`). Until then, `crates/solidcore-record/tests/kernel_cases_corpus.rs` checks the corpus itself:
- every record reads as the record and passes its structural validation;
- every operation's operands are given records or earlier outputs;
- the manifest counts every entry.

## Where it comes from

SolidCraft's cases were exported. In a SolidCraft checkout at the commit `manifest.json` names (`solidcraft`), pinned to the solidcore rev it names (`solidcore`), run:

```sh
cargo xtask kernel-cases --cli target/release/solidcraft-cli --export DIR
cargo xtask kernel-cases --ladder --cli target/release/solidcraft-cli --export DIR
```

Each case runs as the rig runs it, with the kernel's operations traced at the boundary between SolidCraft and the kernel (SolidCraft's `crates/smlib`, `trace.rs`). The trace records:
- each body made from a record, with that record;
- each operation, with its operands, its parameters and its outputs or error;
- each body written back as a record.

## Layout

- `<area>/<case>.json`: one entry. A ladder case's area is `ladder/<tier>`.
- `<area>/<case>.bodies.json.gz`: the entry's operand records, `{"b<n>": record}`, gzip.
- `fixes.json`: the `UpstreamFixes` flags turned on, the same for every entry (SolidCraft's `fixes()`).
- `manifest.json`: the counts per area and every case that isn't expressible, with its reasons.

## An entry

| Field | Meaning |
|---|---|
| `case`, `area`, `sources`, `what` | The case, as SolidCraft's rig names it. |
| `calls` | The operations that made the result, in order: `op`, `inputs` (bodies `b<n>`), `params`, and `outputs` or `error`. Usually one operation on records; a chain where operations pass bodies on without a record between them. |
| `bodies` | The operand bodies given as records: `trusted` and `keeping_splines` say how SolidCraft read each record into the kernel. |
| `params` | The operation's arguments as SolidCraft passed them. A pick by points is `{kind, points, tolerance, picked}`: each point's pick is `{index, at}`, the entity's index in the body's list of its kind and where it is (an edge's ends, a vertex's point), or `{miss}`. |
| `result` | The bodies the case ends with: `made` by an operation, or an `operand` carried through unchanged. |
| `outcome` | `ran`, and for a stopped run the engine's error and the failing operation's `kernel_error`; `raw_counts`, the records' own face, edge and vertex counts. |
| `expected` | SolidCraft's `expected.json`, unchanged (see its README for each field). |
| `rig` | SolidCraft's result at the pin: `status`, `error`, `refused_with`, `measured` (body count, merged face, edge and vertex counts, volume, area) and `known_failing`, the case's line in its area's known-failing list. |
| `golden` | A ladder case's golden status and counts (never its time). |
| `operation_expressible` | The kernel alone accounts for the result: the last traced operation failed, or every final body is an operation's output or an operand. |
| `expressible` | That, and every value graded is the kernel's. |
| `why_not` | Why the operation isn't the kernel's alone. |
| `ungraded` | What SolidCraft grades that the kernel doesn't measure: `continuity`, `watertight`, `files`, `results`, `construction`, `flat_pattern`, counts that SolidCraft's merging across analytic surfaces changes, and a refusal graded on SolidCraft's wording of the kernel's error. |
| `traced_ops` | Every operation the case's commands made, for reference. |

## Adding a case

New kernel cases come here. A case about a command, the UI or a product rule stays in SolidCraft as a test there.

1. **The entry**, `<area>/<case>.json` in an existing area (a new area is a new folder): `case` (`<source number>-<short-name>`, the issue or pull request it comes from), `area`, `sources`, `what`, `calls`, `bodies`, `records`, `params`, `result`, `expected`, `operation_expressible`, `expressible` and, where they apply, `why_not` and `ungraded`, as [An entry](#an-entry) gives them. `rig`, `golden`, `outcome` and `traced_ops` come with an export and are left out of a hand-written entry.
2. **The operand records**, `<case>.bodies.json.gz` beside it (gzip, no timestamp): `{"b<n>": record}`, one solidcore B-rep record per body `bodies` names. Keep them small: the bodies the operation takes, not a whole design.
3. **The expected values** in `expected` (SolidCraft's `expected.json` format) are closed-form or exact: a volume, an area, a distance or a count derived by hand or from analysis, with the derivation in `what`. Never a kernel's output, solidcore's included, and never a value taken from commercial CAD.
4. **The manifest:** add one to the area's `cases` in `manifest.json` (and to `not_expressible` with the reason, for a case the kernel alone does not account for).

To export a case from SolidCraft instead of writing it: draft it there as `tests/kernel-cases/<area>/<id>/recipe.json` and `expected.json`, run `cargo xtask kernel-cases --area <area> --cli target/release/solidcraft-cli --export DIR`, copy `DIR/<area>/<id>.json` and its `.bodies.json.gz` here, and delete the draft there.

Today `cargo test -p solidcore-record --test kernel_cases_corpus` checks the structure (the three checks at the top); the runner that executes the entries and grades them against `expected` arrives with `solidcore-ops` (stage 3b).

## Updating

Re-export SolidCraft's cases at the new pin and replace their entries in one pull request, keeping the cases added here. The SolidCraft and solidcore commits are in `manifest.json`.
