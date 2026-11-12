# Electronics: contributing, process and risks

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

## 1. Before code

1. Check open PRs and issues; electronics may already have a claim.
2. Land **RFC 0008** as a design PR with no behaviour change. It supersedes the PCBCraft rows of RFC
   0001; the CamCraft rows stand.
3. On acceptance, update `ROADMAP.md` (remove PCB from "deferred", add an Electronics milestone) and
   the "Where help is wanted most" section in `CONTRIBUTING.md`.
4. Claim slices with draft PRs; the lowest-numbered PR wins a contested issue.

## 2. Slicing

- One topic per PR. A board editor PR that also restyles the ribbon will not land.
- Slice boundaries are in [roadmap.md](roadmap.md); every slice builds, tests and is useful alone.
- Independent slices (library, schematic, board, fab, simulation) can run in parallel after the E0
  contracts exist. Dependent slices stay sequential.
- Every PR states what changed, why, what was measured, what was not verified, and the
  `Changes a default:` line.

## 3. Repository rules that apply

| Change | Rule |
|---|---|
| Electronics data in `.sc` | One `crates/doc/src/format/pending/electronics.rs` with a `//!` note; never edit `FORMAT` or another change |
| UI strings | `assets/locales/pending/<slug>.json` |
| Assets and materials | Original or openly licensed; row in `attribution/pending/<slug>.md`; read `assets/AGENTS.md` |
| Commands | Input and output schema, or an entry in `schema/unconverted.txt` that only shrinks |
| Command list | `cargo xtask features`; never hand-edit `docs/features.md` |
| Preferences | Row in `preference_settings!`; old behaviour reachable; label/help through catalogs |
| Sorted lists | `cargo xtask lists`; catalogs, `mod` lines and languages stay sorted |
| Tests | The area's existing file, not a per-bug file (#3226) |
| Generated files | Never hand-edit `docs/features.md` or `THIRD-PARTY-LICENSES.txt` |
| New crates | Row in `xtask/src/layers.rs`, row in `docs/architecture.md`, licence, README (shared), `cargo xtask layers/shared/wasm` |
| Dependencies | No GPL/LGPL/AGPL, not even reading it; `cargo xtask licences --check` |

## 4. Evidence

- UI changes: real screenshots or a recording captured by the contributor.
- Import/export: a real project, the netlist comparison, the loss report, and a round-trip result.
- Fabrication: parsed Gerber/Excellon/IPC output, an independent viewer check, and where possible a
  real fab acceptance.
- Simulation: benchmark table, convergence evidence, and the stated validity range.
- Performance: medians, range, machine and load, following `docs/perf.md`.
- Platform claims: the checklist in [testing.md](testing.md) §14, with machines named and gaps
  stated.
- `cargo xtask ci` before every commit/push.

## 5. Licence traps

| Item | Reality | Rule |
|---|---|---|
| KiCad application | GPLv3 | No reading, no porting. Formats from public specifications and user files |
| KiCad libraries | CC-BY-SA 4.0, design exception | Import from the user's disk; do not bundle |
| ngspice, Xyce, Qucs-S | GPL | No SPICE code; clean-room from textbooks and public device equations |
| Vendor 3D models | Per-vendor terms | User-supplied; record provenance; do not redistribute |
| Datasheets and logos | Copyright/trademark | Link only; original parametric geometry for bundled content |
| Gerber X2 | Published specification | Implement from the specification |
| IPC-2581 / ODB++ | Open/published, terms vary | Verify before shipping fixtures or test data |

Implementation sources are specifications, textbooks and papers. Do not copy code from a GPL
project, and do not read it "for reference".

## 6. Risks

| Risk | Why it matters | Mitigation |
|---|---|---|
| Scope | EDA is larger than the mechanical CAD it joins | Vertical slices, E3 gate, depth after a working product |
| Compatibility | No one switches if their project does not open | Import first, loss reports, real-project corpus, round trips |
| Router quality | Push-and-shove is the hardest interactive feature | Walk-around first; shove behind a measured gate; do not claim parity |
| Undo cost | Snapshotting a large board per edit can freeze the app | Arc-shared board chunks, measured per-edit cost, journal if needed |
| Large-board performance | 10k components must stay interactive | Per-layer indexes, incremental DRC, instancing, LOD, budgets |
| Fab correctness | A wrong Gerber costs money and time | Independent parsers, DFM profiles, real orders, checksums |
| Part data licensing | Vendor models and libraries are not freely redistributable | Import from the user, embed per design, provenance, no bundling |
| SPICE fidelity | Clean-room device models are hard to validate | Analytic benchmarks, documented device set, validity ranges |
| Simulation over-trust | A pretty waveform looks authoritative | Convergence evidence, assumptions, non-claims |
| One-file size | Boards, parts and models can grow large | Content hashes, per-design budgets, linked models with warnings |
| UI complexity | Two editors, layers, nets, rules, waveforms | Shared kit, cross-probe, keyboard-first, testable components |
| Community expectations | "Replace KiCad" invites parity demands | State the compatibility contract and the supported subset |
| Review bandwidth | Large editor PRs stall | Small validated slices, one topic each, evidence in the PR |

## 7. Decision log

| # | Decision | Rationale |
|---|---|---|
| D1 | Full EDA is in-tree, not PCBCraft | Owner decision; one design, one assembly, one render, one simulation stack |
| D2 | One `.sc` document with an electronics section | Board and enclosure cannot drift; shared parameters and history |
| D3 | Stable ids for parts, nets and pads; names are labels | Re-annotation and renames must not break connectivity |
| D4 | Schematic owns connectivity; board owns geometry | A clear source of truth and a reviewable difference report |
| D5 | Parts are immutable revisions, embedded in the design | Offline, reproducible, portable designs |
| D6 | Import is a first-class feature | Adoption requires opening existing KiCad/Eagle projects |
| D7 | Zone fills, netlists, DRC, BOM and board bodies are derived | No stale source data; caches carry input hashes |
| D8 | No network required | Local, offline, free; future providers are optional |
| D9 | Fabrication output is verified against independent parsers and real fabs | The output costs money |
| D10 | Slices ship usable products | A partial KiCad is not useful; a complete small tool is |

## 8. First three PRs

1. **RFC 0008** — scope, model, compatibility contract, gates. No code.
2. **Electronics document envelope** — electronics section, part snapshot types, one format pending
   file, save/reopen/undo, library pack entry kind. No editor.
3. **KiCad library import + board body** — import a user library, insert a part snapshot, build the
   exact board body and component envelopes, render them. This is the E0 vertical slice.

Only then does the schematic editor start.
