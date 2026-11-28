# A Python API for SolidCraft

Status: the client, its generated bindings and the benchmark are built and measured. Every release
builds the client's wheels from its own binaries, and `pypi.yml` uploads them once the maintainer
turns it on. The conda package is planned. The decisions this needs are in
[Decisions needed](#decisions-needed).

## Findings in short

- **Driving SolidCraft from Python costs less than the plain MCP tools,** at the same pass rate. On
  31 scx and numeric tasks, the generated full API cost 60-66% less than the plain MCP (Haiku,
  Sonnet and Opus 5.5, 1,395 runs, all passed). On four constrained real-world parts it cost 36-54%
  less ([Results](#results-2026-11-26)).
- **scx's hand-tuned vocabulary still helps on top of the full API.** With it, cost fell a further
  27-45%. The owner's review of 30 open-ended robot assemblies also rated it slightly better than
  the full API alone. So it should ship as part of the package
  ([The owner's review](#the-owners-review-of-the-assemblies-2026-11-26)).
- **The scientific stack mattered little on these tasks.** Models imported NumPy, SciPy or pandas in
  a minority of runs and computed the rest in plain Python. The tasks were small enough for that.
- **On open-ended assemblies, stating the finishing bar mattered more than the interface.** Every
  setup built a whole 12-servo quadruped with a correct Raspberry Pi 5 mount. Only when the prompt
  asked for a build-ready design did the models route wiring, model fasteners and produce full bills
  of materials ([assemblies](#open-ended-assemblies-research-and-a-stated-finishing-bar-2026-11-26)).
- **One run per setup cannot separate the setups on open-ended work.** That needs repeated runs,
  which were left out for budget.

## Why

[scx](scx-hybrid/README.md) is a prototype MCP server: the model writes short Python scripts in
SolidCraft's own words (`box`, `sketch`, `hole`, `fillet`, `part`, ...) instead of calling separate
tools. Against the plain SolidCraft MCP, on its 23-task bench with Haiku 5.5, it had the same passes
with 54% fewer input tokens and 59% lower cost.

The scx README credits its small, hand-tuned vocabulary for most of the gain. Its benchmark could not
show that: it never compared the vocabulary with a full API, and `scx.py` lets a script import only
`math` and `itertools`, so the model could not use Python's scientific stack. This work builds the
full API, then measures both questions.

## What we keep from scx, and what we change

| scx | This client |
|---|---|
| Speaks MCP to `solidcraft-cli mcp` and parses JSON out of MCP text blocks | Speaks MCP to `solidcraft-cli mcp --structured` and reads each reply as data from `structuredContent` |
| Runs the model's script with `exec` inside the MCP server, with restricted builtins (not a sandbox) | The agent runs Python in its own process, as it already runs any other code. Nothing ships a script runner, so there is no sandbox to get right |
| Only `math` and `itertools` can be imported | Any library the agent's environment has |
| ~25 hand-written words, `cmd()` for everything else | Generated, typed methods for every command and MCP tool; the vocabulary on top is the next step |
| A diff of the bodies after each `run` | The same change report, printed when a script ends |

MCP stays. Agents without a Python environment keep using it, and so does the bundled modelling
skill.

## What is built

### Install

```sh
pip install solidcraft          # the client (pure Python, small)
uvx solidcraft mcp              # the MCP server over stdio, nothing installed first
```

The package's `solidcraft` command is `solidcraft-cli` with the same arguments, stdin, stdout and
exit code (`uvx solidcraft --version`, `uvx solidcraft run replay.json --in part.sc`); with no
arguments it prints the CLI's help. An MCP client takes it as command `uvx`, args
`["solidcraft", "mcp"]`.

The package carries no binary. The client (the `solidcraft` command and `sc.session()` alike) runs,
in this order:

1. `SOLIDCRAFT_CLI`, when it is set;
2. the `solidcraft-cli` it downloaded earlier for its own version;
3. a `solidcraft-cli` on `PATH` (an installed SolidCraft), when it is the same minor version or newer;
4. otherwise it downloads its own version's `solidcraft-cli` once: this platform's CLI archive from
   the GitHub release `v<version>` (about 100 MB; about 200 MB for the universal macOS one), checked
   against that release's `SHA256SUMS.txt` before anything runs (a mismatch is refused and
   deleted). It prints one line to stderr while it does, and keeps the binary in the user cache:
   `~/.cache/solidcraft/cli/<version>/` (or `$XDG_CACHE_HOME`), `~/Library/Caches/solidcraft/cli/<version>/`
   on macOS, `%LOCALAPPDATA%\solidcraft\cli\<version>\` on Windows.

There are archives for Linux x86_64 and aarch64 (glibc), macOS (universal, notarized as released),
Windows x64, x86 and arm64, and FreeBSD x86_64. `SOLIDCRAFT_NO_DOWNLOAD=1` turns the download off;
with nothing else found, the command says what to do instead. `SOLIDCRAFT_DOWNLOAD_URL` names
another folder holding the release's files (a mirror, or `file://`).

**Offline or behind a proxy:** set `SOLIDCRAFT_CLI` to an installed `solidcraft-cli`, or download the
archive (`solidcraft-cli-<version>-<platform>.tar.gz` or `.zip`, with `SHA256SUMS.txt` beside it to
have it checked) and run `solidcraft install-cli --from <file>`: it puts the CLI in the cache, so
later runs need no network.

`solidcraft install-cli [--from FILE] [--force]` also copies the CLI to a user bin folder
(`~/.local/bin`, or `$XDG_BIN_HOME`; `%LOCALAPPDATA%\Programs\solidcraft` on Windows) so shells and
MCP clients find `solidcraft-cli` by name. It never edits `PATH`; it prints how to add the folder
when it is not on it, and it replaces a different file there only with `--force`.

### The API

```python
import numpy as np
import solidcraft as sc

d = sc.session()       # the session daemon's design, else its own headless SolidCraft
d.cmd.solid.box(length=60, width=40, height=8, corner=[0, 0, 0], body_name="Plate")

# Hole centres computed with NumPy: 6 on a 30 mm circle around the plate's centre.
a = np.linspace(0, 2 * np.pi, 6, endpoint=False)
for p in np.column_stack([30 + 15 * np.cos(a), 20 + 15 * np.sin(a), np.full(6, 8.0)]):
    d.cmd.solid.hole(position=p, diameter=4)        # an ndarray row is a point

print(d.changes())     # + Plate: vol 18596.8142  bbox [0, 0, 0]..[60, 40, 8]  faces 12
print(sc.find("fillet"))                            # command ids with their parameters
d.measure(summary=True)                             # every MCP tool is a method too
d.call("myplugin.entry", width=3)                   # any command by its id
```

- `d.cmd.<area>.<name>(...)`: every engine command, with keyword arguments, type annotations and
  the command's own documentation (`help(d.cmd.solid.fillet)`).
- `d.<tool>(...)`: every MCP tool (`measure`, `body_topology`, `check_parameters`, `save`,
  `export`, ...). `d.tool(name, **args)` reaches plugin tools, which are only known at run time.
- `d.changes()` and `d.mark()`: new, changed and removed bodies with volume, bounding box and faces.
- `sc.headless(design=...)`, `sc.connect("127.0.0.1:PORT")` (the running app, so a person can
  watch), and `sc.session()`.
- `solidcraft-session serve` keeps one design across scripts. An agent runs each script as its own
  process, so without it every script would start empty. A script attached to it prints the change
  report when it ends.

Arguments:
- **A misspelt keyword fails in Python, where it was written.** Every command with a contract
  declares a closed set of parameters, so its method takes no other keywords. The 20 commands
  without an input schema take any keyword.
- **NumPy arrays and scalars, pandas Series and tuples are sent as plain JSON values.** The client
  imports none of those libraries.
- **NaN and infinities are refused before the request is sent,** naming the argument.

The client uses only the standard library: no pydantic and no MCP SDK. It needs Python 3.10 or
later.

```
python/
  pyproject.toml, README.md
  src/solidcraft/
    __init__.py      sc.session(), headless(), connect(), find(), Session, errors
    __main__.py      solidcraft-session serve | stop | status
    _mcp.py          a minimal MCP client over stdio to `solidcraft-cli mcp --structured`
    _daemon.py       the session daemon: one MCP server shared by scripts over loopback, with a token
    _session.py      Session: tools, cmd, call(), changes(), mark(), the version warning
    _encode.py       array-likes to JSON; NaN and infinities refused
    _types.py        Length, Angle, Point3, Point2, Vec3, Names
    _generated/      written by `cargo xtask python`, not committed
  tests/             pytest against a real headless SolidCraft
  bench/             the experiment below
```

### Transport

The client is an MCP client of `solidcraft-cli mcp`, the same server agents use, over stdio. There
is no second server to keep in step with MCP: every tool, current or added later, reaches Python the
moment it reaches agents.

`solidcraft-cli mcp --structured` makes every tool result carry its reply as data, in MCP's
`structuredContent`: `{"result": the reply's JSON, "notes": [the notice and selection blocks]}`
([mcp.md](mcp.md#structured-results)). The server builds it in one place, from the text every tool
already returns, so a new tool needs no change there. Chat clients leave it off, so what they show
the model does not change.

On connect, the server reports its version. If it differs from the version the bindings were
generated from, the client warns that a command that changed may refuse its arguments. If the
server sends no structured replies (a SolidCraft from before `--structured`), the client says so
and stops.

### Generating the bindings

```
solidcraft-cli commands --schemas  ┐
solidcraft-cli mcp (tools/list)    ┴─>  cargo xtask python  ->  python/src/solidcraft/_generated/
```

1. **Two listings, both from the build.** `solidcraft-cli commands --schemas` prints every registry
   entry with its input and output JSON Schemas; the same build always prints the same bytes.
   `tools/list` from a headless MCP server gives every tool with its input schema.
2. **A Rust generator** in `xtask/src/python.rs` (like `xtask/src/features.rs`, which builds
   `docs/features.md` from the same registry) writes the Python. Its output is not committed:
   committing it would make nearly every command PR conflict on it.
3. **Names.** Ids map to attribute paths: `sketch.rectangle.two_point` becomes
   `d.cmd.sketch.rectangle.two_point`. Python keywords get a trailing underscore (`from_`). An id that
   is both a command and a prefix (`sketch.line` and `sketch.line.*`) becomes a callable namespace.
   Names that clash after mapping, or a tool that would hide a session method, stop the generator.
   Planned commands are left out.
4. **Types.** The shared `$defs` become readable aliases (`LengthExpr` becomes `Length`, `PointExpr3`
   becomes `Point3`). Enums become `Literal[...]`, and other schemas map by structure. The docstring
   is the command's documentation, each parameter's description, and the keys of its reply.
5. **Gaps.** 20 commands that work today have no input schema: the three in
   `crates/engine/src/schema/unconverted.txt` (`solid.hole`, `solid.loft`, `solid.thread`), and 17
   that the coverage test only warns about (`drawing.*`, `component.copy_bodies`,
   `assembly.bom.*`, `file.send_to_slicer`, ...). Their methods take any keyword. Giving them
   contracts makes their methods typed.

At 0.51.0 this gives 898 commands and 27 tools.

### Tests

- `python/tests/`: seven pytest tests against a real headless SolidCraft (`SOLIDCRAFT_CLI` names the
  build):
  - every MCP tool in `tools/list` has a method, so a tool added without regenerating fails here;
  - a volume matching its closed form exactly;
  - NumPy values sent as numbers;
  - mistakes failing where they were written;
  - the change report;
  - `find`;
  - the daemon keeping one design across two script processes.
- `xtask/src/python.rs`: unit tests for names, annotations, callable namespaces and clashes.
- `apps/solidcraft-cli/tests/commands_output.rs` and `crates/mcp/src/tests.rs`: `--schemas` and
  `--structured`.

### Packaging: one wheel and the CLI archives on every release

The release workflow already builds `solidcraft-cli` for every platform and signs it on macOS and
Windows. Its `python` job ([releasing.md](releasing.md#python-packages)) makes:

- **One `py3-none-any` wheel**, with no binary: the package with its generated bindings, the
  `.dist-info` with `METADATA`, `WHEEL`, `RECORD` (SHA-256 of each file) and the `solidcraft` and
  `solidcraft-session` entry points, and the licences, written by `cargo xtask python wheel`. The
  packaging scripts gain no Python dependency. There is no sdist, since building one would mean
  building the whole workspace.
- **The CLI archives the client downloads**: each platform's released `solidcraft-cli` alone, taken
  from its package rather than rebuilt (the same, signed bytes), with the licence files:
  `solidcraft-cli-<version>-linux-<arch>.tar.gz`, `-freebsd-x86_64.tar.gz`,
  `-windows-<arch>.zip`. macOS has its notarized `solidcraft-cli-<version>-macos-universal.zip`
  already. They are release assets, so `SHA256SUMS.txt` lists them.
- **Versions** follow PEP 440: `0.52.0`, `0.52.0rc1`, and `0.52.0.dev<date><run><attempt>` for a
  nightly. The bindings keep the release's own version, which names the tag to download from.
- **Checks:** the release job installs the wheel into a clean environment with no `SOLIDCRAFT_CLI`
  and nothing on `PATH`, lets it download the Linux x86_64 archive from a `file://` copy of the
  release's files, checks that the cached binary is the released one, and runs the client's tests
  against it. `python/tests/test_download.py` covers a match, a mismatch, an offline server, the
  cache, an older `PATH` CLI and `install-cli` with a fake release folder.
- **Upload:** `pypi.yml` uploads a release's wheel with PyPI trusted publishing (no stored
  token). A pre-release goes to TestPyPI when it is published, and to PyPI when it is flipped to a
  full release; it never goes to PyPI while it is a pre-release. It stays off until
  the owner registers the publishers and sets `PYTHON_PUBLISH`. The wheel is small, so PyPI's
  100 MB file limit does not apply to it.

## Planned, not built

### The vocabulary in the package

scx's vocabulary, rebuilt on the session as `solidcraft.build`, so the words the experiment
measured ship with the API. The bench's `scx_vocab.py` shows that it runs unchanged on the session.

### conda and pixi

A rattler-build recipe with two outputs: `solidcraft-cli`, the binary for each platform, and
`solidcraft`, `noarch: python`. They would go on a project channel first, then to a conda-forge
feedstock that builds from the source tag. `pixi add solidcraft` would then work from either.

### Recording, and the agent skill

- `Session(record="part.json")` would write every command it sends as a command script that
  `solidcraft-cli run` replays with no Python, so a model's session becomes a regression test.
- A bundled skill, `skills/solidcraft-python/SKILL.md`, would carry `prompts/e.md`'s text: how to
  attach to a session, find commands, read the change report and pick edges and faces. It would stay
  under 2,048 characters, the client limit scx v13 ran into.

## The experiment: full API against the tuned vocabulary

The question: once a model has every command and the scientific stack, does scx's tuned vocabulary
still help, or does it cost?

| Arm | Surface | Imports | Runs Python |
|---|---|---|---|
| A | Plain SolidCraft MCP | none | no |
| B | scx v15.1 as shipped | `math`, `itertools` | scx's `run` tool |
| C | scx v15.1 with imports opened | the scientific stack | scx's `run` tool |
| D | Generated full API with the change report, no vocabulary | the scientific stack | the agent's own Python |
| E | D plus the scx vocabulary | the scientific stack | the agent's own Python |

- B against C measures what the scientific stack adds when the vocabulary stays the same. D against
  E measures what the vocabulary adds on top of a full API. C against D measures the full API
  without the vocabulary, plus the move from an MCP `run` tool to the agent's own Python.
- **Tasks.** The scx bench's 23 tasks (`docs/scx-hybrid/tasks/`), and 8 new tasks whose numbers
  need computation (`python/bench/tasks/numeric.json`). The new tasks are a cam outline from a
  formula, a geometric least-squares circle through measured points, standoffs from a CSV table, a
  volume optimum, a convex hull, terraces from an exponential, a turned part revolved from a table
  and a beam sized by a deflection limit. Their reference builds are checked against closed forms
  (`python/bench/make_numeric.py`) and were written before any run. The ladder's tasks (L1-L9) are
  not in the repository, so they were not run.
- **Method.** scx's own: 3 runs per task and setup, the same checker (every expected body within
  0.1% in volume and 0.05 mm in box, nothing extra), on Haiku 5.5, Sonnet 5.5 and Opus 5.5. Each run
  is `claude -p` in an empty folder with only that setup's tools, and none of the user's settings,
  CLAUDE.md, skills or memory (`python/bench/run.py`).
- **Decision.** The vocabulary ships only if E beats D by at least 5% in cost or tokens with no lost
  pass, on most models. If it does not, the full API ships alone, and scx's tuning lessons (reply
  formats, error hints) go into the change report and the error messages instead.

### Results (2026-11-26)

1,395 runs (31 tasks × 5 setups × 3 runs × 3 models), all passed. Mean per task, at list prices:

| Setup | Haiku 5.5: cost, input tokens, tool calls | Sonnet 5.5 | Opus 5.5 |
|---|---|---|---|
| A plain MCP | 0.78¢, 175k, 5.6 | 12.67¢, 168k, 6.4 | 17.02¢, 152k, 5.0 |
| B scx, `math` only | 0.13¢, 18k, 1.5 | 2.92¢, 21k, 1.5 | 3.25¢, 14k, 1.2 |
| C scx with the scientific stack | 0.14¢, 22k, 1.7 | 2.86¢, 21k, 1.7 | 2.88¢, 14k, 1.2 |
| D full API, no vocabulary | 0.29¢, 45k, 2.2 | 5.09¢, 41k, 2.2 | 5.82¢, 31k, 2.0 |
| E full API with the vocabulary | 0.19¢, 29k, 1.6 | 3.72¢, 35k, 1.7 | 3.21¢, 20k, 1.2 |

- **The full API is far cheaper than the plain MCP.** D cost 60-66% less than A on every model,
  with 74-79% fewer input tokens, and nothing failed.
- **The vocabulary still pays on top of the full API.** E cost 35%, 27% and 45% less than D
  (Haiku, Sonnet, Opus), with no lost pass. By the rule above, the vocabulary ships. The gain comes
  from discovery: in D's 93 Haiku runs the model called `help()` 70 times, `sc.find()` 43 times and
  fetched the modelling guide 20 times, to learn argument names that E's prompt already gives. D's
  fixed overhead is small (its first request is about 1k tokens larger than scx's).
- **The scientific stack changed little here.** Opening imports in scx (B to C) moved cost by +4%,
  -2% and -11%. Models imported NumPy, SciPy or pandas in only 6-11 of the 24 runs per setup on the
  numeric tasks. The rest computed in plain Python, and every run passed. These tasks are small
  enough for plain Python, so they do not test the stack's value on large or freeform data (L9's
  mesh fitting is where it should matter).
- **No setup failed a task**, so these tasks measure cost, not capability. The difference in what a
  model can do (L8, L9) needs harder tasks.
- **Absolute costs are not comparable with the scx README's.** Those runs asked for a long written
  "Thinking" report at the end of each run; these do not. Compare setups within one table.

### Open-ended assemblies: pilot (2026-11-26)

One prompt, as a person would write it, with no dimensions or parts: "Design a full quadruped robot
assembly using high torque hobby servos, including mounting points for a Raspberry Pi 5." Every
setup also had web search and fetch. Each run was capped at $6, with one run per setup on Sonnet
5.5 and Opus 5.5: $9.74 for the runs and $0.75 for judging.

Scoring (`python/bench/assembly.py`, `python/bench/judge.py`):
- **Core checks** record facts from the final design file: parts, the bill of materials, the Pi
  5's 58 × 49 mm M2.5 hole pattern and the standard 49.5 × 10 mm servo tab pattern (found on any
  part, in any orientation), servo-sized bodies, interference between parts, and the kinds of
  component named (battery, regulator, fasteners, wiring).
- **An AI judge** (Opus) scores six criteria from 1 to 5 and an overall score out of 10. It sees
  four renders, the facts and the designer's summary, never the setup. It checks the summary's
  claims against the renders and the facts.

| Setup | Sonnet: judge /10, cost, calls | Opus: judge /10, cost, calls |
|---|---|---|
| A plain MCP | 7, $1.35, 24 | 6, $1.72, 18 |
| B scx, `math` only | 7, $1.26, 22 | 6, $0.64, 6 |
| C scx with the scientific stack | 7, $0.86, 13 | 6, $0.62, 3 |
| D full API, no vocabulary | 6, $1.01, 21 | 5, $0.87, 12 |
| E full API with the vocabulary | 7, $0.89, 13 | 6, $0.51, 6 |

- **Every run built a whole 12-servo quadruped assembly** with the Pi 5 on its correct pattern and
  no part collisions, mostly around 35 kg·cm standard-size servos. None reached "complete": no run
  routed or secured wiring, few modelled fasteners, and bills of materials stayed at 10-19 items.
- **No run used web search**, on either model. Every part these runs needed (a standard servo, the
  Pi 5) is well known, so the models designed from memory. Research needs parts a model cannot
  know.
- **Opus stopped sooner** (3-18 tool calls, against Sonnet's 13-24) and scored slightly lower.
  With an open prompt, where a model decides it is done sets how far the design goes.
- **One run per setup cannot separate the setups.** The judge spread is 5-7 throughout. Cost
  follows the earlier pattern: plain MCP is the most expensive, and scx and the vocabulary setups
  are the cheapest.

### Open-ended assemblies: research and a stated finishing bar (2026-11-26)

Two variants of the quadruped prompt, one run per setup on Sonnet 5.5 and Opus 5.5 ($44.02
including judging):

- **Research:** "...using Feetech STS3250 serial bus servos...". This is a real 12 V servo with a
  45.22 × 24.72 × 35 mm case, not the standard 40 × 20 mm. The checker counts bodies at that case
  size.
- **Build-ready:** the original prompt plus "It should be ready to build: everything to buy in a
  bill of materials, wiring routed and secured, and fasteners modelled."

| Variant | Sonnet: judge /10 (A-E) | Opus: judge /10 (A-E) | Sonnet cost per run | Opus cost per run |
|---|---|---|---|---|
| Research | 6, 6, 6, 5, 6 | 5, 4, 4, 6, 6 | $0.74-1.43 | $0.30-1.31 |
| Build-ready | 8, 8, 8, 8, 8 | 7, 8, 7, 8, 8 | $1.68-3.70 | $3.65-5.31 |

- **Stating the bar is what moves the result.** With it, every run routed wiring, modelled
  fasteners, and produced a bill of materials of 37-74 items, and the judge's scores rose from 5-7
  to 7-8. The bar also doubles to triples the cost. Opus's build-ready runs came within $1 of the
  $6 cap.
- **Research happened only when needed, and only partly.** Sonnet searched the web in every
  research run (2-7 calls) and quoted the true 45.22 × 24.72 mm case and the 12 V supply in its
  summaries. Its designs, though, contain no servo body at that size: it modelled pockets and
  brackets but not the servo itself. Opus searched in none of its runs, yet already knew the case
  (45.2 × 24.7) and modelled 8-12 bodies at that size in three of five runs. The part turned out
  not to be unknown to Opus.
- **The interference check needs a fastener rule.** The build-ready Opus designs show 1,000-2,000
  mm³ of overlap between parts. That is mostly screws modelled at their nominal diameter inside
  undersized pilot holes (self-tapping into plastic). Overlap between a fastener and its host part
  should be excluded.
- **The setups still cannot be told apart** on one run each: build-ready scores 7-8 everywhere.
  Plain MCP (A) was again the most expensive setup on both models.

### The owner's review of the assemblies (2026-11-26)

The owner opened the 30 assembly designs (`python/bench/work/review/`) and reviewed them:

- **The full API with the scx vocabulary (E) produces slightly better designs than the full API
  alone (D).** This agrees with the cost results (E cost 27-45% less than D on every model), so the
  vocabulary ships on top of the full API, as the decision rule above requires.
- **Sonnet's designs had aspects the owner preferred:** an IMU, and wires modelled as curved runs.
- **Opus's designs were more correct:** proper wiring buses, and the right components.

The judge's first rubric folded all wiring into `detail`, so it could not see the difference
between these two. Rubric v2 (`judge.py --rubric v2`) scores four things apart: the electrical
architecture (power path, regulators, buses and harnesses), the wire routing (real routes, slack
at the joints, securing), sensing (an IMU, a power switch), and fasteners with the bill of
materials.

Re-scored with v2 ($2.59), the build-ready means are:

| | Electrical | Wiring | Sensing | Overall |
|---|---|---|---|---|
| Sonnet | 4.0 | 3.6 | 3.0 | 7.2 |
| Opus | 3.6 | 3.2 | 2.4 | 7.0 |

The judge now agrees with the owner on wiring and sensing (Sonnet higher), and keeps E above D
(5.50 against 5.17 over six designs each). It disagrees on the electrical architecture: the owner
found Opus more correct. The judge sees renders, a summary of the checker's facts and the
designer's own description. It does not see the part names or the bill of materials, where buses
and connectors show, so it may be crediting the descriptions. The next judge version should be
given the parts tree and the bill of materials.

Three build-ready designs leave failed `Move` features in their timelines (8-56 each). Their geometry is still what the model
built: the reopened design matches the live state body for body, so the scores stand. A model
that leaves failed features behind has not finished, though, so the next round adds "no failed
timeline features" as a core check, and excludes the overlap between a fastener and the part it
screws into from the interference check.

### Real-world tasks (2026-11-26, Haiku 5.5)

Four platforms with design freedom and hard constraints from off-the-shelf parts
(`python/bench/realworld.py`): a 5-inch quadcopter frame, a 900 mm flying wing, a quadruped body
shell and a 1/10 RC car chassis. Each task gives the parts' dimensions as a datasheet would: motor
and flight controller hole patterns, servo cut-outs and tab patterns, battery spaces, a gear mesh
to compute, and an airfoil given as coordinates with sweep and washout. Everything else is the
model's choice. `python/bench/checker.py` scores 11-38 constraints per task on SolidCraft itself:
- **Holes** come from `body_topology`'s cylindrical faces: diameter within 0.1 mm, axis within
  0.1 mm.
- **Empty and full regions** are the overlap between the design and a probe solid built for the
  check.
- **The wing's sections** must follow the spec's surfaces within ±0.4 mm at four span stations.

A probe the kernel cannot evaluate is reported as unverified, not failed. Every reference build
passes every check, and each deliberately wrong build fails the check it should
(`python bench/realworld.py --selftest`).

| Setup | Passes | Checks passed | Cost / task | Input tokens | Tool calls |
|---|---:|---:|---:|---:|---:|
| A plain MCP | 12/12 | 100% | 1.80¢ | 345k | 9.2 |
| B scx, `math` only | 12/12 | 100% | 0.55¢ | 28k | 1.5 |
| C scx with the scientific stack | 12/12 | 100% | 0.63¢ | 55k | 2.4 |
| D full API, no vocabulary | 11/12 | 99.6% | 1.16¢ | 147k | 4.8 |
| E full API with the vocabulary | 12/12 | 100% | 1.01¢ | 104k | 3.7 |

On Sonnet 5.5 (3 runs per setup, $9.07 in all):

| Setup | Passes | Checks passed | Cost / task | Input tokens | Tool calls |
|---|---:|---:|---:|---:|---:|
| A plain MCP | 12/12 | 100% | 26.75¢ | 324k | 11.0 |
| B scx, `math` only | 12/12 | 100% | 11.08¢ | 86k | 4.7 |
| C scx with the scientific stack | 11/12 | 98.1% | 9.49¢ | 61k | 4.1 |
| D full API, no vocabulary | 12/12 | 100% | 15.92¢ | 128k | 4.7 |
| E full API with the vocabulary | 12/12 | 100% | 12.37¢ | 90k | 3.4 |

- The ordering matches the scx bench: the full API is cheaper than the plain MCP (36-44% on Haiku,
  40-54% on Sonnet), the vocabulary on top saves more, and scx's single `run` tool is cheapest.
  The flying wing is where the setups converge: on Sonnet it cost 18-38¢ in every setup, because
  the work is the geometry itself, not finding commands.
- C's one Sonnet failure is the spec's, not the model's: it cut the battery bay 0.25-0.5 mm larger
  than the pack for a fit clearance. The 1.5 mm wall check is measured from the pack's nominal
  faces, so the clearance ate into it. A requirements-based tier should measure walls from the
  cavity the model cut.
- D's one failure left two of the four servo cut-outs out of the quadruped.
- These tasks still give every constraint as a number, so Haiku reaches nearly every check. The
  next tier should state requirements instead of geometry and let the numbers follow from them:
  - a quadcopter's wheelbase chosen so its propeller tips clear by 5 mm;
  - a wing sized for a wing loading, with its centre of gravity at 25% of the mean aerodynamic
    chord and the battery placed to get it there;
  - parts given as bodies in the design for the model to measure, as in the scx L8 test, rather
    than as numbers.

## Repository rules this touches

- `.agents/skills/solidcraft-performance/SKILL.md` allows Python only for asset authoring, lint and
  review tools, and now names this client as the one other exception: standard library only, run in
  the user's own Python, and never shipped or imported by the app.
- `.agents/skills/solidcraft-packaging/SKILL.md`: PyPI and conda distribution belong with Homebrew and
  AUR (package distribution, not a platform integration), but publishing accounts are the
  maintainer's decision.
- CI runs the Python tests on an approved pull request or a merge group whose diff touches
  `python/`, `xtask/src/python.rs`, `packaging/python/`, `crates/mcp/` or
  `crates/engine/src/schema/`, and on no other. `scripts/ci/python-client.sh` runs
  `cargo xtask python` and `pytest` against the `ci`-profile `solidcraft-cli` that Native Linux
  tests already builds on its warm disk (about 15 seconds), or, for a change to `python/` alone,
  the one Reference tests builds. pytest is installed from `python/tests/requirements.txt` (exact
  versions, with hashes). The release's `python` job runs them again against the Linux x86_64 wheel.

## Decisions needed

The maintainer closed #12642 (scx built into the MCP server with a sandboxed interpreter) with:
"SolidCraft will never ship a Python interpreter, sandboxed or not ... Agents that want to script
can run Python on their own machine and drive SolidCraft through the existing MCP tools and
commands." This client follows that: the app ships no interpreter, and agents run Python in their
own environment. The same reason ("everyone's broken scripts become our bug reports") bears on
whether the project hosts and publishes the client.

1. **Python in this repository:** allow the client in `python/` (the skill wording above), or keep
   only `--schemas` and `--structured` here and move the client to a separate repository.
2. **The vocabulary:** the experiment's rule is met (E beat D by at least 5% with no lost pass, on
   every model), and the owner's review agrees. Port it into the package as `solidcraft.build`.
3. **Distribution:** turn on `pypi.yml` by registering its trusted publishers on PyPI and TestPyPI
   and setting `PYTHON_PUBLISH` ([releasing.md](releasing.md#python-packages)), and later choose a
   conda channel. `solidcraft` and `solidcraft-cli` were free on both PyPI and conda-forge on
   2026-11-25.
4. **CI:** a Python job, as above.

## Next steps, one pull request each

1. The vocabulary in the package, and the agent skill.
2. A Python CI job.
3. The conda recipe and channel.
4. Repeated assembly runs, to separate the setups on open-ended work. The harness is ready:
   `run.py --sets assembly --tasks quadruped_buildready --reps 3`, about $2-5 a run.
5. Recording sessions as command scripts.
