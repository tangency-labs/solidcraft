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
| Windows (MSI) | `HKLM\Software\Classes\solidcraft`, command `"solidcraft.exe" --open-url "%1"` (`assets/platforms/windows/solidcraft.wxs`) |
| macOS | `CFBundleURLTypes` in `assets/platforms/macos/Info.plist.in`; AppKit hands the link to `application:openURLs:` |
| Linux (deb, rpm, Flatpak, Snap, Nix, FreeBSD) | `x-scheme-handler/solidcraft` in `io.tangencylabs.solidcraft.desktop`, `Exec=solidcraft %u` |
| AppImage | the entry AppRun installs into `~/.local/share/applications` (`Exec="<AppImage>" %U`) |

The portable Windows zip and the plain `.exe` register nothing; links need the MSI. From a
terminal, `solidcraft --open-url 'solidcraft://workshop/plugin/maker/gears'` does the same as
clicking the button.
