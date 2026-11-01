# Contributing to SolidCraft

Agents: start from the router in [AGENTS.md](AGENTS.md), which points to a short skill in [`skills/`](skills/README.md) for each kind of work. This page stays the full human guide.

SolidCraft is an open-source parametric 3D CAD app written in pure Rust.
You sketch on a plane, constrain the sketch, turn profiles into solids, and
change a parameter to watch the timeline rebuild. It runs as a desktop app (Windows, macOS, Linux), in
the browser (wasm + WebGPU/WebGL2), headless from a CLI, and as an MCP server that AI agents can
model with.

Help is welcome, whether that's a one-line fix or owning a whole area. This page covers how to
build it, how the code is laid out, the few rules that aren't negotiable, and where help is
needed most.

Before choosing a task, check the [command inventory](docs/features.md) and
[roadmap](docs/roadmap-detail.md) for implemented capabilities and current priorities.

Questions, ideas or want to say what you're working on? Join the [Discord](https://discord.gg/FtU4NQNVDH).

## Kernel work is paused

We're evaluating a new geometry kernel. Until that's decided, please don't open issues, pull
requests or discussions about kernel work: booleans, fillets, chamfers, shell, offsets, sweeps and
lofts, threads and holes, STEP/IGES import and export, tessellation, or anything else that changes
`crates/kernel/`. The pinned issue on the tracker explains it. New ones get the `kernel-paused`
label (`.github/workflows/kernel-paused.yml`) and a maintainer answers them. Crashes and data loss
are still taken as issues, and everything outside the kernel carries on as normal.

## Get it running (about 10 minutes on a fast machine)

You need stable Rust 1.95 or newer (required by egui/eframe 0.36). For the web
build you also need `rustup target add wasm32-unknown-unknown` and [`trunk`](https://github.com/trunk-rs/trunk)
([documentation](https://trunk-rs.github.io/trunk/)).

```sh
git clone https://github.com/tangency-labs/solidcraft && cd solidcraft
cargo run --release -p solidcraft -- --sample      # desktop app with a sample part
cargo xtask test                                   # unit, engine and UI-scenario tests
cargo xtask ci                                     # everything a PR must pass (see below)
```

SolidCraft is the product, repository, Cargo package and executable name.
Build the desktop app with `cargo build --release -p solidcraft`, or the CLI with
`cargo build --release -p solidcraft-cli`. Run the CLI with
`cargo run --release -p solidcraft-cli -- commands` (or `target/release/solidcraft-cli commands`
after building). The old name appears only in the compatibility shims listed in
[docs/compatibility.md](docs/compatibility.md); don't add new uses of it.

Web: `cd apps/solidcraft-web && trunk serve --release`, then open the printed URL (`?sample`
opens the sample part, `?webgl` forces WebGL2). Saving and opening files in the browser needs
HTTPS or localhost, because it uses the Origin Private File System.

Windows from Linux: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.

### NixOS: `nix develop`

NixOS has no `/usr/lib`, so nothing is on `LD_LIBRARY_PATH` and a plain `cargo run` finds none of
the libraries the app loads at run time: the Wayland ones winit opens the window through, the EGL
loader wgpu renders through, and libdbus-1, which rfd's file dialogs reach xdg-desktop-portal
through (a dialog then never appears and the pick returns no path):

```
ERROR solidcraft: winit EventLoopError: ... The wayland library could not be loaded
ERROR eframe::native::run: Exiting because of error: WGPU error: Failed to create surface for any enabled backend: {}
ERROR rfd::backend::xdg_desktop_portal::portal::libdbus: Can't connect to a portal: libdbus-1.so not found
```

`nix develop` (the [`flake.nix`](flake.nix) in the repository root) supplies Wayland,
X11, EGL, Vulkan-loader and D-Bus runtime libraries. It also exposes the NixOS
driver manifests in `/run/opengl-driver/share`, so Vulkan can discover the system
drivers. Enable `hardware.graphics.enable` on NixOS; other Linux distributions
may need a matching [nixGL wrapper](https://github.com/nix-community/nixGL).
The default development shell retains the Rust/C toolchain already on `PATH`;
`nix develop .#rust` supplies the pinned toolchain and builds in `target/nix`.

To launch, use `nix run .#solidcraft -- --sample`. To install the app's terminal
commands, desktop entry and generated icons, use `nix profile install .#solidcraft`.
`nix build` alone creates a build result; it does not install a launcher in your
desktop menu. The installed `solidcraft` command runs the wrapped app; `solidcraft-cli` is included as
well. See
[Nix installation and troubleshooting](docs/nix.md) for remote-flake commands,
desktop integration and how to distinguish an old executable on `PATH`.

## How the code is laid out

The [architecture page](docs/architecture.md) has the layer table. The short version:

- `crates/geom`: vectors, planes, meshes, measures.
- `crates/sketch`: our own constraint solver, plus profile finding.
- `crates/kernel`: the B-rep kernel boundary. It wraps [truck](https://github.com/ricosjp/truck),
  and it's the only crate allowed to touch truck. Fillets, chamfers, shell/offset, Delete Face,
  Split Face, STEP/IGES read and write, and face provenance are our own code here. Patched
  copies of three truck crates live in `vendor/` and are excluded from our fmt/clippy rules.
- `crates/doc`: the document. It holds parameters and expressions, the feature timeline and its
  incremental rebuild, components and occurrences, joints, appearances, and persistent naming.
- `crates/io`: design files and mesh formats (3MF, STL, OBJ).
- `crates/engine`: the session and the command registry. **Everything the user can do is a
  command**, and the toolbar, dialogs, scripts, CLI, control channel and MCP all run the same
  commands.
- `crates/ui-egui`: the desktop/web front end (egui + wgpu). It's swappable; nothing below it
  depends on egui.
- `crates/mcp`, `apps/solidcraft`, `apps/solidcraft-cli`, `apps/solidcraft-web`.

`cargo xtask layers` fails the build if a lower layer depends on a higher one.

## Rules that aren't negotiable

1. **Independent.** SolidCraft is independent. Behaviour is designed from public
   documentation, general CAD conventions and user feedback. Reference tests use analytic values
   and open-source tools. We don't drive, script, automate or extract data from commercial CAD
   products, and we never copy another product's code, icons, artwork, help text, tooltips,
   templates or materials. Command ids are our own (`solid.fillet`).
2. **No GPL/LGPL/AGPL code, not even reading it.** That rules out OpenCASCADE, FreeCAD
   (including planegcs), SolveSpace, CadQuery/OCP, LibreCAD and QCAD. SolidCraft is
   MIT OR Apache-2.0, and its solver and blends are written from the maths. (Running an
   open-source application to get reference values for a test is fine; reading its source is
   not.) Permissive
   dependencies are fine. Note which one you're adding and why in the PR. THIRD-PARTY-LICENSES.txt
   lists it once the change lands: the Generated files workflow rewrites it on `main`.
   Nightly/manual CI also scans advisories, bans and sources with cargo-deny, and a PR that
   changes Cargo.lock or a Cargo.toml checks the advisories; see
   [the local command and coverage limits](docs/book/src/dev/testing.md#dependency-advisories).
3. **Every asset is original or openly licensed and has a row in
   [ATTRIBUTION.md](ATTRIBUTION.md).** Icons are drawn in code (`crates/ui-egui/src/icons.rs`).
   `cargo xtask assets` enforces this.
4. **Never crash.** People trust a CAD app with their work.
   - Non-test code never uses `unwrap`, `expect`, `panic!` or indexing on input-derived data.
     There's no `unsafe` outside `apps/solidcraft/src/open_documents.rs` (macOS file-open events),
     `apps/solidcraft/src/native_crash.rs` (Windows native crash capture) and
     `crates/threedx-client/src/macos.rs` (the 3Dconnexion driver's client API on macOS), and the
     compiler denies it.
   - An unsupported case returns an error ("not supported yet: …"); it never panics.
   - Every call into truck goes through `solidcraft_kernel::guard`.
   - Every crash fix comes with a regression test.
5. **Everything is a command, with a test.** A new feature means a `CommandSpec` in
   `crates/engine/src/cmd/` with input and output schemas (see
   [Adding a command](docs/book/src/dev/commands.md#adding-a-command); a test warns about a
   command without them, and will fail it once required), engine tests that measure the result (volume, area,
   face/edge/vertex counts against a value you can derive), and for UI work a scenario (below).
   A command on the toolbar also ships with its help tooltip (step 6 of Adding a command).
6. **One set of camera controls.** Every page or canvas that shows a view (the modelling
   viewport, the Render preview, and any new page such as a flat pattern or drawing) calls
   `navigation::handle` (`crates/ui-egui/src/navigation.rs`) for its camera input, with
   `NavCaps` saying whether it can orbit (off for flat 2D pages) and pick. New canvases must use
   `navigation::handle`; never read drags, the wheel or pinch to move the camera yourself. The
   `hand_rolled_camera_input_is_rejected` test fails if a page does.

Labels, parameter docs, messages and docs use the terms in the
[glossary](https://docs.solidcraft.io/reference/glossary.html): one term for each idea. A change that adds a new
idea adds its term there.

[AGENTS.md](AGENTS.md) has the long form of these rules. It's written for AI agents, but it's
the most complete description of the house style.

## Tests

| What | Where | Run |
|---|---|---|
| Unit and engine tests | next to the code, or `*_tests.rs` files | `cargo xtask test` (or `cargo test --workspace --profile ci`) |
| UI scenarios | `crates/ui-egui/tests/scenarios/*.json` | part of `cargo test` |
| Reference tests | `cargo xtask oracle` | `tests/oracle/` (see below) |

A geometry change that changes the robustness goldens re-records them in the same pull request:
`ROBUSTNESS_GOLDENS_BLESS="why it changed" cargo test --profile ci -p solidcraft-engine --lib robustness_goldens::`
(see [docs/robustness.md](docs/robustness.md)); CI runs every seed on pull requests that touch geometry.
[docs/mutation-testing.md](docs/mutation-testing.md) shows how to find kernel behaviour these
black-box suites miss.

**UI scenarios** drive the real app headless: start a tool, click a plane or a sketch point,
press keys, then assert on the result.

```json
[
  {"start": "sketch.create"},
  {"click": {"plane": "XY"}},
  {"start": "sketch.rectangle.two_point"},
  {"click": {"sketch": [0, 0]}},
  {"click": {"sketch": [40, 30]}},
  {"expect": {"errors": 0}}
]
```

A new `.json` file in that folder is picked up automatically. To record a bug before it's fixed,
put `{"pending": "why"}` in the scenario, and remove it in the PR that fixes the bug.

**A failing test without its fix can't merge as-is:** it would turn `main`'s CI red for everyone.
Either include the fix in the same PR, or mark the test `#[ignore = "fails until #123 is fixed"]`
(it can then merge right away), and remove that line in the PR that fixes the bug.

**Use the `ci` profile.** CI runs the tests with `--profile ci`: light optimisation, line tables,
no LTO, and debug assertions off (truck has debug-only assertions that reject valid geometry). It
builds several times faster than `--release`. If a test fails only in a plain debug build, say so
in the PR. `cargo xtask test` builds the tests with it and then runs every test binary at once
(`cargo test` runs them one after another); arguments after `--` go to each binary, e.g.
`cargo xtask test -- shell`. Set `SOLIDCRAFT_TEST_JOBS` to a positive integer to opt into a
shared bound on top-level test/side commands for `cargo xtask test` and `cargo xtask ci`.
For example, `SOLIDCRAFT_TEST_JOBS=2` allows two commands at once; the sequential wasm/doc
worker consumes one of those slots. Every binary and side check still runs after failures.
Without the variable the existing concurrency is unchanged. With it set, each test binary
inherits `RUST_TEST_THREADS` and `ACTION_MATRIX_JOBS` of `max(1, cpus / jobs)` — the machine's
parallelism shared between the binaries in flight — unless you set either one explicitly;
explicit caller overrides always win. Cargo's build-job setting remains an independent caller
override; this limit does not change test filters, timeouts or baselines.
Tests run beside each other on a busy machine, so a test must not depend on wall-clock timing.

**Ready pull requests test what they touch.** CI maps changed files to workspace packages and
runs Clippy, the test build and tests for those and their dependents. The
[package selector](scripts/ci/affected.py) and [CI workflow](.github/workflows/ci.yml) define
which changes require native checks; documentation, packaging, other-workflow and bot-script-only
changes run the repository checks only, on pull requests and on `main`. Workspace-wide changes,
merge-queue groups, nightly runs and Run workflow select the full workspace. A push to `main`
reuses a successful merge-queue run for the same commit, and runs only the repository checks
when no code changed since the last push to `main` that passed; otherwise it runs the full
suite. Doc tests, reference tests and the Windows jobs run on `main` and nightly only.

**CI shards the tests.** One job builds them into a cargo-nextest archive; two shards run on
pull requests and four during full validation. Every test runs in its own process: don't rely
on state left by another test.
Pull requests skip a few heavy tests (one to twelve minutes each), which run in full validation
(the merge queue, nightly, manual runs and pushes to `main` not already tested by the queue).
Check that validation after merging. They are listed in
`.config/nextest.toml` and the [testing page](docs/book/src/dev/testing.md):

- `solidcraft-engine`: `tests::hostile_params_never_panic`, everything in `tests::robustness` and
  the seeds of `tests::robustness_goldens` (`seed_00` … `seed_95`; pull requests that change the kernel, truck or the goldens run all but the slowest, `SLOW_SEEDS` in `ci.yml`),
  `pattern_suppress_tests::enclosure_arc_extinction_keeps_a_real_sharp_inner_lip_after_resize_and_reopen`,
  `pattern_suppress_tests::enclosure_datums_follow_wall_height_and_board_depth_with_material_proofs`,
  `cmd::plastic::tests::enclosure_snap_fit_follows_resized_front_wall`, the `atomic_commands` contract tests
- `solidcraft-kernel`: `tests::boolean_identity_on_tangent_primitives`, `tspline::tests::hostile_tsplines_never_panic`, and the `boolean_props` property tests
  (these run only in the nightly "Property tests" job: their inputs are random, so main skips them too)
- `solidcraft-ui-egui` scenarios: `acceptance_sensor_enclosure`

**The action matrix** tries every ordinary action on every face, edge, vertex and body of a
corpus of designs against a checked-in ratchet ([docs/action-matrix.md](docs/action-matrix.md)).
A pull request that makes an action pass should record it with `cargo xtask action-matrix --bless`;
one that makes a passing action fail fails the sample every CI run checks.

**The reference tests** live in [`tests/oracle/`](tests/oracle/), each a `recipe.json`
in our own format with an `expected.json` beside it. SolidCraft rebuilds the recipe and compares
body count, volume, area, centre of mass and bounding box (and, where there is a `reference.stl`,
the surface itself). Expected values are either worked out in closed form
(`tools/oracle-analytic/`) or computed by building the same recipe in FreeCAD, run as a separate
program (`tools/freecad-oracle/`); each file says which. The generated
[reference test report](docs/oracle.md) has the current case count and results. The reference-test
CI job runs `cargo xtask oracle` during full validation; pull-request runs skip it. A case that
fails because of a known bug names its issue in `expected.json` (`"known_failure": "#123"`);
never change an expected value to match SolidCraft's output.

## Changing the design file format

A design file says which format it is in (`"format": "solidcraft/N"`). If your change saves
something new in a design (a field, a feature type, a new kind of name), or makes older designs
mean something else, it needs a format change. **Add one file, and edit nothing shared:**

1. Create `crates/doc/src/format/pending/<what-it-is>.rs` (lowercase, dashes). Start it with a
   `//!` note: what is saved, what older designs get without it, and that the last release asks
   for an update instead of meeting it.
2. If older designs must be rewritten as they open, put `pub fn step(v: &mut Value) ->
   crate::Result<()>` in the same file. Pending steps run in file-name order, after the older
   formats' steps.
3. Don't touch `FORMAT`, `JSON_STEPS`, the history in `crates/doc/src/format.rs`, another
   change's file, or any `v<N>/` folder: those are fixed once released.

That's all. The build counts the files: while any are pending it writes the next format number,
which no release reads, so a release refuses a design that has your change instead of losing it.
A file also lists the pending changes it has (`format_pending`), so a later development build
runs only the steps that file is missing. Two pull requests that each add a change merge without
a conflict. `crates/doc/src/format/pending/README.md` has an example.

**At release time** the maintainer runs `scripts/fold-format-pending.sh`, which moves the pending
files into `crates/doc/src/format/v<N>/` and fixes that number for good (see
[docs/releasing.md](docs/releasing.md)). Never add a file to a `v<N>/` folder afterwards. Drawings
follow the same rules with their own numbers, from 1, in `crates/doc/src/drawing/format/` (see its
`pending/README.md`), and the same script folds them.

## Lists many changes add to

Some files gain an item in many pull requests at once. They are kept so that two changes that
each add an item only meet when their items are neighbours, which spares everyone a conflict
and a fresh CI run:

- **Growing Rust lists**: use slices (`const ITEMS: &[Item] = &[...];`) without a written
  length. `cargo xtask lists` rejects non-test const/static arrays with more than ten lines
  of entries, except fixed standard or algorithm tables listed with reasons in `xtask/src/lists.rs`.
  Built-in themes need only a JSON file in `assets/styles/ui/themes/`, with a unique nonnegative
  integer `order` (the picker order) and its own `provenance` line; keep existing order values.
- **Message catalogs** (`assets/locales/*.json`): one message per line, sorted by id. Add your
  message anywhere and run `cargo xtask lists --fix`; `cargo xtask lists` (in `cargo xtask ci`
  and the Lists workflow) fails on an unsorted catalog or an id that appears twice.
- **`mod` lines**: one per line, in the sorted group they belong to (`cargo fmt` keeps each group
  sorted). Add a new module into the existing group, not as a new group at the end.
- **Commands without schemas** (`crates/engine/src/schema/unconverted.txt`): sorted, one id per
  line, and it only shrinks. Converting a command removes its line; a new command is never added
  to it.
- **Toolbar commands without help** (`BACKLOG` in `crates/ui-egui/src/command_help.rs`): sorted,
  one id per line, and it only shrinks. Writing a command's help removes its line; a new command
  is never added to it.
- **`ui.prefs` settings** (`docs/control-protocol.md`): one line per setting, sorted by name, in
  the list under the protocol table; add a new setting on its own line rather than editing a shared
  row.
- **Languages** (`crates/ui-egui/src/i18n.rs`): one per line, English first, then by id (a test
  checks it).
- **Generated files**: `THIRD-PARTY-LICENSES.txt` and `docs/features.md` are rewritten on `main`
  after each merge by the Generated files workflow (`cargo xtask licences`, `cargo xtask
  features`). Don't regenerate or edit them in a pull request.
- **Design format**: one file per change, never an edit to a shared number (above).
- **New UI strings and attribution rows**: one new file per change, `assets/locales/pending/<slug>.json`
  and `attribution/pending/<slug>.md`, instead of editing the catalogs or the ATTRIBUTION.md table;
  `cargo xtask fold-pending` folds them on `main` (see the README in each folder). A new English
  message also needs every catalog delivered complete (`COMPLETE` in `crates/ui-egui/src/i18n.rs`:
  today ar, he, ja, vi, and zh) in the same pending file; `cargo xtask i18n --check` reports any gap.

## Sending a change

1. Work is tracked in [GitHub issues](https://github.com/tangency-labs/solidcraft/issues), labelled
   by area (`area:kernel`, `area:sketch`, `area:doc`, `area:ui`, `area:infra`). For anything
   bigger than a bug fix that has no issue yet, open one first. **Claim the issue with a draft
   PR** before you start, so two people don't build the same thing:
   - Check nobody has it: `gh pr list -R tangency-labs/solidcraft --state open --search "#N"`.
   - Branch from the latest `main`, commit `git commit --allow-empty -m "claim: #N"`, push, and
     open a **draft** PR titled `<summary> (#N)`, e.g. `Fillet: variable radius along a chain (#123)`.
   - **The lowest-numbered open PR for an issue wins.** If yours lost, close it (or offer to help
     on the other one).
   - The bot records the claim when the PR is opened: it assigns you, adds `in progress` and
     says so on the issue. It reads `#N` in the PR title and `Fixes #N` in the body. If someone
     already holds the issue, or an older open PR references it, it says so on your PR instead.
   - Want to stake a claim before you have a branch? Comment `/claim` on the issue (below).
   - Push work in progress at least daily. A draft with no pushes for 72 hours may be taken over:
     comment on it first, then open your own claim.
   - Mark the PR ready for review when it is done.
2. Branch from `main` and keep the PR to one topic.
3. Run `cargo xtask ci` before pushing. It runs fmt, the asset, licence and layer checks,
   clippy (`-D warnings`), then the tests with the wasm build check alongside. A PR has to pass it.
   Heavy CI runs after review: a pull request gets the formatting and repository checks and a
   quick compile (`cargo check`) when you push. Once a maintainer has reviewed it they add the
   `approved-for-ci` label when the PR is ready to merge, which runs the full CI (build, tests,
   goldens, Clippy); every push after that runs the full set again. Maintainers merge approved
   PRs in groups by area, so PRs that touch the same code meet a conflict at most once.
4. Reference the issue (`Fixes #12`). In the PR, say what you tested and how. For geometry work, include the measured numbers (and
   the expected ones).

Commenting `/claim` assigns you, adds the `in progress` label, and replies with its automatic cleanup
deadline. It frees a claim with no linked PR after 72 hours, or a claim with a PR after 14 days
without pushes. These automatic cleanup deadlines are separate from the daily progress and
72-hour takeover policy above. A draft PR that references the issue claims it the same way.
`/unclaim` releases the bot claim early (only you or a maintainer can). Issues a maintainer has
taken are never freed automatically.

Help reviewers see what the words mean. For observable CAD, UI or model behavior, please
include actual descriptive screenshots or a short recording in the issue or PR. This evidence
is required for changes or bugs that can be demonstrated in the app, including kernel,
geometry, assembly and backend work. Coding agents should run a small reproduction and
capture the evidence themselves; please don't ask a human for evidence the agent can produce.
Show the actual failure or result, preferably before and after, with brief steps and
expected/actual behavior. An arrow or annotation can help identify the detail that matters.

Flowcharts and Mermaid diagrams cannot replace evidence of visible behavior. Diagrams are
useful for architectural reasoning when actual screenshots cannot illustrate the point.
Changes with no useful observable behavior do not need screenshot or video evidence. Use a
small example without private designs or sensitive details; attach the media or link to an
accessible recording rather than committing generated review artifacts.

To show a picture without committing it, attach the media to the PR (it goes to the PR's own
attachment storage and renders inline in the comment). A gist is the better home for a log or
another text file, and you link it; a gist does not render a picture inline. The review artifacts
stay out of the diff that way, and nothing has to be pushed to a branch of its own.

Attaching works in the browser for anyone who can comment: drag the file into a comment on the
pull request. A command-line agent without a browser session usually cannot attach a file (the
upload needs write access to this repository). Say so in the pull request, and the person running
the agent or a maintainer can attach it.

Keep an eye on your PR's status, but don't push to it only to catch up with `main` (no merge of
`main`, no Update branch button, no rebase for that alone): every push restarts the whole CI
run for everyone waiting behind it, and `main`'s own run catches what lands in between. When you
push a change anyway, or GitHub reports a conflict, rebase onto the latest `main` (push your
branch with `--force-with-lease`) or merge `main` in once, and resolve conflicts without losing
others' changes. Don't re-run CI on an unchanged commit either: a real failure fails again.
Re-run only a job that runner trouble cancelled, with "Re-run failed jobs". While a task remains
active, follow an open PR through its status changes; at handoff, clearly identify anything
still awaiting merge.

Much of SolidCraft so far was written by AI coding agents (Claude) working in parallel, each
owning one area, with a human setting direction and reporting bugs. Their working notes are in
[AGENTS.md](AGENTS.md). Human contributors are just as welcome. Use whatever tools you like, as
long as the change meets the rules above.

Two quick checks run on every pull request (`.github/workflows/pr-checks.yml`, not required): it
fails when a lower-numbered open PR by someone else already closes the same issue (the label
`not-a-duplicate` skips it), or when the description lacks the `Changes a default: no` / `Changes a default: yes — <what>, old behaviour kept behind preference <name>` line (docs-only and bot PRs are exempt).

## Labels and triage

The triage bot labels every new issue and PR (`.github/workflows/triage.yml`). It only adds labels;
correct them freely and it won't put them back.

- **PRs** get `area:*` from the paths they change (`crates/kernel` and `crates/geom` →
  `area:kernel`, `crates/ui-egui` and the apps → `area:ui`, `.github/`, `scripts/`, `docs/` →
  `area:infra`, and so on).
- **Issues** get `bug` (in-app reports and the bug template), `enhancement` (feature requests),
  `priority:P1` for crashes, and an `area:*` from the title or the reported command. Until it has
  a priority, an issue carries `needs-triage`; setting a priority removes it.
- **Maintainers** comment `/triage area:kernel P1` (any existing label; `P0`–`P3` for a
  priority, which replaces the old one; `-label` removes one) to sort an issue or PR.
- **Short bug reports without steps to reproduce** get `needs-info` and one comment asking for
  the steps (and the version, when it's missing). An app report that arrived empty is asked to
  paste the report the app copied to the clipboard. The reporter's reply, or an edit that adds the
  steps, removes the label; an issue that stays in `needs-info` for 14 days is closed, and can be
  reopened any time. Feature requests, app reports with their details, and maintainers' issues
  are never asked.
- `needs-triage` is the queue: [open issues with it](https://github.com/tangency-labs/solidcraft/issues?q=is%3Aopen+label%3Aneeds-triage).

## Where help is wanted most

**Kernel speed (slow fillets, booleans, STEP):** please don't rewrite the current kernel code for speed; it is being replaced by [solidcore](https://github.com/bherbruck/solidcore) ([progress](https://tangency-labs.github.io/solidcore/)). Report slow cases with a file under the `kernel-perf` label, and put kernel effort into solidcore instead. See [#1597](https://github.com/tangency-labs/solidcraft/issues/1597).

See the [`help wanted`](https://github.com/tangency-labs/solidcraft/labels/help%20wanted) and
[`good first issue`](https://github.com/tangency-labs/solidcraft/labels/good%20first%20issue) labels.
In order of how much they'd help users:

1. **Fillet and blend robustness.** Fillets are our own code (truck has no fillet operation).
   Constant, variable-radius and chord fillets work, and so do corners where fillets of
   different radii meet, including three rounds meeting at a vertex. Next are setback corners, fillets along curved edges between curved
   faces, and fillets that consume neighbouring faces.
2. **Boolean robustness.** A random-design fuzzer currently gets about half of its designs
   through without a kernel error. Each failing seed is a reproducible bug.
3. **Drawings, CAM and mesh tools.** These are out of scope for now (see
   [ROADMAP.md](docs/roadmap-detail.md)), but anyone who wants to own one is welcome.

## Updating the generated feature list

After adding or renaming commands, run `cargo xtask features`. The registry is the
source of truth for `docs/features.md`; do not hand-edit the generated rows.
The generator sorts commands by ID and omits global and per-section counters,
so additions to unrelated areas no longer rewrite shared summary lines.

If two branches still conflict in this file (for example, adjacent additions in
the same area), first resolve the command registry and source conflicts, then run
`cargo xtask features` again and stage the regenerated file. Do not use Git's
`union` merge driver: it can silently retain stale or duplicate command rows.
The generator tests cover stable ordering and additions that leave existing lines
unchanged. The initial switch to sorted rows is a one-time larger diff.

## Pictures in the user guide

The user guide's screenshots and short clips (`docs/book/src/media/`) are captured from the real
app by `examples/demo/doc_shots.py`, one function per figure. Each run starts a fresh app with its
own settings, so your preferences never show. When a change makes a figure out of date, run
`python3 examples/demo/doc_shots.py <page or shot>` (it needs Xvfb, ffmpeg, Pillow and a release
build) and commit the new file. Stills come in both themes: `--light` makes `<name>.light.png`,
and the page shows the one that matches the book's theme (an `<img class="dark-ui">` and an
`<img class="light-ui">`). A new figure is a new function there, an ATTRIBUTION row in
`attribution/pending/`, and a `<video>` (clips) or the image pair on the page. Keep a still under
about 250 KB and a clip under about 1 MB.

## Licence

By contributing you agree that your contribution is licensed as MIT OR Apache-2.0, like the rest
of SolidCraft. SolidCraft isn't affiliated with any CAD vendor; product names are trademarks of their owners (see NOTICE).
