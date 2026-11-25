# Security policy

## Supported versions

SolidCraft is young and moves quickly, so only the latest release gets security fixes. Please
update to the [latest release](https://github.com/tangency-labs/solidcraft/releases/latest) before
reporting, and check whether the problem is still on `main`.

| Version | Supported |
| --- | --- |
| Latest release | ✅ |
| Nightly builds | ✅ (fixes land here first) |
| Older releases | ❌ |

## Reporting a vulnerability

Please **don't open a public issue** for a security problem. Report it privately through GitHub:
open the repository's [Security tab](https://github.com/tangency-labs/solidcraft/security) and choose
**Report a vulnerability**
([direct link](https://github.com/tangency-labs/solidcraft/security/advisories/new)).

Please include what you found, the steps you ran to reproduce it (a design file, command or MCP
request), the version and platform, and what an attacker gains. I'll acknowledge the report as soon
as I can, keep you updated in the advisory, and credit you when the fix is published unless you'd
rather stay anonymous.

## What counts as a security issue

Advisories are for problems where someone gets something they shouldn't, without already having
access to your computer or your account. For example:

- opening a design, STEP, STL or 3MF file from someone else runs code, or reads or overwrites files
  it shouldn't
- a website or another computer can reach or control SolidCraft
- an agent gets past a prompt that promised to ask you first
- the web app reaches beyond the page's own storage
- a release package or an update can be tampered with
- an API key or token gets sent somewhere it shouldn't go

## What to report as a normal bug

Please [open an issue](https://github.com/tangency-labs/solidcraft/issues/new) for these. They're
still worth fixing, but they aren't security issues:

- anything that needs a program already running on your computer, or the control token. A program
  running as you can already do what you can.
- MCP's allowed folders. They guard against accidents, they aren't a security boundary, and an agent
  with a shell of its own can already reach those files. Getting around them isn't a vulnerability.
- crashes, hangs, or heavy memory or CPU use, unless they also lead to something in the list above
- anything that only happens in an unusual setup you'd have to choose on purpose

## Before you send it

- **Run it.** Reproduce the problem on the latest release. If you only read the code, say so, and
  expect it to wait.
- **Check it yourself.** If a scanner or an AI agent found it, read the code and confirm it first.
  These tools tend to overrate what they find.
- **Say what's gained.** Name what the attacker needs and what they get. I set the final severity.

Reports that don't fit will be closed with a short note, or moved to the issue tracker.
