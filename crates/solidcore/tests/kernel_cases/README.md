<!-- SPDX-License-Identifier: Apache-2.0 -->

# Kernel rig corpus

This folder holds the kernel's operation-level cases as data, one entry each, by area: every case of SolidCraft's kernel rig (`tests/kernel-cases/` there, which takes no new cases), and every kernel case added since. An entry holds the operation that made the case's result, its operand bodies as B-rep records (`solidcore-record`), its parameters, the fix set and the expected values. It's the input of solidcore's operation-level gate (#2394). The gate checks whole fillets, shells, booleans, sweeps and threads against exact or closed-form values.

The runner that replays the entries is `crates/solidcore-ops/tests/kernel_cases.rs` (see [Running](#running)). The operations are SolidCraft's compositions of kernel calls (picks, per-edge fillet sessions, setbacks, the overflow retry, the stitch, error mapping), and their one home is `solidcore-ops` (`docs/layers.md`). `crates/solidcore-record/tests/kernel_cases_corpus.rs` checks the corpus itself:
- every record reads as the record and passes its structural validation;
- every operation's operands are given records or earlier outputs;
- the manifest counts every entry;
- no two nonempty traces contain exactly the same calls, operand import options and decoded
  operand records. Descriptions, expected values and gzip filenames do not make a replay
  distinct. Numbers are compared exactly, without a tolerance. Empty traces remain records
  of unsupported application operations and are not kernel replay inputs.

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
- `<area>/<case>.jpg`: the case's render, where it has one: the site's case gallery shows it.
- `fixes.json`: the `UpstreamFixes` flags turned on, the same for every entry (SolidCraft's `fixes()`).
- `manifest.json`: the counts per area and every case that isn't expressible, with its reasons.
- `known_failing.json`: the kernel cases that fail today, the one list: `case` (`<area>/<case>`), `issue` and `reason`. Seeded from the entries' `rig.known_failing`; an entry whose `issue` is `to file` has none yet.

## An entry

| Field | Meaning |
|---|---|
| `case`, `area`, `sources`, `what` | The case, as SolidCraft's rig names it. |
| `calls` | The operations in execution order: `op`, `inputs` (bodies `b<n>`), `params`, and `outputs` or `error`. Keep discarded tool construction and handled attempts too, even when their outputs do not reach the final bodies. |
| `continued` | Optional boolean on a call with `error`. `true` means the application handled this refusal: replay must observe a refusal, continue to any later calls, and grade the final result. A handled refusal can be the last call when the application keeps an earlier body. Omitted or `false` keeps the terminal-refusal behavior. A marker after a terminal refusal is invalid. |
| `bodies` | The operand bodies given as records: `trusted` and `keeping_splines` say how SolidCraft read each record into the kernel. |
| `params` | The operation's arguments as SolidCraft passed them. A pick by points is `{kind, points, tolerance, picked}`: each point's pick is `{index, at}`, the entity's index in the body's list of its kind and where it is (an edge's ends, a vertex's point), or `{miss}`. |
| `result` | The bodies the case ends with: `made` by an operation, or an `operand` carried through unchanged. |
| `outcome` | `ran`, and for a stopped run the engine's error and the failing operation's `kernel_error`; `raw_counts`, the records' own face, edge and vertex counts. |
| `expected` | SolidCraft's `expected.json`, unchanged (see its README for each field). |
| `rig` | SolidCraft's result at the pin: `status`, `error`, `refused_with`, `measured` (body count, merged face, edge and vertex counts, volume, area) and `known_failing`, the case's line in its area's known-failing list. |
| `golden` | A ladder case's golden status and counts (never its time). |
| `gallery_note` | Optional: what the case gallery (the site's `cases.html`) says beyond `what` and the case's status, such as why it passes, how its round ends or what its failure means. Nothing reads it but the site. |
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

`cargo test -p solidcore-record --test kernel_cases_corpus` checks the structure (the three checks at the top).

## Running

```sh
cargo test --release -p solidcore-ops --test kernel_cases -- --ignored --nocapture
```

replays every entry through `solidcore-ops`' public operations, with the fixes in `fixes.json`, and grades it against its `rig` values (a hand-written entry, against `expected`): the bodies' count, their face, edge and vertex counts against the records' (`outcome.raw_counts`; the rig's merge faces across analytic surfaces) exactly, the volume within `expected.tolerance` (the rig's relative 1e-3 where none is given), and every refusal where the rig's trace has one. It prints, per area, pass, fail, unsupported (not the kernel's alone, rated unsupported by the rig, or not carried by the conversion yet), unported (an operation `solidcore-ops` doesn't have yet, named) and known-failing, and fails on a case that fails, a listed case whose result moves from the rig's (fixed, or failing differently: shrink the list), or a hang. The test is `#[ignore]`d, so the workspace run skips it; run it with `--ignored`. Environment: `KERNEL_CASES_AREA` (comma-separated areas, `ladder` taking every tier: the CI shards), `KERNEL_CASES_CASE` (a substring of the case's path), `KERNEL_CASES_TIMEOUT_S` (180 by default), `KERNEL_CASES_REPORT` (a file for the table) and `KERNEL_CASES_REPORT_ONLY` (report without failing).

## Updating

Re-export SolidCraft's cases at the new pin and replace their entries in one pull request, keeping the cases added here. The SolidCraft and solidcore commits are in `manifest.json`.

The five entries refreshed for #2722 carry `export_provenance` with their source,
exporter patch and fix configuration; the manifest pins describe the remaining
original exports. See [the duplicate audit](duplicate-audit.md) for reproduction
steps and the evidence for each removed entry.
