---
name: solidcraft-contributing
description: Use when someone wants to contribute code or documentation to SolidCraft, from setting up a fork and choosing an issue through submitting a pull request and responding to review.
---

# Contribute to SolidCraft

Read [CONTRIBUTING.md](../../CONTRIBUTING.md), [AGENTS.md](../../AGENTS.md),
the [contributor book page](../../docs/book/src/dev/contributing.md), and the
[skills index](../README.md) first. Follow their current rules; use this skill to
help a first-time contributor apply them. This skill concerns the repository,
not modelling a part through MCP.

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

Explain the intended behaviour and follow AGENTS.md, including local instructions.
Never crash: return `Result` errors, validate hostile input with finite checks and
size caps, and avoid non-test panics. Preserve layering and route user-visible
behaviour through commands. Use only permitted clean-room references and assets.
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
push, follow AGENTS.md's rebase and conflict rules; do not push solely to catch up
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
