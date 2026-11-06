# Git dependencies for cloud saves

The native repository adapter uses installed Git for transport, worktrees,
credential helpers, SSH agents, hooks and signing. Snapshot storage and the durable
outbox have independent interfaces; the browser implements publication through
the GitHub API.

Git is invoked as argument vectors through a bounded process adapter with a
configurable executable, connection-specific environment and cancellation.
Git's documented [SSH override](https://git-scm.com/docs/git#Documentation/git.txt-GIT_SSH_COMMAND),
[credential helpers](https://git-scm.com/docs/gitcredentials) and
[commit signing](https://git-scm.com/docs/git-commit) define the compatibility target.

## Reuse shell quoting

`shlex 2.0.1` is MIT OR Apache-2.0. Its fallible `try_join`/`try_quote` APIs handle
POSIX shell argument quoting and reject NUL. Use that for `GIT_SSH_COMMAND`
instead of maintaining a custom quoting function. Keep normal Git invocations
as argument vectors, with no shell. POSIX quoting applies to Git's documented
shell-interpreted SSH override; it is not a general Windows `cmd.exe` or
PowerShell quoting API. Validate inputs and test actual Git for Windows before
claiming cross-platform special-character coverage.
[shlex release](https://crates.io/api/v1/crates/shlex),
[try_join](https://docs.rs/shlex/2.0.1/shlex/fn.try_join.html),
[quoting warning](https://github.com/comex/rust-shlex/blob/master/src/quoting_warning.md),
[Git SSH command semantics](https://git-scm.com/docs/git#Documentation/git.txt-GIT_SSH_COMMAND).

## Native subprocess lifetime

Use [`process-wrap` 10.0.1](https://docs.rs/process-wrap/10.0.1/) with the
`std`, `process-group`, and `job-object` features. Its Apache-2.0/MIT license
fits the repository rules. It wraps the configured Git executable in a Unix
process group or Windows Job Object, so cancellation can stop SSH, hooks, and
credential helpers together with Git.

The adapter still owns bounded concurrent pipe reads and its deadline; the
library's unbounded output collection is unsuitable for hostile repository
output. Linux probes passed for simultaneous stdin/stdout/stderr, oversized
stdout and stderr, cancellation with descendants holding pipes, and a deadline
with descendants holding pipes. Windows behavior follows the crate's public
Job Object API and still needs validation on Windows.

Standard-library `File::try_lock` serializes publication in an existing checkout
across application processes. The lock lives inside Git's own per-worktree
metadata and changes no shared repository files or global configuration.

## Required adapter contract

The reusable interface should express repository inspection and publication of a
captured revision, rather than expose a giant Git command API. Native Git and a
browser host API must independently provide the following application semantics:

- Inspect the destination and format before writing; preserve unrelated work.
- Publish the captured snapshot with an expected prior revision and no force.
- Keep local pending data until remote confirmation; identify an already
  published retry without duplicating its revision.
- Return conflicts, authentication failures, cancellation and disconnected state
  as typed outcomes, without silently changing configuration.
- Scope each operation to its repository connection and project folder; do not
  couple multiple configured repositories through global process state.

This contract remains SolidCraft-specific. A Git crate does not decide document
compatibility, durable-save acknowledgement or design conflict-copy policy.
