# Code signing and privacy

## What is signed

SolidCraft is built from the public source in
[tangency-labs/solidcraft](https://github.com/tangency-labs/solidcraft) by the release workflow,
and published by Tangency Labs.

- **macOS:** the app is signed with Tangency Labs' Apple Developer ID and notarized by Apple.
- **Windows:** the app (`solidcraft.exe`), the command-line tool (`solidcraft-cli.exe`) and the
  MSI installer are unsigned for now. They will be signed by Tangency Labs through Azure Artifact
  Signing once its validation is complete; this page will say so when it happens.
- **Linux and the other packages:** unsigned. Each release lists the SHA-256 of every file in
  `SHA256SUMS.txt`.

Only files built by the release workflow from this repository are ever signed.

## Who maintains it

SolidCraft is maintained by [Brennen Herbruck](https://github.com/bherbruck) at Tangency Labs.
Contributions from others land through pull requests that are reviewed before they merge, and
every release is approved by hand before it is published. Multi-factor authentication is required
on the accounts that publish releases.

## Privacy

SolidCraft sends nothing about you or your designs unless you ask it to. The one exception, which
you can turn off, is the update check: it asks GitHub whether a newer release exists, at start
and once a day. See [Updates and privacy](guide/updates.md) for what it sends and how to turn it
off. Features that reach the network otherwise, such as sending a problem report or the Assistant
with an online model, do so only when you set them up and use them.
