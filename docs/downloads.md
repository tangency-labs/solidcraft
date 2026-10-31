# Downloads

Every platform and package. The links point at the latest release.

- **Windows:** download and run the [installer][windows-msi].
  Also for [ARM64][windows-arm64-msi] and [32-bit][windows-x86-msi] PCs, or as a
  [portable zip][windows-portable] that runs without installing.
- **macOS:** open the [disk image][macos-dmg] and drag SolidCraft into Applications
  (Apple silicon and Intel).
- **Linux:** download the [AppImage][linux-appimage], allow it to run as a program and open it.
  Or install the [.deb][linux-deb] (Ubuntu, Debian, Mint) or [.rpm][linux-rpm] (Fedora,
  openSUSE), or use [Flatpak](https://docs.solidcraft.io/guide/getting-started.html#linux-flatpak).
  With Snap: `sudo snap install solidcraft` ([Snap Store][snap-store], or the [.snap file][linux-snap]).
  For ARM64: [AppImage][linux-arm64-appimage], [.deb][linux-arm64-deb], [.rpm][linux-arm64-rpm].
  On Arch Linux: `sudo pacman -U` the [package][linux-arch] (ARM64: [package][linux-arm64-arch]).
- **Web:** [try it in your browser][web-app], no install.


## Code signing

Windows releases will be code signed through the SignPath Foundation: free code signing provided by
[SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/). This is being set up; until it's live, the note
below applies. macOS releases are signed with an Apple Developer ID and notarized by Apple (the app
and the disk image carry the notarization ticket), so they open normally.

The Windows downloads aren't signed yet. If SmartScreen warns you, choose **More info**, then
**Run anyway**.

**On SolidCraft 0.11 to 0.14?** Its **Update now** can't install 0.15.0 or later, because the
releases moved to a new address. Download the new version once from the links above; after that,
updating from inside SolidCraft works again ([details](https://docs.solidcraft.io/guide/updates.html#updating-from-011-to-014)).

Older versions, checksums and other formats are on the [releases page](https://github.com/tangency-labs/solidcraft/releases).
Install details, upgrading from earlier builds and building from source are in

[Getting started](https://docs.solidcraft.io/guide/getting-started.html).
[windows-msi]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-windows-x64.msi
[windows-arm64-msi]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-windows-arm64.msi
[windows-x86-msi]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-windows-x86.msi
[windows-portable]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-windows-x64-portable.zip
[macos-dmg]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-macos-universal.dmg
[linux-appimage]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-x86_64.AppImage
[linux-deb]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-x86_64.deb
[linux-rpm]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-x86_64.rpm
[linux-arm64-appimage]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-aarch64.AppImage
[linux-arm64-deb]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-aarch64.deb
[linux-arm64-rpm]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-aarch64.rpm
[linux-snap]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-x86_64.snap
[linux-arch]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-x86_64.pkg.tar.zst
[linux-arm64-arch]: https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-linux-aarch64.pkg.tar.zst
[snap-store]: https://snapcraft.io/solidcraft
[web-app]: https://solidcraft.io/app
