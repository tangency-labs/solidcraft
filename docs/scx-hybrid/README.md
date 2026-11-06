# scx: a script-first "hybrid" MCP server for SolidCraft

scx lets a language model build CAD parts in [SolidCraft](https://github.com/tangency-labs/solidcraft) by
writing short Python scripts in SolidCraft's own words (`box`, `cylinder`, `sketch`, `hole`, `fillet`,
`part`, ...) instead of calling dozens of separate tools. Every script runs at once in SolidCraft, and every
reply ends with a short list of what changed, with volumes and bounding boxes, so the model can check its
work without another call.

On a 23-task benchmark with Claude Haiku 5.5 (3 runs per task, 69 runs per setup), compared with the plain
SolidCraft MCP server:

| | Plain SolidCraft MCP (21 tools) | scx v13 (3 tools) |
|---|---:|---:|
| Parts built correctly | 69 / 69 | 69 / 69 |
| Tool calls per task | 6.8 | 2.8 (**-58%**) |
| Input tokens per task | 269k | 125k (**-54%**) |
| Cost per task | 45.7 cents | 18.6 cents (**-59%**) |
| Wall time | 100% | about 70% |

Same model, same tasks, same checker; only the MCP server changed. Edits to an existing design gained the
most: 12.6 calls and 74 cents per task on the plain MCP, 23 cents on v13.

This is a prototype for benchmarking, not a product. See [Limits](#limits) before you use it.

## How it works

### Three tools

| Tool | What it does |
|---|---|
| `run {code, new?}` | Runs a Python script. `new: true` (or a name) starts a new, empty design first. Returns anything printed, errors with line numbers and hints, and a summary of what changed. |
| `design` | A compact outline: every body with volume, bounding box and face count, grouped by part. Identical parts are folded into one line. |
| `look {view?}` | A small picture of the model. Left out with `--text-only`. |

Other SolidCraft MCP tools (`execute`, `inspect_design`, `save`, ...) are not listed, but a client that
calls one by name is forwarded to SolidCraft. Inside a script, `cmd()` reaches any SolidCraft command and
`commands('word')` looks up command ids and their parameters.

### The vocabulary

The server instructions (the text the client gives the model once) hold the whole vocabulary in under
2,000 characters:

```
box(x, y, z, at=min corner | center=(..), op=, name=, into=)
cylinder(h, d=|r=, at=base centre, axis='Z'|'X'|'Y', op=, name=)   cone(...)   sphere(...)
hole(at=point ON a face, d, depth=None (through), direction=None (into the face))
s = sketch('XY'|'XZ'|'YZ', offset=0)
  s.rect  s.rect_center  s.circle  s.slot  s.polygon  s.poly
  s.extrude(dist, op=, symmetric=False)   s.revolve(axis=, angle=360)
fillet([points on edges], r)   chamfer([points on edges], d)
hollow(t, open='top'|...)   shell([points on open faces], t)
loft([sketches], op=, name=)
combine(target, [tools], op=)   move(bodies, by=, rotate=('Z', deg), copy=False)
remove(bodies)   rename(body, new)
with part('Name'): ...
measure()   edges(body)   faces(body)
cmd('command.id', **params)   # any SolidCraft command; commands('word') finds ids
```

A whole part is usually one call:

```python
box(60, 40, 8, name='Plate')
for x, y in [(8, 8), (52, 8), (8, 32), (52, 32)]:
    hole(at=(x, y, 8), d=6)
```

The reply:

```
+ Plate: vol 18295.2213  bbox [0, 0, 0]..[60, 40, 8]  faces 10
bodies now: 1, total volume 18295.221
```

### What makes it work

Most of the gain did not come from scripting alone. It came from making the tools behave the way the model
already expects, one observed failure at a time:

- **The reply is the check.** Every `run` ends with `+` new, `~` changed (volume before -> after) and `-`
  removed bodies, tagged with their part. Models stopped calling `measure` or `design` after each step.
- **Defaults match the words people use.** `hollow(3, open='top')` instead of box-minus-box. `op='cut'`
  acts on the last body made or changed, so a cut does not slice every body it touches. `slot` takes the
  overall length, not the distance between arc centres.
- **Forgiving inputs.** The script is accepted as `code`, `script`, `command`, `source` or `python`.
  Feature objects work wherever a body name does. `import math` works.
- **Errors keep the work.** Steps before an error stay built, the reply says so, and the hint names the fix
  (for example "Block is a body name, so quote it: 'Block'").
- **Instructions under 2,048 characters.** Claude Code keeps only the first 2,048 characters of an MCP
  server's instructions. Ours were over that from v4 to v12 without our noticing; the rules at the end
  never reached the model. v13 fits.
- **Small replies.** Many identical new bodies (fins, standoffs, washers) are folded into one line, and the
  `design` outline groups identical parts.

## Setup

You need Python 3.10+ and SolidCraft 0.40 (built and tested on 0.40.0-rc). `solidcraft-cli` must be on PATH,
or set `SCX_CLI` to its path.

Against a running SolidCraft window (you watch the model work):

```bash
solidcraft --control 7879
```

```json
{ "mcpServers": { "solidcraft": { "command": "python", "args": ["-I", "/path/to/scx.py", "--port", "7879"] } } }
```

Headless, for scripts and benchmarks (scx starts its own SolidCraft through `solidcraft-cli`):

```json
{ "mcpServers": { "solidcraft": { "command": "python", "args": ["-I", "/path/to/scx.py", "--headless"] } } }
```

Add `--text-only` for clients that cannot show images. Some clients turn image blocks into text, and one
picture became about 200,000 characters of base64 that filled the model's context.

## How we tuned it

We tuned it like a small training loop, with a person in the loop and one change per version.

1. **Benchmark.** 23 tasks in four sets ([`tasks/`](tasks)): 12 simple parts (plate, L-bracket, enclosure,
   flange, stepped shaft, hex spacer, ...), 4 big ones (a 16-fin heatsink, a 16-part board on standoffs, an
   8-part rack, an 11-part bolted flange pair), 4 that test placement relative to other features (pipe tee,
   slotted cube, ...) and 3 edits to an existing rack (move a board with its standoffs, add parts, add holes).
2. **Independent checking.** Every task carries the expected bodies, worked out beforehand from a reference
   build. A task passes only if every expected body is found with volume within 0.1% and bounding box within
   0.05 mm, and nothing extra is left. The model's own claims are never trusted.
3. **Ask the model what happened.** Each run ends with a written `Thinking:` section: the plan, one line per
   tool call (what it meant and what the reply said), anything it guessed, anything confusing, and one
   **Wish**: what the tools could have done to save a call. This was the most useful signal we had. For
   example, Haiku said in almost every run that the instructions were cut off at "act on the las...". They
   were.
4. **Change one thing, only from evidence.** A change needs a trigger seen in more than one task: a repeated
   error, a repeated Wish, or a wasted call pattern. Each version records its trigger, the exact change and
   the result (see the [change log](CHANGELOG.md)).
5. **Measure with repeats.** Three runs per task. Run-to-run spread was 2-12%, so one run cannot separate a
   10% change from noise.
6. **Stopping rule.** Keep a change only if the 3-run average improves cost or tokens by at least 5% with no
   lost pass. Stop after two changes in a row miss that bar. For Haiku 5.5 that happened at v12 (a
   regression, reverted) and v13 (flat), so v13 is the final Haiku version.

What we learned:

- Gains came from matching what the model already expects (names, words, defaults), not from more
  instruction text. Instruction text is paid for on every call.
- Some well-meant changes made things worse. A "words -> calls" table (v6) cost 30% more tokens and gained no
  pass. Letting `remove()` take part names (v12) doubled edit costs, because bodies and parts shared names.
  The log of triggers made both easy to undo.
- Check what the client actually delivers, not just your source file.

## Version history

The full table with numbers is in [CHANGELOG.md](CHANGELOG.md).

| Version | Change | Result |
|---|---|---|
| v1 | One `run` tool for Python scripts, automatic body diff | 8/12 simple, half the tokens of the plain MCP |
| v2 | Script accepted under several argument names; `slot` takes overall length | 19/20 |
| v3 | `remove()`, last expression printed, identical bodies grouped | 21/23 |
| v4 | `hollow()`, compact `design` outline | 20/23, fewer tokens |
| v5 | cut/join act on the last body; feature objects as names | **23/23**, best of the Haiku 4.5 rounds |
| v6 | "words -> calls" table, cylinder start/end | +30% tokens, no gain: reverted |
| v7 | Error and naming fixes | slips elsewhere: tuning paused |
| v8 | Headless mode, pass-through tools | plumbing for other benches |
| v9 | `run(new=true)` starts a new design | Haiku 5.5: calls 3.5 -> 2.5, input -24% |
| v10 | Replies tag each body with its part | flat; edits a little better |
| v11 | `import math`, `measure(feature)` | two repeated errors gone |
| v12 | `remove()` takes parts, `move(part=)` | edits cost doubled: reverted |
| v13 | Instructions rewritten under 2,048 characters | complaints gone, steadier edits: **final for Haiku** |
| v14 | `--text-only`, hint for unquoted body names | fixes text-only clients |
| v15 | `loft()`, `move(rotate=)`, `commands()` lookup, sketch fix | ladder L7: 41% cheaper than the plain MCP, same passes |
| v15.1 | bodies SolidCraft cannot measure no longer break every call | found on L8 (imported toolhead). Shipped as `scx.py` |

## Difficulty ladder

After tuning stopped, we checked whether scx only saves money or also lets the model do harder work. New
tasks, written before any run and never used for tuning, in levels of rising difficulty. Same checker, 3 runs
per task, plain SolidCraft MCP against scx. L1-L6 used v14; v15 was made from what L5 and L6 showed, then
tested on L7. L8 and L9 used v15.1. L8 was repeated on Sonnet 5 and Opus 5.5; L9 ran on Sonnet 5.5.

| Level | What it adds | Plain MCP | scx | Passes |
|---|---|---:|---:|---|
| L1 | several features on one part | 46¢ | 20¢ | all |
| L2 | placement relative to other features | 55¢ | 34¢ | all |
| L3 | edits to an existing design | 53¢ | 28¢ | all |
| L4 | revolve, sweep, loft, fillet chains | 68¢ | 44¢ | all |
| L5 | assemblies with lofts and patterns (duct, rotor, gearbox) | 56¢ | 44¢ | gearbox 2/3 in both, rest all |
| L6 | 4 change requests in one conversation on a growing part | 91¢ | 64¢ | every step |
| L7 | 4-5 harder change requests (bent duct, handle, impeller), v15 | 135¢ | 80¢ | every step |
| L8 | a hotend mount inside a real imported toolhead, 6 steps | 164¢ | 68¢ | plain 11/18, scx 13/18 |
| L8 (Sonnet 5) | the same six steps, on a larger model | 407¢ | 177¢ | plain 17/18, scx 18/18 |
| L8 (Opus 5.5) | the same six steps, on the largest model | 209¢ | 96¢ | 18/18 in both |
| L9 (Sonnet 5.5) | rebuild a freeform vessel from a mesh, 3 chained steps | 222¢ | 130¢ | scored by overlap: plain 0.896, scx 0.877 (median) |

Cost is per task (per conversation for L6 and L7, per set of 6 steps for L8, per set of 3 steps for L9).

What it shows:

- **No ceiling up to L7.** Haiku 5.5 passed nearly everything with either server up to L7. scx did not let
  it do harder work; it did the same work for less.
- **L8 found the first ceiling, and it is the model's.** Both servers failed the same step in all 6 runs
  (see below). scx passed two more steps overall and cost 59% less.
- **scx was cheaper at every level**, by 20-57%. The two places v14 cost more than the plain MCP were missing
  words: the L5 rotor (no `loft` or rotate; Haiku spent 9-12 calls guessing names) and an L6 hole move (no
  way to look up a command). v15 added `loft`, `move(rotate=)` and `commands()`; on L7 those were used in
  40 calls and the guessing stopped.
- **The saving held on every model.** L8 cost 59% less with scx on Haiku 5.5, 57% less on Sonnet 5 and
  54% less on Opus 5.5; L9 cost 41% less on Sonnet 5.5.
- **Fewer calls is not the point; tokens are.** Some scx runs made as many calls as the plain MCP and were
  still cheaper, because each reply is short.

### L8: a hotend mount in an imported toolhead

The design is the MultiPlex toolhead, an open-source modular toolhead by Gabe, exported from Fusion as STEP:
23 bodies (toolhead, fans, blowers, an E3D V6 with its finished mount as a worked example, a loose MK8 hotend
of 11 parts, and a blank mount block). Six steps turn the blank block into an MK8 mount. Each is a separate
run that starts from a reference solution of the step before, so one miss does not sink the rest:

1. move the MK8 so its nozzle tip and axis match the V6's;
2. cut a pocket so no mount material is inside any MK8 part's bounding box;
3. keep 3 mm of air around the heater block, sock, nozzle and its screws;
4. a 4-4.5 mm bowden channel from the top down to the PTFE clip;
5. an air inlet from the fan face to the heatsink and an exhaust vent out the back;
6. two M3 holes lined up with the heatsink's cross holes, to clamp it.

The checker adds probe solids (the regions that must stay empty, or must keep material) and measures their
overlap with the mount. Every step also checks that the provided bodies are unchanged, no bodies were
added and the mount is still one solid inside its block.

| Step | Plain MCP: passes, calls, cost | scx: passes, calls, cost |
|---|---|---|
| 1 align | 3/3, 6.7, 18¢ | 2/3, 10.7, 13¢ |
| 2 pocket | 2/3, 23.7, 28¢ | 3/3, 9.7, 9¢ |
| 3 hot zone | 3/3, 26.0, 32¢ | 3/3, 7.3, 8¢ |
| 4 bowden | 1/3, 33.7, 35¢ | 3/3, 12.3, 10¢ |
| 5 airflow | 2/3, 12.0, 16¢ | 2/3, 12.3, 12¢ |
| 6 retention | 0/3, 35.3, 35¢ | 0/3, 17.0, 16¢ |
| **Total** | **11/18, 164¢** | **13/18, 68¢** |

Calls and cost are means per run. Input tokens per set of six steps: 6.65M on the plain MCP, 2.31M on scx.

- **Step 6 is the ceiling.** No run found the heatsink's cross holes. All six placed the screw holes by
  eye, 1-3 mm off (one put both on the centre line), and reported success. Finding a feature on an existing
  part, not building one, is where Haiku 5.5 stopped, and the server made no difference to that.
- **The plain MCP spent its money on the middle steps.** Steps 2-4 took 24-34 calls on the plain MCP and
  7-12 on scx; one plain run of step 4 gave up after the kernel refused a cut that overlapped the existing
  hole exactly.
- **Real files break things.** The exported blank block had edges shorter than 0.005 mm and the kernel
  refused every cut on it, so it was rebuilt with the same shape before the test. One imported screw
  cannot be measured at all; that crashed scx v15 on every call (fixed in v15.1).

#### The same six steps on Sonnet 5

L8 was then run again, unchanged, on Claude Sonnet 5 (3 runs per step per server).

| Step | Plain MCP: passes, calls, cost | scx: passes, calls, cost |
|---|---|---|
| 1 align | 3/3, 14.7, 60¢ | 3/3, 10.0, 36¢ |
| 2 pocket | 3/3, 10.0, 47¢ | 3/3, 5.7, 21¢ |
| 3 hot zone | 3/3, 18.7, 63¢ | 3/3, 7.3, 20¢ |
| 4 bowden | 2/3, 18.3, 73¢ | 3/3, 11.0, 37¢ |
| 5 airflow | 3/3, 23.3, 56¢ | 3/3, 9.0, 23¢ |
| 6 retention | 3/3, 29.0, 109¢ | 3/3, 11.7, 39¢ |
| **Total** | **17/18, 407¢** | **18/18, 177¢** |

Input tokens per set of six steps: 5.67M on the plain MCP, 2.11M on scx.

- **The bigger model cleared the ceiling.** Step 6 went from 0/6 on Haiku to 6/6 on Sonnet 5: it measured
  the heatsink's cross holes instead of guessing, and every run put the screw holes within 0.01 mm of
  y ±7, z 47.555. Finding features on existing geometry is a model capability, and no server change
  substituted for it.
- **The saving did not shrink with model size.** scx cost 57% less on Sonnet 5 against 59% less on
  Haiku 5.5. If the plain MCP's cost came from a weaker model mishandling a wide tool surface, a stronger
  model should have narrowed the gap. It did not, which points at the interface rather than the driver:
  the plain MCP's cost is in how much conversation each call carries, not in how well the model aims it.
- **The one miss was marginal.** A plain-MCP bowden run stopped its channel 0.3 mm above the PTFE clip;
  the diameter was right and the step otherwise correct.

Two scx step-6 runs were recorded as failures by the first version of the checker because the kernel
refused to evaluate the clearance pair. Measuring those two bodies directly showed holes identical to the
runs that passed, so they are counted as passes; the checker now reports an unevaluable probe as
unverified rather than failed.

#### The same six steps on Opus 5.5

L8 again, unchanged except for one line (see the note below), on Claude Opus 5.5 (3 runs per step per server).

| Step | Plain MCP: passes, calls, cost | scx: passes, calls, cost |
|---|---|---|
| 1 align | 3/3, 8.7, 38¢ | 3/3, 5.0, 18¢ |
| 2 pocket | 3/3, 5.7, 31¢ | 3/3, 4.3, 12¢ |
| 3 hot zone | 3/3, 5.7, 24¢ | 3/3, 6.0, 16¢ |
| 4 bowden | 3/3, 8.0, 53¢ | 3/3, 3.7, 13¢ |
| 5 airflow | 3/3, 7.3, 35¢ | 3/3, 5.0, 16¢ |
| 6 retention | 3/3, 7.3, 27¢ | 3/3, 4.7, 20¢ |
| **Total** | **18/18, 209¢** | **18/18, 96¢** |

Input tokens per set of six steps: 1.74M on the plain MCP, 0.97M on scx.

- **Every run passed, and Opus was the cheapest model on this test.** It cost about half what Sonnet 5 did
  on either server, because it needed a third of the calls: 43 per set on the plain MCP and 29 on scx,
  against 114 and 55 on Sonnet 5.
- **scx still cost 54% less.** The gap narrowed by a few points from Haiku and Sonnet 5, but the plain MCP
  still carried about 1.8 times the input tokens for the same work.

Note: earlier rounds ended each prompt with a request for a short "Thinking:" report of the plan and of each
call. On the 5.5 models that request tripped a safety filter at the end of most runs, so the Opus 5.5 and
Sonnet 5.5 runs leave it out; the flagged runs were thrown away and rerun. The report was never scored, but
it added some output tokens to the Haiku and Sonnet 5 rounds.

### L9: rebuild a freeform vessel from a mesh (Sonnet 5.5)

L1-L8 can be checked pass or fail. L9 can only be scored by how close the model gets. The target is a
decorative candle vessel Gabe modelled in Fusion from revolves, side-profile cuts, heavy fillets and a lofted
relief: 132.5 mm tall, hollow with 12-15 mm walls and a flat floor, a rim that rises on one side, and a
relief of folds a few mm deep on the -X face. The models get only its STL, as a mesh body in the design, and
must rebuild it as one editable solid in three chained steps (each starts from that run's own previous
result):

1. the solid outer form: foot, bulge, waist, flared top, the shaped top edge and the large blends;
2. the cavity, floor, open top and rounded lip;
3. the lofted relief on the -X face.

SolidCraft cannot turn this mesh into a solid for them (`mesh.convert` only handles planar and cylindrical
meshes), and the models had no file access, so there was no shortcut. The scorer exports the solid and
compares it with the original mesh on a 0.5 mm voxel grid (intersection over union; step 1 against the
mesh with its cavity filled), plus the mean distance between the two surfaces. For scale: a plain
cylinder of the right height scores 0.757 on step 1 and 0.327 hollow; a perfect revolve of the true average
profile scores 0.944 and 0.898.

| Step | Plain MCP: overlap, calls, cost | scx: overlap, calls, cost |
|---|---|---|
| 1 outer form | 0.869, 16.7, 71¢ | 0.887, 15.3, 41¢ |
| 2 cavity and lip | 0.891, 18.3, 69¢ | 0.739, 21.3, 51¢ |
| 3 relief | 0.882, 24.0, 82¢ | 0.738, 14.7, 38¢ |
| **Set of 3** | **222¢; final 0.904, 0.896, 0.846** | **130¢; final 0.893, 0.877, 0.444** |

Overlap, calls and cost are means per run; the last row gives each run's final overlap. Final mean surface
distance: 0.92-1.39 mm on the plain MCP, 0.77-1.11 mm on scx (3.4 mm for the failed run).

- **scx cost 41% less**, the smallest saving on the ladder, with a similar number of calls: this test was
  measuring and thinking, not issuing commands.
- **Accuracy was a draw, apart from one bad step.** The best outer form (0.920) and the closest final
  surfaces (0.77 mm) were scx runs. One scx run cut a cavity from guessed radii, left the walls far too
  thin and finished at 0.444; the plain MCP had no failure that large.
- **No run got far past a plain revolve.** The best final overlap, 0.904, is about what a perfect revolve of the
  average profile gets (0.898). The rising rim and the slightly oval body were approximated at best.
- **Step 3 is the ceiling.** No run reproduced the relief: overlap in its region moved by -0.14 to +0.03.
  The runs built a single raised loft or ridge from estimated numbers and said so. The models could take
  sections of the mesh, but many could not read usable coordinates back from them, and none used
  `inspect.compare` to check its solid against the mesh. Measuring freeform geometry is where Sonnet 5.5
  stopped, much as finding the heatsink holes stopped Haiku on L8.

Two runs were thrown away and rerun: in one the model invoked an unrelated built-in Claude Code skill
(`/init`) instead of working, and in the other a skill launched and loaded its instructions. The harness now
blocks skills. The STEP export of the vessel was not usable as a scoring reference: SolidCraft imports it
only as surfaces, because two of its faces have zero-area trim loops.

## Limits

- **Not a sandbox.** Scripts run in the server's own Python process with restricted builtins. That keeps
  honest models on track; it does not stop a hostile script. Do not point it at untrusted prompts.
- In Claude Code the three tools load deferred, so each session spends one ToolSearch call before the first
  `run`. That is a client setting, not the server.
- Known issue: `hollow(open='top')` picks the face at the centre of the top of the bounding box, so it fails
  when something sticks up from that face.
- Tuned on Claude Haiku 5.5 only. Larger models were measured on L8 (Sonnet 5, Opus 5.5) and L9
  (Sonnet 5.5), not tuned for.
- `measure()` and replies show `bbox None..None` for a body SolidCraft cannot measure.

## License

Like SolidCraft, dual-licensed under [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE), at your
option.

## Credits

Built by Gabe ([@dudedagger42](https://github.com/dudedagger42)) with Claude, on top of SolidCraft's MCP
bridge.
