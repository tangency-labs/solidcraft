---
name: solidcraft-packaging
description: "Use when preparing or shipping a SolidCraft release, changing installers or packages (AppImage, MSI, dmg, Flatpak, Snap, WinGet, Nix, FreeBSD, web), release asset names, signing or the Pages deployment."
---

# Packaging and releases

Development pushes are not application releases. Pushing to `main` or pushing a tag does not start a release. The full procedure is `docs/releasing.md`; follow it when asked to prepare or ship a release.

## Preparing
1. Choose one version for the whole batch from the actual changes (patch, minor, major, `-rc.N`). Do not infer it from commit prefixes. Moving to `1.0` is the owner's decision.
2. Update `[workspace.package] version` in `Cargo.toml`, then `cargo update --workspace --offline`; only our own crates' `version =` lines in `Cargo.lock` change.
3. Fold the pending design-format changes with `scripts/fold-format-pending.sh` (`skills/solidcraft-file-formats/SKILL.md`).
4. Run `cargo xtask ci` and the packaging checks in `.github/workflows/packaging-lint.yml`. Land the version commit on `main` through the normal review flow; never use `[skip ci]`.

## Building and publishing
- Advance the `release` branch with fast-forwards only (`git merge --ff-only`); never force-push. `.github/workflows/release.yml` builds every package and creates a draft GitHub Release.
- Verify the draft before publishing: the run succeeded for the intended SHA, every platform and architecture is present, `sha256sum --check SHA256SUMS.txt` passes, and the built CLI runs (`--version`, an example model).
- Only when shipping is authorised, publish the verified draft with `gh release edit`, then confirm its tag resolves to the recorded SHA and the downstream deployment ran. A request to prepare a release is not a request to publish.
- Never overwrite a published version, tag or asset; use a new version. Do not push the version tag yourself. Packages are unsigned unless `docs/releasing.md` says otherwise; do not claim they are signed.
- Nightlies are prereleases built by the same workflow; they never become latest or advance `release`.

## Names that must stay stable
- Assets are `solidcraft-<version>-...` and are also attached without the version (`solidcraft-windows-x64.msi`). The README links and the in-app update check (`crates/ui-egui/src/update.rs`) pick them by name; renaming one breaks both.
- The old product name survives only in the shims in `docs/compatibility.md`. Do not rewrite published assets, checksums, tags or historical notes.
- Documentation, the browser app and the Flatpak repository publish under project Pages `/solidcraft/`. Add no old-name Pages aliases.

## Where things live
- `packaging/` holds the per-platform scripts (`packaging/linux`, `packaging/windows`, `packaging/macos`, `packaging/freebsd`, `packaging/web`); `packaging/tests` checks them.
- Nix: `flake.nix` and `docs/nix.md`. Building: `docs/building.md`. Downloads: `docs/downloads.md`.
- Build with `CARGO_TARGET_DIR=<repo>/target`. Windows from Linux: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.
