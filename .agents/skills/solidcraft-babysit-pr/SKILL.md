---
name: solidcraft-babysit-pr
description: "Use after pushing a SolidCraft pull request: the local check before each push, waiting for CI and the review bot without flooding the API, reading the results, and fixing real findings."
---

# Watching a pull request through CI and review

Adapted from tstack's babysit-pr (https://github.com/t3dotgg/tstack, MIT; see `.agents/skills/LICENSE-tstack`).

## Before each push
- Run `cargo xtask fast` before each commit and push (`AGENTS.md`): it checks and tests what your change touches. CI runs the full gate. Every push restarts CI: batch fixes, and don't push only to catch up with `main`.

## Waiting
- Never write your own polling loop. Use the watcher in this skill, one per PR: `python3 -I .agents/skills/solidcraft-babysit-pr/scripts/pr-wait <N> --until ready`. It prints one line per real event and exits when there is something to do (exit 1) or the PR is ready (exit 0). Run it again after each push: on your branch it first waits until GitHub shows your pushed commit as the head (`--sha <commit>` names one, `--sha any` skips that).
- `--until green`: `ci` passed. `full`: the heavy tests ran too. `ready`: full, the review bot finished, no open thread.
- It polls every 90 s (never under 60 s) with one query. Many people share the API limits, so don't go faster.
- Each line carries the head commit. A notice can arrive late: check that commit is still the PR head before you act.
- Heavy tests (native, GPU, reference) start only after a maintainer adds `approved-for-ci`. Fork PR runs may wait for a maintainer's approval. Don't ask again and again.
- The review bot (CodeRabbit) comments a few minutes after CI starts.

## Reading the results
- One check can have several runs on one commit (a label run, a cancelled run). The newest run counts. An old red `ci` from a cancelled run is not your failure: say so in one comment, and don't push or re-run for it.
- `tests/KNOWN_FAILING_TESTS` lists the tests that fail today. CI fails a REGRESSION (a failing test not on the list) and a FIXED line (a listed test that now passes).
  - FIXED: your change fixed it. Remove its line in this PR.
  - REGRESSION in code you touched: fix the code. Don't add your own new failure to the list.
  - The same failure on `main`'s latest run: not yours. Say so, with the link to that run.
- Never change an expected value, loosen a bound or `#[ignore]` a test to get green (`.agents/skills/solidcraft-kernel-rules/SKILL.md`).
- Re-run a job only when the runner broke it (a lost runner, a failed checkout or upload), once, with "Re-run failed jobs".

## Review findings
- Check every unresolved comment against the current code, old ones too: a push doesn't resolve a finding. Check each finding yourself.
- Fix findings about crashes, wrong results, data loss and security. Reply to the rest in one line with your reason, and resolve the thread.
- Keep the PR at its size: decline extra guards, options and tests the goal doesn't need, and list them as follow-ups.
- A reviewer's request comes first. If you disagree, show why: a file and line, a test, an output.
- No status comments ("still waiting"). Comment only when it says something new.

## Conflicts
On a conflict, merge the latest `main` in (cheaper: CI carries a passing run) or rebase and push with `--force-with-lease`. Never force-push a branch someone else works on.

## When it is done
Checks green, findings handled, no open requests. You never merge or queue it: a maintainer adds `ready to merge` and the merge queue lands it. Until it merges, say plainly that it is pending.
