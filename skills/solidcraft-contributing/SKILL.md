---
name: solidcraft-contributing
description: Use when someone wants to contribute code or documentation to SolidCraft, from setting up a fork and choosing an issue through submitting a pull request and responding to review.
---

# Contribute to SolidCraft

Read [CONTRIBUTING.md](../../CONTRIBUTING.md), [AGENTS.md](../../AGENTS.md) (the router
to the other skills), the [contributor book page](../../docs/book/src/dev/contributing.md),
and the [skills index](../README.md) first. Follow their current rules; use this skill to
help a first-time contributor apply them. This skill concerns the repository,
not modelling a part through MCP.

## Rules for every PR

- **Look at what is open first.** Pull the whole list, drafts included:
  `gh pr list -R tangency-labs/solidcraft --state open --limit 500` and
  `gh issue list -R tangency-labs/solidcraft --state open --limit 500`. Look for the same issue, the same
  feature under another number, and PRs that change the files you will change
  (`gh pr list ... --json number,title,files`). If someone has it, comment on their PR or issue
  instead of opening a second one: two PRs for one change cannot both land.
- **Claim with a draft PR.** `gh pr list -R tangency-labs/solidcraft --state open --search "#N"`; if none,
  branch from the latest `origin/main`, `git commit --allow-empty -m "claim: #N"`, push, and open a
  draft PR titled `<summary> (#N)`. The lowest-numbered open PR for an issue wins; close yours if it
  lost. The bot assigns you and labels the issue `in progress`. Do not claim by editing files; comment
  `/claim` only to stake a claim before you have a branch.
- **Push daily.** A draft with no pushes for 72 hours may be taken over: comment first, then open your
  own claim (you may start from its branch). The bot's automatic cleanup deadlines in CONTRIBUTING.md
  are separate.
- **One topic and one issue per PR.** Never commit `plan/`, build directories (`target*/`) or generated
  reference data; check `git ls-files` before pushing. Run `cargo xtask ci` before every commit.
  Commit after every feature arc that builds.
- **Merge flow.** The heavy CI jobs start when a maintainer adds `approved-for-ci`; approved PRs go
  through the merge queue (`gh pr merge N -R tangency-labs/solidcraft --squash --auto`), which runs the
  full suite on each group before it lands on `main`. Never force-push `main`.
- **Do not push only to catch up with `main`** (no merge of `main`, no Update branch, no rebase for
  that alone): every push restarts the whole CI run, and `main`'s own run catches what lands in between.
  When you push anyway, or GitHub reports a conflict, rebase onto the latest `main` and push your own
  branch with `--force-with-lease`; never merge `main` into a PR. Resolve conflicts without discarding
  others' work. Do not re-run CI on an unchanged commit: re-run only a job that runner trouble
  cancelled, with "Re-run failed jobs".
- **Evidence.** For observable CAD, UI or model behaviour (kernel, geometry, assembly and backend
  changes included), the issue or PR carries real screenshots or a short recording from a small
  reproduction, preferably before and after, with steps and expected/actual behaviour. Capture it
  yourself; do not ask a human for evidence you can produce. Diagrams do not replace it. Changes with
  nothing observable need none. Use a public-safe example, check for private designs, paths and account
  details, attach the media rather than committing it.
- **Follow through.** Check the PR's merge status throughout and before handoff. Report the state you
  checked and any conflicts resolved. While the PR is open, keep monitoring it and say clearly that it
  is pending; do not claim it merged without checking.
- **Shared build machine.** Check the load before a big build (`cut -d' ' -f1 /proc/loadavg`), wait
  while it is over 24, and build with `nice`.
- **Maintainers' machines** keep a local, gitignored `plan/` folder: read `plan/STATUS.md` (current
  milestone, next task), then the task in `plan/execution-plan.md` and the relevant `plan/architecture.md`
  section. Kernel and library decisions are in `plan/adr/0001-geometry-kernel.md`. Follow the autonomous
  operation protocol in `plan/execution-plan.md` section 6; do not stop to ask unless it lists the decision
  as the owner's. Contributors without `plan/` can ignore this.
- **Update the roadmap** when a milestone task lands: `docs/roadmap-detail.md` tracks status, milestones and estimates; `ROADMAP.md` is the short user-facing roadmap.
- **Start here.** What to work on: `ROADMAP.md`, "Where help is wanted most" in CONTRIBUTING.md and the open issues labelled `help wanted`, `good first issue`, `needs-triage`; check what is already open first. Architecture: `docs/architecture.md` and "How the code is laid out" in CONTRIBUTING.md; kernel work is paused ("Kernel work is paused" in CONTRIBUTING.md). Licence rules: `deny.toml` and `cargo xtask licences --check`; only `crates/kernel` may use `truck`. When the code, docs and rules settle a question, act; ask on the issue or PR only when the answer changes what users see or what the change covers.
- **Do not open a second PR** for an issue a lower-numbered open PR already closes, and keep the `Changes a default:` line filled in: `pr-checks.yml` fails both.
- **The gate reads the working tree.** Do not switch branches while `cargo xtask ci` runs: it compiles the other branch's code and fails on a change not in the PR. Run it in the background and wait for its log.

## Working as an agent

- **The code is English. The comments are English. Always.** Identifiers, doc comments,
  inline comments, commit messages, branch names, test names, error messages and
  documentation are all in English. This is not a preference: a comment in another
  language is a defect, it is not reviewable by the people who maintain this code, and a
  reviewer rejects it. If something is not obvious, write it in English and keep it
  short. A user-visible string is not inline in the code: it goes to the message
  catalogs in `assets/locales/` (a new one to `assets/locales/pending/<slug>.json`),
  and `skills/solidcraft-ui-text/SKILL.md` says how to name and translate it.
- **Parallel work runs in parallel.** When a task splits into independent parts (several
  files, several areas, several checks that do not depend on each other), dispatch one
  subagent per part and let them run concurrently instead of walking the list one item at
  a time. Give every subagent a self-contained brief: what to change, what not to touch,
  and what to report back. Read the answers before you continue; a subagent's report is
  not a result you can assume.
- **Do not parallelize what depends on itself.** A fix and the test that proves it, a
  refactor and the callers it breaks, two edits to the same file: those stay sequential.
  One subagent per independent unit of work, and no two units may write the same file.

## 1. Check the environment

Explain what you are checking and why, in everyday language. Run:

```sh
gh --version
gh auth status
git --version
rustup show
rustc --version
cargo --version
df -h .
```

Check available memory, CPU and whether a display is available. Establish whether
this is the person's computer or a cloud sandbox; do not assume desktop access,
persistent files or enough space for a full build. Compare Rust with the current
minimum in CONTRIBUTING.md. If tools or sign-in are missing, explain the next
step and help the person set them up; never ask them to paste credentials.

Before every public action, show the exact proposed content and destination and
ask the person to approve it. This includes creating a public fork, pushing,
opening or updating a PR, uploading evidence, marking it ready and commenting.
Explain a PR as a request for maintainers to review a change. Keep check-ins short
at each step: what you found, what comes next, and what needs their decision.

## 2. Fork and clone

Explain that changes go to their own copy on GitHub. After approval, run:

```sh
gh repo fork tangency-labs/solidcraft --clone --remote
cd solidcraft
git remote -v
```

Ensure `origin` points to the person's fork. If `upstream` is absent, add it:

```sh
git remote add upstream https://github.com/tangency-labs/solidcraft.git
```

Never push to upstream. If cloning an existing fork instead, check the same remote
layout. With a clean working tree, sync local `main` before starting work:

```sh
git fetch upstream
git switch main
git merge --ff-only upstream/main
```

After approval, sync the fork with `git push origin main`. If fast-forwarding
fails, inspect the divergence and preserve existing work; do not reset it away.

## 3. Choose an unclaimed task together

Ask about their interests and show a few issues that fit the available resources.
Read the current kernel-pause notice in CONTRIBUTING.md and skip kernel work
while paused, including tasks outside `crates/kernel/` that change kernel behaviour.
Fetch the whole open list, including drafts, before narrowing it:

```sh
gh issue list -R tangency-labs/solidcraft --state open --limit 500
gh pr list -R tangency-labs/solidcraft --state open --limit 500 --json number,title,files
gh issue list -R tangency-labs/solidcraft --state open --label 'good first issue' --search '-label:kernel-paused -label:"in progress"' --limit 100
gh issue list -R tangency-labs/solidcraft --state open --label 'help wanted' --search '-label:kernel-paused -label:"in progress"' --limit 100
```

Let the person choose. Set `N` to that issue's number, then inspect its discussion,
assignee and linked PRs; check similar work under other numbers and overlapping files:

```sh
gh issue view "$N" -R tangency-labs/solidcraft --comments
gh pr list -R tangency-labs/solidcraft --state open --search "#$N"
```

If it is already claimed, choose another task with the person. Do not open a
competing PR or post a comment without their approval.

## 4. Claim with a draft PR

Explain the proposed change and show the draft title and body for approval before
publishing. Recheck availability immediately before claiming. Follow the claim
rules in CONTRIBUTING.md; substitute the chosen summary and fork owner below:

```sh
git fetch upstream
git switch -c "fix-$N" upstream/main
git commit --allow-empty -m "claim: #$N"
git push -u origin "fix-$N"
gh pr create --repo tangency-labs/solidcraft --head "FORK_OWNER:fix-$N" --base main --draft --title "SUMMARY (#$N)" --body-file /tmp/solidcraft-pr-body.md
```

Write the body file first, describing the intended fix and referencing `Fixes #N`.
The draft PR is the claim; check the bot's response and the lowest-numbered open
PR rule before editing. If another claim won, explain it and ask before closing
the duplicate. Do not try to claim by editing files.

## 5. Make and check one focused change

Explain the intended behaviour, then pick the skill for the area from AGENTS.md's table and
follow it (never crash, commands, layering, clean-room rules and tests all live there).
Put synthetic regression tests in the existing test module for the affected area;
prove a bug test fails before the fix and passes after it.

Use the affected crate and test module in commands such as:

```sh
cargo test --profile ci -p solidcraft-engine --lib -- tests::feature_inputs_are_parameters
cargo clippy -p solidcraft-engine --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask ci
```

Run `cargo xtask ci` before each implementation commit on a capable environment.
On small environments, use targeted tests and report exactly what passed and what
could not run; leave the full suite to CI rather than claiming it passed. Follow
[testing guidance](../../docs/book/src/dev/testing.md) for applicable checks.
Review the diff, keep one topic, and commit the finished change. Before an approved
push, follow "Rules for every PR" above; do not push solely to catch up
with `main`. Explain progress and ask before publishing each update.

## 6. Show the result

For visible changes, reproduce the result with a small public-safe example and
capture actual screenshots or a short recording, preferably before and after.
For model output without a display, run:

```sh
solidcraft-cli snapshot script.json --out /tmp/solidcraft-result.png
```

For UI behaviour, launch the desktop app with a free control port:

```sh
cargo run --release -p solidcraft -- --sample --control 7878
```

Follow [the control protocol](../../docs/control-protocol.md): connect to
`127.0.0.1:7878`, read the launch token from its documented location, and send JSON
lines. Put the real token in the first request's top-level `token` field:

```json
{"id":1,"token":"TOKEN","method":"ui.inspect","params":{}}
{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/solidcraft-result.png"}}
```

Use `engine.execute` for commands and `ui.start` plus `ui.click` for interactive
reproductions. Open and inspect the image. Check for private designs, paths and
credentials before sharing; never expose the token. Show the person the media
and ask before attaching it to the PR or linking an accessible recording. Include
brief reproduction steps and expected/actual behaviour; do not commit generated
review media. Documentation-only changes need no screenshot.

## 7. Finish and respond to review

Prepare the final title and body: `Fixes #N`, the problem, resulting behaviour,
validation and any evidence or limitations. Show them to the person before
updating the PR. After their approval, mark the draft ready:

```sh
gh pr ready PR_NUMBER -R tangency-labs/solidcraft
```

Explain that a maintainer reviews the change and adds `approved-for-ci` to start
heavy checks; do not add that label yourself. Monitor checks and merge status,
address review comments, rerun affected checks for fixes, and ask before public
replies or pushes. Report whether it merged or remains pending; do not promise a
merge or rerun an unchanged failing check without evidence of infrastructure trouble.

## Cloud sandbox

Without a display, use `solidcraft-cli snapshot` for model evidence or launch the
app under `Xvfb :99` with `DISPLAY=:99` for UI evidence. With limited disk or CPU,
prefer a documentation or small isolated task, targeted tests and existing build
artifacts; explain any missing checks. Preserve work before temporary storage
expires and ask before publishing it. Always push only to the person's fork,
never to upstream.
