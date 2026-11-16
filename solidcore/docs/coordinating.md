# Coordinating agents

For an agent that takes a problem, splits it, hands the pieces to subagents and answers for the
result. [AGENTS.md](../AGENTS.md) still holds for every line any of them writes; this page is
how to lead the work. Its order is the order of the work: understand, search, show, split,
inspect, report.

## 1. Understand what we are building before you plan

Read, in this order, before you write a brief:

1. [AGENTS.md](../AGENTS.md), the whole file: what to work on, the hard rules, how a pull
   request is claimed and finished.
2. [definitions-and-certificates.md](definitions-and-certificates.md), whatever the area. A
   surface is its definition, its B-spline a certified approximation of it; every construction
   keeps the invariants listed there. A fix that breaks one is wrong however good it looks.
3. [layers.md](layers.md): which layer owns the job. A plan that puts a computation in ops or
   in an application is wrong before it starts.
4. The area's reference ([generic-blend.md](generic-blend.md),
   [procedural-sweep.md](procedural-sweep.md), [ssi.md](ssi.md),
   [validity-and-tolerances.md](validity-and-tolerances.md), [booleans.md](booleans.md),
   [offset-and-shell.md](offset-and-shell.md)) and its records: the unit's
   `plan/units/<id>.toml`, its `plan/findings/` and `plan/upstream-issues/` entries. Where an
   area has no reference, read the module's `//!` header, its records and its design issue, and
   say in the plan that no reference covers it.
5. The code the problem runs through. Open the functions, not a grep of their names: a name
   tells you something exists, not what it does. The default for a textbook capability in this
   kernel is that it is already there and you are missing context.

Write down, in two or three sentences, what the correct result is and which invariant the
failing case breaks. If you cannot, you are not ready to split the work.

Every face of a result is exact (the net is the definition) or certified (the net within a
stored, proved ε of the definition); an operation that cannot build such a body refuses with a
typed error that names its cause and, where one exists, the size that builds
([definitions-and-certificates.md](definitions-and-certificates.md)). There is no fourth
outcome. A correct refusal is a success. A body that builds with a bare B-spline face standing in
for its definition, an estimated bound, or a widened tolerance is a failure, whatever its picture
looks like.

## 2. Cast a wide net

Before any agent builds anything, find everything that touches the problem:

- **Issues, open and closed**, searched by the symptom (the error code, "self-intersecting",
  "zero width"), the operation, the upstream symbol, the record id (`WR-`, `UB-`, `HANG-`), the
  file path and the case name. Read the area's hub issue.
- **Pull requests, open, draft and merged**, searched the same way, titles and bodies. A
  merged pull request that never closed its issue is common: the work may be done.
- **The records in the repository**: `plan/findings/`, `plan/upstream-issues/`, the known-failing
  kernel cases (`crates/solidcore/tests/kernel_cases/`) and the definition ledger. SMLib and the
  fork are reading material for inputs and known constructions, never a result to match.
- **Claims**: an issue's assignee, its `in progress` label, an open draft that names it. Build
  on claimed work, never next to it.
- **The same cause elsewhere.** Several failing cases with one cause are one problem. Group them
  before you split: a fix for one case that opens another means the cause is elsewhere.

Say in the issue what you found: the related issues and pull requests, and which of them your
work builds on, replaces or leaves alone.

## 3. Show the problem first

Show the person you work for what is wrong before anyone builds a fix (AGENTS.md, "Show the
problem first"): a picture of the failing case next to the correct result, and a VectorCraft
schematic when the problem is a mechanism a picture cannot show. Then read the literature and
design from it.

Look at the geometry yourself. Numbers can pass while the shape is wrong, and a picture often
ends an argument the numbers could not:

- a fillet whose width runs to zero along its length, so the round pinches to a point;
- a sharp edge that should be there and is gone, rounded over or merged into a face;
- an edge whose curve sits off its faces (`cargo run --release -p diagnostics -- FILE.smb`
  draws edges above a tolerance over everything);
- a seam crossed by a curve that was never cut there;
- a sliver face, a flipped normal, a face that folds over itself.

Choose the view from the geometry so it shows the defect, close on it, with the whole part
inset where it helps. Show before and after from the same view, drawn the same way.

A picture shows a defect; it never proves a result right. The diagnostics picture is a mesh: a
dropped definition, a sampled bound or a fit standing in for the definition looks the same on
screen. Check those in the body itself: each face's definition, its stored ε, the edge
tolerances against the body's.

## 4. Split the problem into pieces a subagent can take

A piece is one question with one answer that can be checked: one cause, one construction, one
measurement. A subagent sees only its brief, so the brief carries everything:

- **The goal**, in a sentence, and the invariant or case that decides it is met.
- **What is already known and ruled out**, so it does not redo your search.
- **Where to read**: the reference section, the functions and files, the issue.
- **What to bring back**: numbers (counts, gaps, distances, never timings), the picture, the
  commit; and a length limit on the report.
- **The boundary**: the files it may change and the ones it must not; its own worktree and
  target directory, on a base you name.
- **The literature**: tell it to search for the problem and cite what it read in its report.
- **The rules it must keep**, since it will not read AGENTS.md unless told: the licence list of
  upstream files; no `unwrap` or panic, deterministic results; no fallback, retry, per-case
  branch or compatibility flag; never loosen a tolerance or change an expected value without a
  reason shown in numbers and pictures; the layer the work lives in; that you hold the claim and
  the pull request.
- **That a refusal can be the answer**: a refusal that names its cause, and the size that builds
  where one exists, is a correct result, and the brief says so.

Independent pieces run at once; a piece that needs another's answer waits for it. Keep the
number of agents to what you can inspect: a piece you cannot check is not worth starting. Start
small and fail fast: one case and a short time limit first, the full run once the small one
holds.

## 5. Inspect everything that comes back

A subagent's report is a lead, never a fact. Before you build on it or pass it on:

- **Read the diff yourself**, every line. Look for fallbacks, retries, special cases, a
  loosened tolerance, a test made cheaper or an expected value changed without a reason: each is
  a patch over a cause it did not find (AGENTS.md §4b).
- **Run the case** it says it fixed and look at the result. Check the claim against the
  invariant, not against its own test.
- **Check the contract**
  ([definitions-and-certificates.md](definitions-and-certificates.md)): no final answer (an
  intersection, a projection, a check, a continuity) reads a net without asking for its ε; every
  bound is enclosed, never sampled or estimated; rounding is counted; the definition ledger only
  shrinks; ops and the applications compute no geometric quantity ([layers.md](layers.md)).
- **Check a changed expected value**: every test whose expected value moved is listed with its
  old and new values and the reason the new one is right. A gate tighter than rounding (about
  1e-10 at part scale) is not a gate.
- **Check the numbers add up**: counts before and after, the cases that changed and why each is
  right, nothing that passed before now failing.
- **Be rational.** Judge by evidence, not by how confident the report sounds. When two agents
  disagree, find the measurement that settles it. When you change your mind, name what changed
  it.

Send back what does not hold, with the evidence, and keep the piece open until it does.

## 6. Report

When the docs and the code do not settle a design question, ask the maintainers on the area's
hub issue or in [`#solidcore`](https://discord.gg/jNNJ39Nz3P) on the project Discord, and wait
for the answer. Never let a subagent guess.

Lead with the result and its evidence: what changed, the numbers, the picture. Then what is
open and who has it. Keep it short; the person you work for knows the system. A decision that
is theirs to make (a changed result, a design choice the docs do not settle) goes to them with
your recommendation and what it costs if it is wrong, not as a list of options.
