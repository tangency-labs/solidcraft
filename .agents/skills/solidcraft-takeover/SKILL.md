---
name: solidcraft-takeover
description: "Use when finishing someone else's SolidCraft pull request, branch or stalled claim: when you may take it, how to keep the original author's credit, and what happens to the old PR."
---

# Taking over a pull request

Adapted from tstack's takeover (https://github.com/t3dotgg/tstack, MIT; see `.agents/skills/LICENSE-tstack`).

You take on the original goal, not the original code. Don't defend an implementation because someone already wrote it.

## May you take it?
- A draft claim with no pushes for 72 hours may be taken over after a comment on it (`.agents/skills/solidcraft-contributing/SKILL.md`). A claim with recent pushes is not yours: leave it.
- A PR a maintainer has put on hold (`status:decision`, a `hold-` label) stays as it is.
- If the author answers your comment and wants to finish, let them.

## Read it all
The body, every comment and review thread, linked issues, the commits and the full diff. Find the live head. A summary of the work is a lead, not proof: check it against the code.

## Continue
- Start a new branch from the latest `origin/main`. Cherry-pick the original commits, so their author stays on them. Put your own changes in separate commits.
- Keep the credit: keep their commits' authors and any `Co-authored-by` trailers. On your commits, add `Co-authored-by: <Name> <id+login@users.noreply.github.com>`. Take the id from `gh api users/<login> --jq .id`: a verified address, never a guessed one. A squash merge keeps these trailers.
- Credit them in the PR body: "This continues @login's work in <full link>."
- Check the idea against current `main` with `.agents/skills/solidcraft-modernize/SKILL.md`. File with `.agents/skills/solidcraft-file-pr/SKILL.md`, and watch it with `.agents/skills/solidcraft-babysit-pr/SKILL.md`.
- Look at the original PR again before you file: its author may have pushed in the meantime.

## The original PR
- Comment on it with a link to the new PR. If it is your own claim, close it.
- Close someone else's PR only if you have the rights and it meets the stale-PR rule (`.agents/skills/solidcraft-modernize/SKILL.md`). Otherwise the maintainers close it.
- The note is friendly and specific:

```markdown
Thanks for starting this. I continued it in <full link to the new PR>, with your commits kept.
A fresh PR on the latest main is always welcome.
```

## Report
What you inherited, what you changed, how you proved it, how the credit was kept, and both PRs with their titles and full links.
