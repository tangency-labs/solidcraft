# Releasing SolidCraft

Releases are deliberate operations that an agent can complete end to end. Development lands
on `main`; pushing the selected commit to `release` builds all packages and creates a **draft**
GitHub Release. Publishing that verified draft is a separate operation. Pushing to `main` or
pushing a tag does not start the release workflow. There is no automatic commit-message-based
version bump and no patch release for every merge.

## Names and deployment

Every release asset, binary, installer product name and package uses the SolidCraft name
(`solidcraft-<version>-linux-<arch>.AppImage`, `SolidCraft-<version>-windows-x64.exe`,
`solidcraft-cli` archives, …). The old name survives only in the shims listed in
[compatibility.md](compatibility.md) (the end-of-life Flatpak refs of the old Linux app ID, the legacy `.zsync` and `.flatpakrepo`
copies, the deprecated old-name CLI alias); keep them until that document says they can go. Do not
rewrite published assets, checksums, tags or historical release notes of earlier versions.

The main installers (`.msi`, portable `.zip`, `.dmg`, `.AppImage`, `.deb`, `.rpm`, `.flatpak`, `.snap`) are
also attached without the version in their names, for example `solidcraft-windows-x64.msi` and
`solidcraft-linux-x86_64.AppImage`, and listed in `SHA256SUMS.txt`. The README's Download section
links to `https://github.com/bherbruck/solidcraft/releases/latest/download/<name>`, which follows
the release marked latest; nightlies never are. Keep these names stable: renaming one breaks the
README links and the in-app update check, which picks them by name (`crates/ui-egui/src/update.rs`;
[updates and privacy](book/src/guide/updates.md)).

Project Pages publishes the documentation, browser app and published-release Flatpak
repository under `/solidcraft/`. Do not add a separate owner-site publisher or permanent
old-name Pages/Flatpak aliases. GitHub repository redirects are separate from Pages hosting;
verify the new project URLs after deployment.

The browser app keeps itself for offline use (`apps/solidcraft-web/sw.js`). Browsers keep that
service worker after the code that registered it is gone, so to switch it off deploy
`apps/solidcraft-web/sw.js` unbuilt (as `trunk serve` builds do): it deletes what it kept and
unregisters. A site without `build.json` is also safe: launches then go to the network.

## Nightly prereleases

`release.yml` also runs at midnight in `America/New_York` (Eastern time, following daylight
saving time). GitHub may start scheduled runs later when its runners are busy.
Before building, it compares the exact scheduled `main` commit with the tag of the most
recently published release, including prereleases. If the commits match, all packaging and
publication jobs are skipped: no new version, tag, or release is created. Drafts do not count
as published releases, so a failed build or publication can be retried the following night.

Changed commits use the existing package build and validation jobs. Only after all jobs
succeed, the workflow stages the assets in a draft and publishes a prerelease named
`v<workspace-version>-nightly.<YYYYMMDD>.<run-number>.<run-attempt>` with checksums and generated
notes. Any existing workspace prerelease suffix is replaced by the nightly suffix. Dates
use Eastern time; package versions are overridden without changing Cargo.toml or Cargo.lock,
so the binaries' embedded Cargo version remains the workspace version. Nightlies do not
replace the latest stable release or advance the `release` branch. The normal deliberate
release process below continues to create drafts for separate verification and publication.

Nightly publication uses `GITHUB_TOKEN`, so it does not trigger the downstream Flatpak Pages
deployment. The stable Flatpak repository is not automatically updated with nightly builds.

## Choose and prepare the version

1. Fetch `origin` and its tags. Inspect the previous published release and the changes since
   it. Choose one version for the complete batch, based on user-visible behavior:
   - Patch: compatible fixes, reliability improvements, and packaging corrections.
   - Minor: new compatible capabilities. While on `0.x`, also use a minor bump for breaking
     changes and explain the break and any migration in the notes.
   - Major: incompatible public behavior or file-format/API changes after `1.0`. Moving to
     `1.0` is an explicit product decision, not something inferred from a commit prefix.
   - Use `-rc.1`, `-rc.2`, etc. when a release candidate is requested.
   Documentation-only changes normally wait for the next release. State the reason for the
   chosen bump in the release notes. Follow any version or release scope specified by the owner.
2. Update `[workspace.package] version` in root `Cargo.toml`. Then run
   `cargo update --workspace --offline` to refresh the workspace packages' versions in
   `Cargo.lock` (it changes only our own crates, never dependency versions, and needs no
   network). Inspect the diff: only `version = ` lines of workspace crates should change.
   SolidCraft does not currently implement `cargo xtask version set`.
3. Fix the design format number: run `scripts/fold-format-pending.sh`. It moves the pending
   format changes (`crates/doc/src/format/pending/*.rs`) into the next `crates/doc/src/format/v<N>/`
   folder, so the release writes and reads that number for good, and does nothing when nothing
   is pending. Commit the move with the version files. (CONTRIBUTING.md, "Changing the design
   file format".)
4. Run `cargo xtask ci` and the packaging checks in `.github/workflows/packaging-lint.yml`.
   Commit the version files through the normal review flow and land them on `main`.
   Do not use `[skip ci]`. Record the exact commit SHA to release. If it changes, revalidate it.

## Build the draft

Use an authenticated maintainer account or GitHub App with the repository's normal release
permissions. Follow branch rules; never force-push or bypass required checks.

```sh
git fetch origin --tags
git switch release
git merge --ff-only origin/release
git merge --ff-only origin/main
git push origin release
```

For the first release, after this workflow has landed on `main`, create `release` from the
validated `origin/main` commit (`git switch -c release origin/main`) and push it. If the branch
already exists, use the commands above. Do not create the branch from unrelated work. If a
fast-forward fails, inspect and resolve the divergence through the normal review flow.

Find the `release.yml` Actions run for the recorded SHA and `release` branch, and monitor that
specific run. The workflow builds Linux (both architectures, including Flatpak), macOS,
Windows, FreeBSD, and web packages. Once every required job succeeds it creates a draft
`SolidCraft v<version>` with the artifacts, `SHA256SUMS.txt`, install notes, and generated changes.

A second push with the same untagged version can rebuild and replace that draft's complete
asset set. Existing release notes are preserved on rebuild; refresh their changes section if
new commits were added. An existing tag or published release is never overwritten. Use a new
version after publication. Do not push the version tag yourself: publishing the draft creates
it at the draft's target commit.

## Notes for the next release

Items to carry into the next release's notes (step 3 below), then remove from here:

- None yet.

## Verify and publish

When the task authorizes shipping a release, agents should complete these steps without
requiring the owner to click through the GitHub UI. A request only to prepare a release or
change release infrastructure does not itself request publication of the application.

1. Confirm that the workflow run succeeded for the intended SHA and that the matching release
   is still a draft targeting that SHA. Avoid publishing while another release build is active.
2. Download all assets to a fresh directory. Run `sha256sum --check SHA256SUMS.txt` and check
   that every expected platform/architecture is present. Smoke-test the built CLI's `--version`
   and an example model on an available platform. Inspect the package-check job output for
   other platforms; report any platform you could not exercise yourself.
3. Review the generated notes. Add a concise description of features, fixes, breaking changes,
   and known limitations. Keep the installation instructions. Packages are currently unsigned;
   do not claim otherwise. Resolve failed checks or missing artifacts before publication.
4. Publish the verified draft using `gh` or the equivalent GitHub release API. For example:

   ```sh
   # Substitute the version that was actually built and verified.
   gh release edit v0.2.0 --draft=false --latest
   # For a release candidate instead:
   gh release edit v0.2.0-rc.1 --draft=false --prerelease --latest=false
   ```

5. Confirm that the release is public and its tag resolves to the recorded SHA. Report the
   release URL, version, checks performed, and any remaining limitations.

## Dry runs and retries

`gh workflow run release.yml --ref <branch> -f draft=false` builds packages and uploads the
`release-assets` Actions artifact without creating or publishing a release. For an explicit
retry that creates/updates a draft, use `--ref release -f draft=true`. Draft creation from
another branch is rejected. A rerun must still use an unpublished, untagged version.

## Flatpak repository coordination

The Flatpak repository proposed in PR #39 consumes **published** releases, not these drafts.
Keep that deployment separate from building the application. A publication made with an
Actions `GITHUB_TOKEN` does not trigger downstream `release: published` workflows; in that
case explicitly dispatch the Flatpak workflow with the published tag, or use an appropriately
authorized maintainer/App token for publication. Do not assume the repository updated: check
the deployment run. The Flatpak workflow requests the shared Pages workflow with that tag;
Pages assembles the documentation, browser app, and Flatpak repository together. Ordinary
docs updates include the latest stable Flatpak release too. Verify the resulting Pages run,
not only the dispatch job, so a release cannot erase the docs or app and docs updates cannot
erase the package repository.

## Snap package and Snap Store

Every release and nightly attaches `solidcraft-<version>-linux-<arch>.snap` for x86_64 and
aarch64 (`packaging/linux/snap-pack.sh`, manifest `packaging/linux/snap/snapcraft.yaml`). Like the
Flatpak, it repackages the Linux job's tarball. The `snap` job installs it with `--dangerous`,
starts the confined app on headless X11 and Wayland (`packaging/linux/snap-smoke.py`), models a
box over the control channel and keeps the captured frames as the `snap-smoke-<arch>` artifact.

The Snap Store steps run only when the `SNAPCRAFT_STORE_CREDENTIALS` repository secret exists;
without it they are skipped and the `.snap` files are still attached to the release:

- nightlies are uploaded to `latest/edge`;
- release drafts (pushes to `release`) are uploaded to `latest/candidate`, after every package
  built and the draft was staged;
- publishing a non-prerelease GitHub release runs `snap-store.yml`, which promotes
  `latest/candidate` to `latest/stable`. As with the Flatpak repository, a publication made with
  `GITHUB_TOKEN` doesn't trigger it: then run `gh workflow run snap-store.yml`.

To enable the Snap Store once (owner):

1. Sign in at <https://snapcraft.io> with an Ubuntu One account and register the name at
   <https://snapcraft.io/register-snap> (or `snapcraft login` then `snapcraft register solidcraft`).
2. On a machine with snapcraft (`sudo snap install snapcraft --classic`), export upload-only
   credentials for this snap:

   ```sh
   snapcraft export-login --snaps=solidcraft \
     --channels=edge,candidate,stable \
     --acls=package_access,package_push,package_update,package_release \
     --expires=2027-10-01 solidcraft-store-credentials.txt
   ```

3. Store them as the repository secret and delete the file:

   ```sh
   gh secret set SNAPCRAFT_STORE_CREDENTIALS -R bherbruck/solidcraft < solidcraft-store-credentials.txt
   rm solidcraft-store-credentials.txt
   ```

4. Optional: fill in the store listing at <https://snapcraft.io/solidcraft/listing> (the icon,
   summary and description come from the snap; add screenshots and the category "Art and design").
5. The next nightly fills `edge` and the next release fills `candidate`, then `stable` on
   publication. To fill `stable` from an already published release right away, download its
   `.snap` files and run `snapcraft upload --release=stable <file>` for each.

Renew the credentials before `--expires` by repeating steps 2 and 3. The snap uses strict
confinement and only plugs that connect automatically, apart from `removable-media`, which users
connect with `sudo snap connect solidcraft:removable-media` (getting-started.md).

## WinGet (Windows Package Manager)

SolidCraft is not in [winget-pkgs](https://github.com/microsoft/winget-pkgs) yet: its first
version, as `bherbruck.SolidCraft`, has to be submitted and merged there once (by hand, with
Komac or `wingetcreate`) before `winget install bherbruck.SolidCraft` finds it. A first
submission, for 0.21.0, is already open as
[microsoft/winget-pkgs#452486](https://github.com/microsoft/winget-pkgs/pull/452486) (flagged
`Validation-Executable-Error`): take that one up rather than opening another. Until then the
README and [downloads](downloads.md) don't offer the command (#5734); add it back when the
package is listed. After that first version, publishing a non-prerelease
GitHub release runs `winget.yml`, which uses [Komac](https://github.com/russellbanks/Komac)
(a pinned, checksum-verified binary) to open a pull request at microsoft/winget-pkgs from the
owner's fork `bherbruck/winget-pkgs`. It submits the three versioned MSIs
(`solidcraft-<version>-windows-{x64,x86,arm64}.msi`, never the fixed-name copies), reads their
product codes, links the release notes and keeps the other fields from the previous version's
manifest. The winget-pkgs bots validate it and a moderator merges it, usually within a day or two.

The job does nothing, successfully, while the `WINGET_TOKEN` repository secret is missing or
before the package's first version is merged into winget-pkgs. Like `snap-store.yml`, a
publication made with `GITHUB_TOKEN` doesn't trigger it: then run
`gh workflow run winget.yml -R bherbruck/solidcraft -f tag=v<version>`.

To enable it once (owner):

1. Create a **classic** personal access token at <https://github.com/settings/tokens/new>
   (Settings > Developer settings > Personal access tokens > Tokens (classic)): note
   "solidcraft winget", an expiry such as one year, and only the **`public_repo`** scope.
   Fine-grained tokens can't open pull requests on microsoft/winget-pkgs.
2. Store it as the repository secret (paste the token when asked):

   ```sh
   gh secret set WINGET_TOKEN -R bherbruck/solidcraft
   ```

3. Keep the fork `bherbruck/winget-pkgs` (Komac pushes its branches there). Its default branch
   doesn't need syncing.

When the token expires, create a new one the same way and run step 2 again. If a release was
published while the secret was missing or expired, submit it afterwards with
`gh workflow run winget.yml -R bherbruck/solidcraft -f tag=v<version>`.

## Update signing

The app's **Update now** (docs/book/src/guide/updates.md) only installs a package whose SHA-256
matches the release's `SHA256SUMS.txt`, fetched over HTTPS. That protects against corrupted and
tampered downloads in transit, but not against someone who can upload release assets. Signing
the checksum list closes that gap: once `PUBLIC_KEY` in `crates/ui-egui/src/update/install.rs`
is set, the app also requires `SHA256SUMS.txt.minisig`, a [minisign](https://jedisct1.github.io/minisign/)
signature by that key whose trusted comment names the release (`SolidCraft v<version>`), and
refuses to update in place otherwise. The release workflow signs the list when the
`UPDATE_SIGNING_KEY` secret exists, and fails when the app expects signatures but the secret is
missing.

To turn it on once (owner):

1. On a trusted machine with minisign (`sudo apt install minisign`, `brew install minisign` or
   `winget install minisign`), make a key pair without a password (the workflow can't type one):

   ```sh
   minisign -G -W -p solidcraft-update.pub -s solidcraft-update.key
   ```

2. Store the secret key as the repository secret and keep an offline backup of it (a lost key
   means a new key and an app release that carries it; a leaked key means the same, quickly):

   ```sh
   gh secret set UPDATE_SIGNING_KEY -R bherbruck/solidcraft < solidcraft-update.key
   ```

3. Put the public key (the second line of `solidcraft-update.pub`, starting with `RW`) into
   `crates/ui-egui/src/update/install.rs`:

   ```rust
   pub const PUBLIC_KEY: Option<&str> = Some("RW…");
   ```

   and land that through a PR. Builds from then on refuse unsigned updates; releases built
   from then on carry `SHA256SUMS.txt.minisig`.
4. Delete `solidcraft-update.key` from the machine once the backup is safe.

Older builds without the key keep updating with the SHA-256 check alone.

## Repository setup

The `release` branch is advanced only for intentional release batches. Restrict its writers to
maintainers/release agents through repository rules if required by the owner's policy. The
workflow needs `contents: write` only in its draft-creation job. It does not push commits,
calculate versions, or require a semantic-release bot to bypass `main` protections.

This workflow replaces the every-push-to-main semantic-release proposal in PR #38; do not
merge that automation alongside this release model without redesigning its triggers.
