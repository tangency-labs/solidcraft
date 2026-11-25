---
name: solidcraft-modernize
description: "Use when bringing an old SolidCraft pull request up to date: rebase it, check that its idea still fits current main, and decide honestly whether to keep it, rework it or close it."
---

# Bringing an old pull request up to date

Adapted from tstack's modernize (https://github.com/t3dotgg/tstack, MIT; see `.agents/skills/LICENSE-tstack`).

Modernize the idea, not just the branch. `main` changes many times a day. A clean rebase doesn't prove the PR still belongs.

## Guardrails
- Fetch first: `git fetch origin main` and the PR head (`git fetch origin pull/<N>/head:pr-<N>`). Work in a separate worktree if your checkout has other work.
- Your own branch: rebase it, and push with `--force-with-lease=<branch>:<the head you saw>`. If the lease fails, fetch and look again.
- Someone else's branch: never force-push it. Open a replacement with `.agents/skills/solidcraft-takeover/SKILL.md`.
- Don't push only to catch up with `main`. Rebase when the PR conflicts, or when you push a real change.
- Modernizing never means merging.
- To search many PRs, fetch their refs once (`git fetch origin 'refs/pull/*/head:refs/remotes/pr/*'`) and use `git grep` or `git log -S`. Don't make one API call per PR.

## The PR's intent
Record its link, author, base, head commit, checks, labels and review state. Read the body, the linked issues, the threads and the full diff. Write the goal it was reaching for in one sentence.

## What changed around it
Read what landed on `main` since the PR branched, and the open or merged PRs on the same files or issue.
- Is the user's problem already solved on `main`?
- Did the code it changes move, get deleted or move into `solidcore/`? Geometry has one implementation in solidcore (`.agents/skills/solidcraft-kernel-rules/SKILL.md`). A PR that adds a second one, or a new off-by-default fix flag, has the wrong shape now.
- Would it bring back old behaviour, or add a second way to do the same thing?

## Choose before you push
- **MODERNIZE:** still useful and the right shape. Rebase, resolve each conflict by the PR's intent and `main`'s newer rules, compare with `git range-diff`, run `cargo xtask fast`, push.
- **REWORK:** the goal is useful, the shape is wrong now. Describe the smallest rewrite, and stop unless you were asked to do it.
- **CLOSE:** superseded, obsolete or wrong. Name the evidence: the PR or commit on `main` that solved it, the code that moved.

## The stale-PR rule
A PR older than 7 days may be closed when one of these holds: nobody pushed or commented for 7 days; it conflicts, or fails with a real regression, and its last commit is over 3 days old; or it is superseded. Never close a PR for its age alone. PRs on hold (`status:decision`, a `hold-` label, `needs-ui-review`, `owner-approved`) and ready, green PRs with no conflict are exempt. The note is friendly, names what the PR did well and what changed, and ends with "a fresh PR on the latest main is welcome".

## Report
Lead with MODERNIZE, REWORK or CLOSE and why. Then what changed on `main`, the conflicts found (or none), the push result, the checks and their results, and the PR's title and full link. Keep facts apart from judgment.
