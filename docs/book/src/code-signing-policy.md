# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

## What is signed

The Windows downloads of SolidCraft built from this repository's release workflow: the app
(`solidcraft.exe`), the command-line tool (`solidcraft-cli.exe`) and the MSI installer. They are
built from the public source in [tangency-labs/solidcraft](https://github.com/tangency-labs/solidcraft)
on GitHub-hosted runners, and SignPath verifies that each signed file comes from that build
before it signs it. Nothing built elsewhere, and no third-party file, is signed under this
certificate.

The macOS app is signed separately with an Apple Developer ID and notarized by Apple.

## Team roles

SolidCraft is maintained by [Brennen Herbruck](https://github.com/bherbruck), who holds every role:

- **Committers and reviewers:** [Brennen Herbruck](https://github.com/bherbruck). Contributions
  from others land only through pull requests that he reviews and merges.
- **Approvers:** [Brennen Herbruck](https://github.com/bherbruck). Every release signing request
  is approved by hand before it is signed.

Multi-factor authentication is required on the GitHub and SignPath accounts that hold these roles.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it.

The one exception you can turn off is the update check, which asks GitHub whether a newer release
exists and sends nothing about you or your designs. See
[Updates and privacy](guide/updates.md) for what it sends and how to turn it off. Features that
reach the network, such as the Assistant with an online model, do so only when you set them up
and use them.
