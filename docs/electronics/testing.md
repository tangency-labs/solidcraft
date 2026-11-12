# Electronics testing and verification

Part of the [electronics plan](README.md) and [RFC 0008](../rfcs/0008-electronics.md).

Two rules govern this area:

1. **Connectivity is a proof, not a picture.** A netlist or Gerber is correct when an independent
   check agrees, not when the screen looks right.
2. **A fab package is money.** It is verified against independent parsers and a real fab's DFM
   before it is called done.

## 1. Test pyramid

| Level | What | Where |
|---|---|---|
| 1. Unit | Part validation, pad/pin mapping, net resolution, rule evaluation, geometry | `crates/eda/src/*_tests.rs` |
| 2. Model | ERC/DRC rule matrix, connectivity, annotation, variants, BOM | `crates/eda/src/erc_tests.rs`, `drc_tests.rs` |
| 3. Format | KiCad/Eagle/IPC/Gerber/Excellon/SPICE read/write and loss reports | `crates/io/src/eda_*_tests.rs` |
| 4. Command | `library.*`, `schematic.*`, `board.*`, `fab.*`, `sim.*` contracts | `crates/engine/src/eda_tests.rs` |
| 5. Simulation | SPICE analytic benchmarks; thermal/PI/SI benchmarks | `crates/spice/src/tests.rs`, `fea` tests |
| 6. Render | Board materials, component fidelity, golden images | `crates/ui-egui/src/render_*_tests.rs` |
| 7. Robustness | Hostile files, caps, huge boards, bad rules, recovery | `crates/io/src/eda_fuzz_tests.rs` |
| 8. Compatibility | Real KiCad/Eagle projects, netlist and round-trip equivalence | `tests/eda-compat/` (user-supplied corpus) |
| 9. Performance | Open, pan, DRC, zone fill, route, save, render | `docs/perf.md` entries and a bot report |

Tests live in the area's existing file, never a per-bug file (#3226). No wall-clock assertions; use
deterministic seeds and fixtures.

## 2. Compatibility corpus

The corpus is **user-supplied and never vendored** (KiCad libraries and projects carry their own
licences). A contributor runs the corpus locally and reports results in the PR. CI uses small,
original fixtures authored for the repo.

For every project:

- open without errors;
- netlist equality with KiCad's exported netlist (nets, pins, refdes, values);
- reference designator stability;
- footprint and pad count equality;
- layer stack and drill equality;
- export and re-import with a loss report that names every difference;
- no silent drops: every unsupported construct is either preserved or reported.

A round trip that "opens" but loses a net is a failure.

## 3. ERC matrix

Each case has a fixture and an expected severity/location:

| Case | Expected |
|---|---|
| Output connected to output | Error |
| Power input with no power source | Error |
| Input pin unconnected | Warning |
| No-connect on a connected pin | Error |
| Unconnected hierarchical port | Error |
| Duplicate reference designator | Error |
| Bus member not defined | Error |
| Label on a wire that resolves to two nets | Error |
| Pin type conflict (input vs power output) | Error |
| Alternate pin function in use | Report, not error |

## 4. DRC matrix

Rules are tested at their boundary, not only in the middle:

| Rule | Cases |
|---|---|
| Clearance | exact pass, 1 µm under, different net classes, same net |
| Track width | minimum, minimum − 1 µm, per class |
| Annular ring | exact, below, micro via |
| Hole size and spacing | minimum, hole-to-hole, hole-to-edge |
| Board edge | exact, below, cutout edge |
| Courtyard | touching, overlap, both sides |
| Solder mask | sliver, expansion, via tenting |
| Silkscreen | over pad, minimum width, clipped text |
| Zone | isolated island, thermal relief, clearance, priority |
| Differential pair | gap, skew, uncoupled length |
| Length | min/max, matched group, tuning |
| Connectivity | unrouted net, split net, zone-only connection |

DRC ordering and messages are deterministic; the same board and rules produce the same report.

## 5. Fabrication verification

- **Gerber X2:** file functions and attributes present; apertures match the board; polarity correct;
  no zero-area or self-intersecting flashes; drill alignment checked against Excellon.
- **Independent check:** open the output in at least one independent Gerber viewer, and parse it with
  the app's reader; compare layer count, apertures and object counts.
- **IPC-2581:** schema-valid, round-trips the netlist, layers and stackup.
- **BOM/CPL:** quantities, refdes sets, DNP/variant state, rotation conventions and side match the
  board and the fab profile.
- **DFM:** the selected profile's rules pass; a known-bad board produces the expected violations.
- **Real fab:** at least one board per milestone is ordered and accepted without manual Gerber edits.
  The order confirmation and fab DFM result are the evidence.
- **Panel:** rails, fiducials, mouse-bites and V-grooves are present and within the profile.

A Gerber that "looks right" is not evidence. Parsed geometry and a fab acceptance are.

## 6. SPICE benchmarks

| Circuit | Reference | Tolerance |
|---|---|---|
| RC step response | Exponential with `τ = RC` | 1% |
| RLC series/parallel | Damping and resonant frequency | 1% |
| Half-wave and bridge rectifier | Peak, ripple, conduction angle | 2% |
| Common-emitter amplifier | Bias point, gain, bandwidth | 5% |
| Ideal op-amp integrator | Analytic ramp | 1% |
| 555 astable (macromodel) | Frequency and duty | 2% |
| Buck converter (switching) | Duty, ripple, efficiency trend | 5% |
| Lossless transmission line | Delay and reflection | 1% |
| Diode temperature | `Vf` vs temperature | 2% |
| Non-convergent circuit | Refusal with node and iteration | exact |

Every result records convergence, tolerances, temperature and model provenance. A non-converged run
never produces a waveform labelled valid.

## 7. Thermal, power and signal integrity

- Thermal: reuse the simulation plan's benchmarks (1D wall, fin, thick cylinder) plus a board case
  with known copper fraction and a component power step; compare with a hand calculation.
- IR drop: a straight copper trace and a via chain against `R = ρL/A` and a via resistance model.
- PDN: a single capacitor and plane against an analytic impedance curve.
- SI: microstrip/stripline impedance against the closed-form for the same stackup; crosstalk against
  a coupled-line calculation.
- Conservation: total current in = out; total power in = dissipated + stored.
- Validity: every result reports the range and assumptions; out-of-range is a warning, not a pass.

## 8. Board body and mechanical

- Board volume equals outline area × thickness minus holes and cutouts, within the kernel's exact
  tolerance.
- Holes and cutouts appear at the exact positions and diameters.
- Component envelopes match courtyard and height from the part revision.
- Enclosure clearances use envelopes, not render meshes.
- Parameter references (board size, hole spacing) survive edits and configuration changes.
- Export to DXF/STEP matches the board exactly.

## 9. Render tests

- Board materials are distinguishable by ID: mask, copper, ENIG/HASL, silk, FR-4 edge, solder.
- Component fidelity is recorded and visible: vendor model, parametric, envelope.
- Golden images with a small perceptual tolerance for a fixed scene; material and geometry IDs
  asserted separately.
- Instancing and LOD: a 10,000-component board renders within the budget and LOD changes are visible
  in the metadata.
- No third-party logos or artwork in bundled scenes.

## 10. Robustness and hostile input

- Malformed KiCad/Eagle/IPC/Gerber/Excellon/SPICE files: named error or loss report, never a panic.
- ZIP bombs, path traversal, duplicate entries, oversized files: refused before extraction.
- NaN/Inf coordinates, self-intersecting outlines, zero-width tracks, negative sizes: refused.
- Huge boards: caps enforced with a message that names the limit.
- Library cycles, missing dependencies, hash mismatches: refused.
- Crash recovery and `.bak` restore for a design with electronics.
- Opening a newer format on an older build: warning and no rewrite.

## 11. Determinism

The same inputs must produce the same:

- netlist, with stable net and pin ordering;
- BOM and CPL rows, with stable sorting;
- zone fill polygons;
- Gerber/Excellon bytes where the format allows, or identical semantic content otherwise;
- DRC/ERC report order;
- ratsnest and connectivity results.

Non-determinism is a bug, not a UI detail.

## 12. Performance

Budgets are in [requirements.md](requirements.md) §3.2. Measure with release builds on a quiet
machine and report medians with range and machine, following `docs/perf.md`.

- Open, pan, DRC, zone fill, routing feedback, save and render are measured per milestone.
- A >2× regression on the same machine class is reported; CI does not fail on wall-clock time.
- Large-board memory is measured, not guessed.

## 13. CI and platform

| Job | Runs | Gates |
|---|---|---|
| Linux CPU | Unit, model, format, command, SPICE, thermal | Required |
| Linux wasm | `cargo xtask wasm`, offline test | Required |
| macOS CPU | Full suite | Required |
| Windows CPU | Full suite | Required |
| Render | GPU tests where an adapter exists, skipped with a reason otherwise | Recorded |
| Compatibility | Small original fixtures; user corpus reported in the PR | Required for format PRs |

## 14. Manual acceptance checklist

1. Import a real KiCad project; compare the netlist; fix an ERC error; export and re-import.
2. Place and route a two-layer board; pass DRC; change a net; confirm the diff report.
3. Run ERC/DRC/DFM headless through the CLI and parse the typed results.
4. Generate a fab package; open every layer in an independent viewer.
5. Order the board; record the fab acceptance.
6. Run a SPICE transient and a thermal case; check convergence and validity text.
7. Render the board alone and in its enclosure; check fidelity labels.
8. Undo/redo across schematic, board and library edits; save, close, reopen with the library removed.
9. Cancel zone fill, DRC, simulation and render; the app stays responsive and the design is intact.
10. Screenshot or record the observable change.

## 15. What we will not do to make tests pass

- Compare a netlist against our own previous output instead of an independent reference.
- Treat "opened without error" as compatibility.
- Ignore a loss report because the result looks right.
- Loosen a clearance rule to make DRC green.
- Accept a Gerber that only our own reader can parse.
- Call a non-converged simulation a result.
