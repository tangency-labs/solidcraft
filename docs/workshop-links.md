# Workshop links (`solidcraft://`)

Every item page on the Workshop (<https://workshop.solidcraft.io>) has an **Open in SolidCraft**
button. It is a plain link the browser hands to the desktop app:

```
solidcraft://workshop/<kind>/<owner>/<name>[@<major.minor.patch>]
solidcraft://workshop/<kind>/<id>[@<major.minor.patch>]      (the first format, still read)
```

`kind` is `model`, `plugin` or `part` (`library` also means a parts library). The full format is
in the Workshop's `docs/link-scheme.md`; the app's parser is `crates/ui-egui/src/workshop/link.rs`.

## What the app does

1. **Parses the link strictly.** Anything that isn't exactly a Workshop link (a query, a fragment,
   percent escapes, another host, upper case, a pre-release version) is refused with a message.
   The link names an item only; it never carries a file or an address to fetch.
2. **Reads the signed index** from `https://workshop.solidcraft.io/api/index.json` (no redirects)
   and checks its Ed25519 signature from the same response's `X-Workshop-Signature` header against
   the key built into the app (key id `5247b5af3f50dbc4`). An index that doesn't verify, names
   another key, has a lower `sequence` than one seen before, or has expired is refused, and
   nothing opens.
3. **Finds the item.** Delisted items and versions revoked with `block` are refused with the
   Workshop's reason; a `warn` revocation is shown in the question. Without a version, the newest
   version this SolidCraft can run is offered.
4. **Always asks first**, showing the name, kind, author, tier (Official, Community, Unverified),
   version, download size, licence and, for a plugin, the permissions it asks for. Nothing
   downloads, opens or installs before Open or Install is chosen.
5. **Downloads the one file the index pins** (from the Workshop, or a parts library's GitHub
   release) and checks its sha256 before using it.
   - A **model** opens read-only, as File › Open would open it, so Save asks where to keep your
     copy. A design from a newer SolidCraft opens with the usual warning.
   - A **plugin** goes to Install plugin…'s review, which asks you to approve its permissions as it
     does for any plugin `.zip`.
   - A **parts library** unpacks into the data folder (`parts-libraries/<id>/<version>`) and the
     Parts Library panel shows it. A library zip with links, paths outside its folder, or programs
     and scripts is refused whole.

If SolidCraft is already running, a link opened from the browser goes to that window
(`apps/solidcraft/src/single_instance.rs`) instead of starting a second copy.

## Where the scheme is registered

| Platform | Where |
|---|---|
| Windows (MSI) | `HKLM\Software\Classes\solidcraft` (an install for all users) or `HKCU\Software\Classes\solidcraft` (just for me), command `"solidcraft.exe" --open-url "%1"` (`assets/platforms/windows/solidcraft.wxs`) |
| macOS | `CFBundleURLTypes` in `assets/platforms/macos/Info.plist.in`; AppKit hands the link to `application:openURLs:` |
| Linux (deb, rpm, Flatpak, Snap, Nix, FreeBSD) | `x-scheme-handler/solidcraft` in a hidden entry of its own, `io.tangencylabs.solidcraft.workshop-links.desktop` (`NoDisplay=true`, `Exec=solidcraft --open-url %u`); the visible `io.tangencylabs.solidcraft.desktop` opens files as before (`%f`) |
| AppImage | the two entries AppRun installs into `~/.local/share/applications` (`Exec="<AppImage>" %F`, and `Exec="<AppImage>" --open-url %u` for links) |

With `--open-url`, the argument right after it is the link and the app reads nothing else from
its command line: a link holding quotes or spaces that a command line splits into more arguments
can't add an option (`--control`, `--theme-file`) or a file. The question that asks before
anything opens ignores Enter and letter keys for its first second, so a key pressed in the window
as it appears can't answer it; a click counts at once. Downloads go to a new folder with a random
name that only you can read (0700). The sha256 is checked through one open handle: a parts library
unpacks from that handle, and a model or plugin is handed on only while the file is still the one
checked (same size, time and inode). A `block` revocation matches the version as semver reads it
(`v1.2.0` and `1.2.0+build` are `1.2.0`).

The portable Windows zip and the plain `.exe` register nothing; links need the MSI. From a
terminal, `solidcraft --open-url 'solidcraft://workshop/plugin/maker/gears'` does the same as
clicking the button.
