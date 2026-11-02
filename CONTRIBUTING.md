<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contributing to solidcore

Thank you for helping. solidcore is a B-rep kernel in safe, deterministic Rust. It is based on
SMLib and goes its own way: clean, procedural constructions, each built from the geometry of its
case, replacing the code they supersede. This page is the short version;
[AGENTS.md](AGENTS.md) has the full rules, which apply to people and agents alike.

## Before you start

- **Put the work in its layer.** [docs/layers.md](docs/layers.md) says where each job lives. Geometry is computed only in the kernel (`solidcore`). The modelling API (`solidcore-ops`) only chooses and sequences kernel calls. Applications compute no geometry. If a workflow needs maths the kernel lacks, add it to the kernel with tests.
- **Read the docs for the area.**
  - For fillets and chamfers, read [docs/generic-blend.md](docs/generic-blend.md): what the blend
    builds, its events, its exact classes, its open work, and how each step is ported (§9).
  - For any unit, read its `plan/units/<id>.toml`, plus its `plan/findings/` and
    `plan/upstream-issues/` entries.
- **Check what's already open.** Look at open pull requests, the issues for the area, and its
  hub issue (fillets and chamfers: [#2243](https://github.com/tangency-labs/solidcore/issues/2243)).
  Claim an issue before you start, and build on others' work rather than next to it.
- **Ask when the docs and the code can't answer.** Ask the maintainers on the hub issue and wait
  for an answer rather than guessing. A short question saves a long review.

## How we fix things

- **Root causes, not patches.** Find why the construction is wrong for the case and fix that. Don't
  add a branch for one input, a fallback, a retry, a loosened tolerance or a skipped check.
- **Check more than one issue.** Several failing cases usually share one cause. Before fixing, look
  for the other open issues and cases it explains, and fix the cause for all of them.
- **Read the papers.** Most kernel failures are known problems with published constructions.
  Searching the literature is most often the right way to solve a case; code invented to make one
  case pass is not.
- **Events, not try/catch trains.** Decide from the geometry which case you are in (a tangent end, a
  fold, a corner type) and build that one construction. Never attempt one construction, catch its
  failure and try the next.
- **Leave no code smells.** No dead code, no unused flags, no commented-out code, no magic numbers
  without their reason.
- **Tests first.** Commit the failing test before the fix, in the area's existing test file. Gate on
  values a wrong result changes (exact volumes, distances to the exact surface, counts), never on
  time.

## The new construction replaces the old

- **SMLib is the base, not the limit.** Where a construction has been redesigned (the generic
  blend), the new one is what solidcore builds, and its steps keep their results bit for bit.
- **Flags are temporary.** A port lands behind an opt-in flag only while it's in progress. Once its
  construction is ported and parity is shown, the flag becomes the default and the old code is
  deleted, together with the patches made to it.
- **Don't extend the old paths.** Improve the construction that replaces them.

## Cases and known failures

Fillet and chamfer cases from SolidCraft's kernel rig are listed in
[docs/generic-blend.md §7](docs/generic-blend.md#7-case-gallery), each with its state: passes,
refused as expected (with the size that fits), or known failing (with its reason). When your change
makes a case pass, or fixes a known failure, update its row **and its image** in the same pull
request: a new render of the result (`docs/generic-blend/cases/<case>.jpg`) whose view shows the
case's issue (the corner, the seam, the spot refused or fixed), framed close on it, not a fixed
angle. Agents: choose the view from the case's geometry. A changed row without a new image is not
accepted.

## Concurrency and parallelism

- **The concurrency unit is the `Kernel`.** It holds no global state and can move between
  threads, so independent kernels run side by side: use one per thread or worker. Upstream SMLib
  needs a process per job instead, because its default context holds unlocked mutable state; see
  the fork's `.agents/operations/concurrency.md`.
- **Operations have parallel structure but run serially for now.** Their pieces are pure functions
  of immutable inputs, so they can be spread over threads later with the same result bit for bit
  ([docs/generic-blend.md](docs/generic-blend.md) §5, [AGENTS.md](AGENTS.md) §4c).
- **Never add global, `static` or `thread_local` state,** or a cache that fills lazily without
  synchronisation.

## Pull requests

- **One topic per pull request,** with a closing keyword (`Fixes #n`) where it fixes an issue.
- **Every commit builds,** and the tests it touches pass, or are listed known failing with their
  reason.
- **Show the evidence:** the commands you ran and their results
  ([docs/completion-evidence.md](docs/completion-evidence.md)).
