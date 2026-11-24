---
name: solidcraft-full-stop
description: "Use when asked to take a SolidCraft change all the way to a pull request that is ready for review (\"full stop\"), with nobody watching: prove it, review it yourself, file it, watch it to green, and stop. Never merges."
---

# Full stop: from change to a ready pull request

Adapted from tstack's full-stop (https://github.com/t3dotgg/tstack, MIT; see `.agents/skills/LICENSE-tstack`).

Nobody is watching. You own the change until its PR is green and ready for a maintainer. Then you stop. You never merge.

## Never
- Merge, queue or auto-merge a PR (`gh pr merge` in any form), push to `main`, or bypass a branch rule.
- Add `ready to merge` or `approved-for-ci` to your own PR: maintainers add them after review.
- Ask another agent to do any of these.

## Steps
1. **Finish the change.** Read `AGENTS.md` and the skill for the area. Make the smallest change that fixes the case that happened. Claim the issue first (`.agents/skills/solidcraft-contributing/SKILL.md`).
2. **Prove it** with evidence you saw yourself: the failing case, then the same case passing; `cargo xtask fast` green; for UI or model behaviour, the app driven and a screenshot you looked at (`.agents/skills/solidcraft-agent-tools/SKILL.md`). Green mocked tests don't prove the user's experience.
3. **Review it yourself** before anyone else does. Read the whole diff against `origin/main` once more, with this list:
   - the hard rules in `AGENTS.md`: never crash, layering, commands, licences, lists;
   - the area skill's rules: `.agents/skills/solidcraft-shared-controls/SKILL.md` for controls, `.agents/skills/solidcraft-kernel-rules/SKILL.md` for geometry and tests, `.agents/skills/solidcraft-repro-first/SKILL.md` for platform fixes;
   - one topic, no stray files, and nothing the goal doesn't need. Cut what grew.
4. **File it** with `.agents/skills/solidcraft-file-pr/SKILL.md`, rebased on the latest `main`, with one line in Validation on how you proved it.
5. **Watch it** with `.agents/skills/solidcraft-babysit-pr/SKILL.md` until CI is green and the review bot has finished. Prove each fix before you push it.
6. **Stop at the gate** and report.

## The gate
All true on the live head (read it again; don't trust a late notice):
- your proof covers every change in the diff;
- `ci` passed on the newest run, and the heavy tests passed if a maintainer started them;
- the review bot finished, and each real finding is fixed or answered with a reason;
- no review asks for changes; the PR is open, not a draft, and has no conflict;
- the body has the `Changes a default:` line, and `Test change:` if an existing test line changed.

## Verdicts
- **Ready for review:** the gate passes. Leave it for a maintainer.
- **Waiting for a maintainer:** only `approved-for-ci` or a human review remains. Say that this is the blocker.
- **Blocked:** what blocks it, what you tried, and what is needed. Ask only when you can't go on: you can't prove the change, the goal is unclear in a way that changes the result, or another PR makes this one obsolete.

## Report
Lead with the verdict. Then the PR's title and full link, the final head commit, your proof, the review outcome, the choices you made, and what is still open. Say plainly that the PR is open and not merged.
