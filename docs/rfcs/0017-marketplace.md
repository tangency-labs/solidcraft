# RFC 0017: Marketplace, finding and installing plugins, part generators and content packs

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-25 |
| Apps | SolidCraft (the index and the shared client crate are app-agnostic, so other Craft apps can use them) |
| Discussion | [#22331](https://github.com/tangency-labs/solidcraft/issues/22331) |
| Extends | [RFC 0002](0002-plugins.md) (plugins; this RFC is its phase 6, "curated, signed index") |
| Related | [RFC 0015](0015-plugin-ui-workspaces.md) (plugin UI), [#20425](https://github.com/tangency-labs/solidcraft/pull/20425) (`min_host_version` and `optional_capabilities`), [#5586](https://github.com/tangency-labs/solidcraft/pull/5586) (install from a GitHub link), [#7558](https://github.com/tangency-labs/solidcraft/issues/7558) (extras and accessory packs: what users want) |

People should be able to find a plugin, a theme, a material library or a part generator inside SolidCraft, read who made it and what it may do, and install it in a few clicks, without us shipping every one of them and without us pretending to have vetted code we have not. This RFC says how, in three trust tiers, on top of the sandbox, permissions and approval screen RFC 0002 already built.

Today there is a plugin runtime, an install flow from a `.zip`, a "Update to x.y.z" check against a plugin's own GitHub releases, and Preferences > Plugins to enable, disable and remove. There is no place to discover anything, no signing, no list of who is trusted, and no way to pull a bad plugin back.

**One idea runs through every section: the marketplace adds discovery and trust, never power.** An installable from the marketplace runs in exactly the sandbox, with exactly the permissions and the same approval screen, as the same package installed from a file. The tier changes what we tell the user and how much friction the riskiest grants carry, not what the sandbox allows. The one exception is the reserved `net` permission, which RFC 0002 already limits to reviewed, signed plugins.

## Decisions

These are proposed here and are open for comment until the RFC is accepted.

1. **Three kinds of installable:** plugins, content packs and part generators (section 2). Each is a package with one manifest. What a package may contain decides its kind; there is no second packaging format.
2. **Three trust tiers:** Verified (reviewed and signed), Listed (in the index, checked by machine only, not reviewed) and Custom (any GitHub `owner/repo` or URL the user types) (section 3). The user always sees which tier an item is in.
3. **The index is a public Git repository** of small JSON files, built into one signed snapshot. SolidCraft downloads the snapshot and reads it. It never calls a server of ours that holds user data (section 4).
4. **Artifacts stay on GitHub Releases** in the author's own repository, as RFC 0002 section 5 already prefers. The index points at them and pins their hashes. We do not host packages in v1.
5. **Everything needs the existing approval.** Nothing runs before the user approves the exact package (`package_hash`). A marketplace install never auto-approves, and a changed package asks again (section 6).
6. **Versions follow what #20425 built.** The index shows, and the installer picks, the newest release whose `min_host_version` and required capabilities this SolidCraft meets (section 5).
7. **Delisting and revocation are different.** Delisting hides an item from browse. Revocation switches an installed package off (section 9.1).
8. **Offline is the normal case.** An installed package works with no network. The index is cached. Browsing offline shows the cache with its date (section 10).
9. **Listed items follow our clean-room and licence rules** (section 12). Custom items are the user's own risk, and the app says so.
10. **No new Preferences rows.** Marketplace settings live on the marketplace's own page.

## 1. Goals and non-goals

**Goals**
- Browse, search, install, update and remove community work from inside SolidCraft, on desktop first.
- Let a person who does not know what a WebAssembly sandbox is tell a reviewed item from an unreviewed one at a glance.
- Let an author publish by adding one small file to their repository and opening one pull request, with automated checks that tell them what to fix.
- Let an agent do all of it over the command surface, with no dialog (everything is a command).
- Keep the user's designs opening without a marketplace and without the network (RFC 0002: SolidCraft never refuses to open a file).

**Non-goals**
- No payments, licences keys, paid listings or advertising. A free marketplace only.
- Not a replacement for core features. If many people need something, it belongs in SolidCraft itself.
- Not hosting. We do not store or serve authors' packages in v1 (the web build is the one question, section 11).
- Not a social network. No accounts, comments or follower counts of our own in v1.
- No native executables, no separate processes and no containers. Home Assistant's add-ons, which run as separate programs beside the host, are the part of that model we deliberately leave out: a SolidCraft installable is sandboxed WebAssembly or data, and nothing else.

## 2. The three kinds

We took Home Assistant's split as the model: code that runs inside the host and is described by a manifest the host reads before loading any code (its integrations); heavier things that run beside the host (its add-ons), which we do not copy; and the community content that a store such as HACS also carries, which is data and configuration (themes, templates, dashboard parts). SolidCraft's three kinds are:

| Kind | What it is | Runs code | Exists today |
|---|---|---|---|
| **Plugin** | Commands, importers, exporters, panels, workspaces: a sandboxed WebAssembly component with a `runtime` in its manifest (RFC 0002, RFC 0015) | Yes | Yes, in `crates/craft-plugin` |
| **Content pack** | Data only: colour themes, view shaders, material and appearance libraries, sample designs, drawing templates and title blocks, render studios and environments, profile and fastener tables | No | Themes and shaders: yes (`themes@0.1`, `shaders@0.2`, a package with no runtime). The rest need a capability each (section 2.2). |
| **Part generator** | Builds a part from a vendor ordering code or from parameters (section 2.3) | Yes (a plugin with a `generators` declaration) | No |

A package may be more than one thing, as a plugin that ships its own theme already may. The marketplace shows its **primary kind** (the first that applies in the order plugin, generator, content pack) and lists the rest as contents.

### 2.1 Plugins

No change to the package. The marketplace adds a listing around it (section 4.2). A plugin keeps every field RFC 0002 defines: `id`, `name`, `version`, `capabilities`, `runtime` with entries, permissions, limits, listeners and slots.

### 2.2 Content packs

A content pack is a package whose manifest has no `runtime` and one or more data sections, each behind a capability the host negotiates the way it does for `themes@0.1` today:

| Section | Capability | Contents |
|---|---|---|
| `themes` | `themes@0.1` (exists) | Colour themes for the theme picker |
| `shaders` | `shaders@0.2` (exists) | View backgrounds and tints |
| `materials` | `materials@0.1` (new) | Engineering material data and visual appearances, in the formats the appearance and material work already defines |
| `samples` | `samples@0.1` (new) | Sample designs shown on Start under Samples, each with its own picture |
| `templates` | `templates@0.1` (new) | Drawing sheets, borders, title blocks, design templates |
| `tables` | `tables@0.1` (new) | Standard-size tables for the profile and fastener commands |

The new capabilities are separate small proposals, one per section. They are not part of the first marketplace phase. Themes and shaders are enough to ship the first content packs, and the marketplace must not wait on the rest. Each new section follows the existing rules: it is data, a host that does not know the section ignores it with a "needs a newer SolidCraft" note instead of failing to load the package, it adds no code, and it is checked by the host against schemas and size caps before it is used.

Because a content pack runs no code, its approval screen is short: name, publisher, licence, what it adds, and nothing to grant.

### 2.3 Part generators

A part generator takes a **vendor ordering code** ("the part number you would order") or a set of **parameters**, and builds the part in the open design. It is a plugin with one extra declaration, so it uses the sandbox, limits, approval and undo that plugins already have.

```json
"generators": [{
  "id": "hex-bolt",
  "label": "Hex bolts",
  "family": "fasteners",
  "code": {
    "pattern": "^HB-M(\\d{1,2})x(\\d{1,3})-(A2|A4)$",
    "examples": ["HB-M8x30-A2"]
  },
  "entry": "build",
  "input_schema": { "type": "object", "properties": { "diameter": {}, "length": {}, "grade": {} } },
  "sources": [{ "title": "Maker bolt datasheet", "url": "https://example.com/datasheet.pdf" }]
}]
```

- **Two ways in, one result.** The user gives a code, the host checks it against `code.pattern`, and passes `{ "code": "…" }` to the entry. Or the user fills a form drawn from `input_schema` (the same schema-driven forms as every plugin entry) and the entry gets the parameters. Either way the entry builds the part.
- **The result is native features.** The entry calls ordinary engine commands (sketch, extrude, hole, pattern) inside one undo step, so the part is editable, journaled and opens without the generator. A generator that cannot express its part with native features means core lacks a building block, and that goes to an issue, as RFC 0002 section 4 says.
- **The design remembers where it came from.** The host records the generator's id, version and the code or parameters on the part's feature row, so the part can be rebuilt, updated, or reopened later without the plugin through the fallbacks RFC 0002 section 4 lists (`recorded_script` first).
- **Agents find generators by code.** `generator.find` takes a code and returns the enabled generators whose pattern matches. Each generator is also a named tool `<plugin>.<generator>` (RFC 0002 section 6c), so an assistant can say "insert HB-M8x30-A2" under the usual approval gate.
- **No network.** A generator cannot look a code up online, because plugins have no network. The catalog data (codes, dimensions) is in the package and is updated by a new release. This is also what keeps generators reproducible: the same code at the same version gives the same part.
- **Sources are listed.** `sources` says where the dimensions came from (a public datasheet, a standard). It is required for a Verified or Listed generator, because it is the evidence for the clean-room rules in section 12. The approval screen shows it.

A catalog-only generator, which needs no code because a built-in parametric template reads its table, is attractive but depends on those templates existing in core. It is left to a later proposal and does not change this section.

## 3. The three tiers

| | **Verified** | **Listed** | **Custom** |
|---|---|---|---|
| How it gets there | A person on the review team reads the source and the permissions, then adds it to the index's verified list, signed | The author opens a pull request on the index repository; automated checks pass; it is merged without a code review | The user types `owner/repo`, a GitHub URL or `owner/repo@tag` |
| Who we are vouching for | The code and the publisher | Only that the listing is well formed and meets the rules in section 12 | Nothing |
| Shown as | Verified badge, publisher name | "Not reviewed" label | "Not in the index, not reviewed" banner, in a different colour |
| In search | Yes, ranked first | Yes, below Verified | No (the user already knows what they want) |
| Sandbox | The standard sandbox | The same | The same |
| Permissions on the approval screen | As requested | As requested, with the extra care in section 6 | As requested, with the extra care in section 6 |
| `net` (RFC 0002, reserved) | Eligible later, for named `https` origins, requested by the host | Never | Never |
| Updates | Offered; auto-offered without re-review when the declared permissions and entries are unchanged (section 8) | Offered, with the changed-permission rule | Offered, with the changed-permission rule |
| Index signature covers its hash | Yes | Yes | Not in the index |
| Revocable by us | Yes | Yes | Only if its hash or repository is on the revocation list |

**Verified.** The review reads the source and the component's build (section 7), the manifest's permissions against what the code needs, and the clean-room checks (section 12). A "trusted developer" is a GitHub account or organization on the index's developers list, added after at least one reviewed plugin and an agreement to the publisher rules. Trusted developers still go through review for a first listing and for any update that changes permissions or entries; a code-only update from a trusted developer goes through the machine checks and reaches users with the badge once the index builds. This is the HACS idea of a default list that maintainers curate, with the Home Assistant idea that custom integrations are not supported by the project, turned into an explicit label.

**Listed.** The name is honest: the index lists it, we have not vetted it. Automated checks cover the manifest, version rules, licence, attribution, sizes, a built component that matches the listing, and a scan of the package for banned content (section 12). They cannot prove behaviour.

**Custom.** The user adds a source, as HACS lets a person add a custom repository. We accept GitHub only in v1 (`owner/repo`, `owner/repo@tag`, or an `https://github.com/owner/repo` URL), because the release API gives us a digest to check (section 7). Other hosts need a reason to trust their digests and are left to a later proposal; a `.zip` from anywhere still installs from a file. The same install checks, the same approval, and a clear banner. The app never lists a Custom source in search. The user's added sources are kept in the marketplace page and can be removed.

## 4. The index and the listing

### 4.1 The index

One public repository, `tangency-labs/solidcraft-index` (name to confirm), holds:

- `verified/<id>.json` and `listed/<id>.json`: one small file per item (section 4.2).
- `developers.json`: trusted developers.
- `revoked.json`: revocations (section 9).
- A build workflow that validates every file, fetches each item's releases, checks them (section 4.3), and writes one `index.json` plus a detached signature `index.json.sig`, published as a release asset and on project Pages.

SolidCraft downloads `index.json` and `index.json.sig`, verifies the signature with public keys built into the app (section 7), and reads the verified snapshot. A snapshot has an issue time, a sequence number that only increases, and an expiry. The app refuses an older sequence than it has already seen, so an old snapshot cannot be replayed to hide a revocation.

The index is small: id, name, short description, kind, tier, publisher, licence, repository, tags, and per release the version, `min_host_version`, required and optional capabilities, size and hashes. It carries no package bytes and no pictures. Pictures are loaded when the user opens an item's page (section 10).

The index schema is generated from Rust types in a shared crate, `craft-market` (no SolidCraft dependency, builds for wasm; `cargo xtask layers` forbids otherwise), the way the plugin manifest schema is generated from `craft-plugin`.

### 4.2 The listing file

An author adds one file to the index repository. It is deliberately short, because the package's own manifest stays the source of truth for what the package does:

```json
{
  "id": "gears",
  "repo": "maker/gears",
  "tier": "listed",
  "kind": "plugin",
  "summary": "Spur and helical gear generator.",
  "tags": ["gears", "mechanisms"],
  "licence": "MIT",
  "homepage": "https://github.com/maker/gears",
  "sources": []
}
```

- `id` must equal the package manifest's `id`. `repo` is where the releases are. The two must also agree with the package's `updates.github` (RFC 0002 section 5), so a package cannot claim another repository's updates.
- Everything else (name, version, description, capabilities, permissions, entries, `min_host_version`) is read from the released package by the index build and copied into the snapshot, never typed twice.
- The listing lives in the index repository, not inside the package, for two reasons: reviewers and the revocation tooling edit it without the author's release, and the installed manifest keeps its current strict field set (`deny_unknown_fields`) so SolidCraft before this RFC still loads the package. The inert `extensions` map in the manifest can carry optional extra metadata, such as a long description file name, that the host ignores.
- A listing for a content pack names its sections in the build output, not the listing.

### 4.3 What the index build checks

Failing any check fails the pull request with a message that names the field and how to fix it. These are machine checks, not a review:

1. The listing parses and the id is unused or is the same repository's.
2. A stable release exists, tagged as a `major.minor.patch` version, with an asset `<id>.zip` or `<id>-<version>.zip` and a GitHub-listed sha256 digest (the rule `updates.github` already uses).
3. The package passes `plugin check` (and the content-pack equivalent): manifest, schemas, size caps (`MAX_MANIFEST_BYTES`, `MAX_COMPONENT_BYTES` and the rest of `craft-plugin`), safe archive rules, an icon that passes the image checks.
4. The manifest's id, version and `updates.github` agree with the listing and the tag.
5. The licence in the listing is on the allowed list and matches the repository's licence file (section 12).
6. Every non-code asset has an attribution row (section 12).
7. For a generator, `sources` is present.
8. The repository is public, not archived, and has issues enabled, so reports have somewhere to go.

Every release the build sees is re-checked on every run, so a release whose digest later changes is dropped from the snapshot.

## 5. Manifests, versions and compatibility

**Per kind, what the host reads.** The installed package's manifest is the one in `crates/craft-plugin`: `id`, `name`, `version`, `min_host_version`, `description`, `capabilities`, `optional_capabilities`, `runtime`, `shaders`, `themes`, `updates` and the inert `extensions`. A plugin fills `runtime`; a content pack fills only its data sections; a generator fills `runtime` and `generators`. The listing (section 4.2) adds the marketplace-only facts. Nothing in the listing can widen what the package may do.

**`min_host_version`.** A complete semantic version, the oldest SolidCraft the package supports. The host already refuses a package that needs a newer SolidCraft, with that reason, before publishing it. The index build records it for every release.

**`capabilities` and `optional_capabilities`.** Required capabilities must be served by this host, or the package is refused before any code runs. Optional ones are granted when supported and the plugin must provide a fallback. The index records both lists for every release, as `name@major.minor`.

**Choosing the version.** For an item with several releases, the marketplace offers the newest release that this SolidCraft can run: `min_host_version` is not newer than the host, every required capability is served, and the release is not revoked. If the newest release needs a newer SolidCraft, the item page says so ("version 2.1 needs SolidCraft 0.54; version 2.0 is offered"), the way HACS lets a person install an older version when Home Assistant is too old for the latest. The installer re-checks on download and never trusts the index's reading of compatibility, because the host's own check is the authoritative one.

**Version rules.** Package versions are strict `major.minor.patch`, as the update check already requires. A pre-release is never offered unless the user turns on "show pre-releases" for that one item. A listed version cannot be reused with different contents: the index pins the hash of each version, and a hash mismatch is a revocation reason, not an update.

**API compatibility.** The WIT version the package was built for is part of its required capabilities (`commands@0.1` and so on), so a package built for an older API keeps loading, as RFC 0002 requires. The index shows the oldest and newest host versions each release runs on, and the author is told at listing time when a host release is about to drop a capability their package needs (a deprecation list in the index).

**Content-pack format versions.** Each data section carries its capability version, so a newer pack is refused or skipped by an older host with a clear message, never half-loaded.

## 6. Permissions and the sandbox, per tier

The sandbox and the permission model do not change. This section only says how the tier shapes what the user sees and what extra care applies.

What exists and stays exactly as it is (RFC 0002, `docs/plugins.md`): permissions in the manifest (`files` folders with read or write, `clock`, command groups, `input_folder`, the reserved `net`), fuel, memory, stack and deadline limits and the app's ceilings, the approval screen showing what is requested, folders chosen by the user with Browse, approvals stored per plugin id, and the approval bound to `package_hash`, the hash of the exact manifest and component. The `archive` hash covers the whole `.zip`. A changed package, even one edited in place, runs only after it is approved again. Approvals can only narrow the manifest's requests.

**What the tier adds**

| | Verified | Listed | Custom |
|---|---|---|---|
| Approval screen header | Publisher, "Verified", licence, source link | Publisher, "Not reviewed", licence, source link | Repository, "Not in the index, not reviewed", licence as GitHub reports it |
| High-risk grants (write access to a folder, the `file` command group, application settings or other plugins' command groups, fuel or memory above the app's default ceilings, `input_folder`) | One confirmation | A second step: the user must tick each high-risk grant on its own line | Same as Listed |
| Install with no folder, no command group and default limits | One confirmation | One confirmation | One confirmation |
| Headless install (`plugin.install` and the new market commands) | Needs `approval` and `archive` from a dry run, as today | Same, plus `"accept_unreviewed": true` | Same |
| Content pack | Nothing to grant | Nothing to grant | Nothing to grant |

A second step for the riskiest grants on unreviewed items is the only added friction, so a harmless theme or a read-only importer installs in one click while a plugin that wants to write to a folder makes the user stop and look.

The approval screen's wording is plain: what the item can do, in the words the screen already uses (files, designs, application settings, other plugins). The tier line is never hidden inside a tooltip.

**Not changed, and worth saying again:** there is no network for plugins, a manifest asking for `net` is refused, and Custom and Listed items can never get it. The marketplace itself talks to the network, not the plugin (section 10).

## 7. Signing and provenance

Three separate things need to be true: the index is ours, the package is the one the index described, and the package is what the author built from the source they showed us.

1. **The index is signed.** `index.json` has a detached signature with a project key (ed25519, with a permissively licensed Rust implementation, which the new dependency's PR names as the ground rules ask). The public keys are compiled into SolidCraft, with room for more than one so a key can be rotated: a new key is added to the app, the old one retired after a release cycle. The signing key is kept offline and used by the release process, not by the index repository's workflow, which uploads the unsigned file for a maintainer to sign. A signature that does not verify means the app uses its last good snapshot and says so.
2. **The package matches the index.** The index pins the `archive` sha256 (and the content hash `package_hash`'s inputs) of every offered release. The download is checked against the hash GitHub lists for the asset and against the pin; both must match. For Custom there is no pin, so GitHub's digest over TLS is the only check, which is the check the update flow already makes.
3. **The package was built from the source.** Verified requires that the release asset is built by public CI from the tagged commit, and records the repository, commit and workflow in a build attestation published with the release (GitHub's artifact attestations or an equivalent signed statement). The review reads the workflow. Verifying attestations inside the app is not in v1 (it needs a verifier we would have to trust and ship); v1 records them in the index, shows them on the item page, and checks them in the index build and in review. Verifying them in the app is a later phase.
4. **Source must be public and reproducible enough to read.** Verified plugins are open source (RFC 0002 section 5 already says so). A component with no public source is not eligible.
5. **Developer keys.** Trusted developers are identified by their GitHub account or organization in `developers.json`, not by a key of theirs. Per-author signing keys come later; they add a second thing to lose and a second thing to revoke.

The approval hash the user sees binds all of it: if anything about the package changes after review, `package_hash` changes, the stored approval no longer matches, and the plugin stops until approved again, the existing behaviour.

What this does not give: it does not prove a verified plugin is free of bugs or of a later malicious update that passes review. That is why Verified updates that change permissions or entries are reviewed again, and why revocation exists.

## 8. Updates

The update flow RFC 0002's `updates.github` field and `docs/plugins.md` describe is the base, and stays: a daily and at-start check, one request per plugin (at most 20), a stable release with a version tag newer than the installed one, an asset with a GitHub-listed digest, a download into the hidden updates folder, the same confirmation as install, an undoable replacement with no restart, folders kept by the existing rules.

What the marketplace adds:

- **One index request replaces many.** For an item that is in the index, the check reads the signed snapshot (one small download) instead of calling GitHub once per plugin. Custom items and items whose index entry is missing still use the per-repository call. The update setting that turns checks off, and the organization's policy over it, turn this off too.
- **Newest compatible, not newest.** The offered update is the newest compatible release (section 5), and the card says when a newer one is held back and why.
- **Verified updates without re-review.** If a Verified item's new release has the same declared permissions, command groups, entries, slots and limits as the reviewed one (compared on the manifest, not on the code), the index build admits it with the Verified badge. A release that changes any of them stays Listed-until-reviewed: it is shown as an update with "not yet reviewed" and the user is told. RFC 0002's rule stands: a code-only update never adds an entry or surface silently, and a change in what was approved stops the plugin until the user approves again.
- **Never silent.** Updates are offered, never installed without the user's click and the confirmation. There is no auto-update in v1. A later "update trusted plugins automatically" choice, if people want it, is a separate decision with its own page, never a Preferences row.
- **Rollback.** The previous version is kept until the next update, so "Restore previous version" works offline. It is the same package and the same approval hash, so nothing new runs.
- **Content packs** update the same way. A pack that adds a theme and a plugin updates as one package.

## 9. Ratings and reports

**v1 does not collect ratings.** We do not run accounts, and a star count we cannot defend (fake votes, review brigades) would be worse than none. Instead the item page shows facts the index build already has: the publisher, the licence, the date of the last release, how many releases, the compatible host versions, the repository's open-issue count and star count as reported by GitHub on the day the snapshot was built (labelled as GitHub's, with that date), and whether the item is Verified.

**Reports.** Every item page has **Report a problem**. It opens, in the user's browser, a pre-filled GitHub issue form on the index repository with the item id, version and category, and nothing else from the app (no design, no path, no machine details unless the user adds them). Categories: it does not work, it behaves badly, it asks for too much, licence or clean-room concern, security. A security category points to a private way to report (GitHub's private vulnerability reporting on the index repository) rather than a public issue. The app sends nothing itself; the user's browser does.

**Handling.** The maintainers triage reports. A licence, clean-room or security report can lead to delisting or revocation (section 9.1). A report about behaviour goes to the author's repository. Reports that name an author are answered in public on the issue.

**Later.** Ratings may come back as "works for me" counts if we find a way to count one person once without accounts. That is not a promise.

### 9.1 Delisting and revocation

Two actions, kept apart, because one hides and the other stops.

- **Delisting** removes the item from browse and search and from "Update available". Already-installed copies keep working, and keep their approvals. Used when an item is abandoned, the author asks, or the licence or listing rules are no longer met and no harm is involved.
- **Revocation** is for harm: malware, data theft, a sandbox escape, a clear licence or clean-room violation, a replaced package whose hash no longer matches what was reviewed. `revoked.json` lists a package id and version range or an exact `archive` hash, with a reason, a severity and a date.
  - `block`: the host refuses to run the package and shows why; the user cannot override it. The stored approval stays on disk but is not honoured.
  - `warn`: the package keeps running, the card shows a banner with the reason, and the user can remove it from there.
- Revocation applies to Custom items too, by hash or by repository, because the list is checked against everything installed. A Custom item the user installed that we have revoked is shown the same way.
- The app checks the revocation list whenever it reads a fresh snapshot (at start and daily, with the update check) and applies it to installed packages at once. The list is small and is kept even when the item is no longer in the index. An expired or stale snapshot never clears an entry.
- Revoking a package never deletes the user's designs, the data a plugin saved in them, or the plugin's package folder. Removing the package stays the user's choice.
- Offline: the cached revocation list applies. Past a set age (30 days, to be confirmed) the app shows a notice that it could not check, and never silently pretends the list is current.
- The reasons for each delisting and revocation are recorded in the index repository's history, in public.

## 10. Offline use

- **Installed items need no network.** A package is a folder in the plugins directory; its approval is on disk. Opening a design, running a plugin, using a theme, a generator or a material library never touches the network.
- **The index is cached on disk** with its issue time and sequence number. Browse works offline from the cache and says "as of <date>". Search runs on the cache.
- **Installing offline** works from a `.zip` file (as today), and from the cache of a package the user already downloaded. There is no way to download a package offline; the button says why.
- **Pictures and long descriptions** are fetched when the user opens an item's page, from the author's repository over `https`, and cached with a size cap. A missing picture never blocks anything. The app draws everything itself: text from the listing and README is shown as plain text, with no links that run, and no remote content is rendered as markup. This keeps the page from being a way to put anything on screen.
- **What the app sends.** To read the index: a plain `GET` for two files, with no identifiers. To download an item: a plain `GET` of the release asset from GitHub. Nothing else, and no usage counts. The same controls as the update check govern it: the update setting, and an organization policy that can turn it off, or point the app at the organization's own mirror of the index (an `index_url` the policy sets), so a company can run a fully private marketplace or none.
- **Air-gapped use.** `solidcraft-cli` can verify and install from a downloaded snapshot and package files (`market install --from <dir>`), so a machine with no network can still use a reviewed set carried over on a drive.

## 11. The web build

The browser has no plugins folder, and RFC 0002 leaves plugin hosting in the web build to its phase 5. The marketplace follows it, in three steps:

1. **Browse only.** The snapshot is a static file; the index build publishes it with permissive CORS headers on project Pages, so the web build can read it, verify the signature (the verifier is plain Rust and builds for wasm) and show items. Install is replaced by "Open in the desktop app" and by a link to the item's repository.
2. **Content packs.** Themes, shaders and later materials, samples and templates are data. They can be stored in the browser's own storage and used. GitHub's release downloads do not allow cross-origin reads from a page, so packs for the web are mirrored by the index build onto the Pages site, with their hashes in the signed snapshot. This is redistribution, which is why Verified and Listed content packs must carry an open licence that allows it (section 12). Custom items cannot be installed on the web.
3. **Plugins and generators** wait for the web plugin host (RFC 0002 phase 5) and use the same mirror and the same approvals.

The client crate follows the repository's wasm rules: no thread spawning or `std::time::Instant` in code that runs on the web (`web_time` instead), no filesystem outside the host's storage trait, and `cargo xtask wasm` and `cargo xtask web-offline-test` cover it. A web page never fetches an arbitrary URL on a package's behalf; the only URLs it reads are the index's own.

## 12. Clean-room and licence rules for listed items

Listed and Verified items are shown under our name, so they must follow the rules the project follows. The rules are enforced by the index build for Listed and by review for Verified. Custom is not ours to enforce, and the app says that.

**Licence**
- The item's code and data are under an open licence from an allowed list: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unlicense, CC0-1.0, and for data and artwork CC-BY-4.0 with its attribution kept. MPL-2.0 is also allowed for Listed and Custom items, and never inside SolidCraft itself.
- No GPL, LGPL or AGPL, and no dependencies under those licences in a package we list, whether or not they are linked: AGENTS.md rules out GPL code in the project, and a listed plugin is something we put in front of users as part of it. The index build scans a plugin's dependency lock file when the source is public (the same check as `cargo xtask licences --check`, run on the author's repository), and a reviewer checks it for Verified. A licence that restricts commercial use, derivatives or redistribution (for example "non-commercial" or "no derivatives" terms) is refused: the web mirror needs redistribution, and users need to be free to use the work.
- The licence in the listing must match the licence file in the repository and the SPDX identifier a package declares. Mismatch fails the build.

**Independence from commercial CAD**
- The item must not contain, copy or derive from another product's code, icons, artwork, fonts, help text, tooltips, templates or materials. This is the first rule in AGENTS.md and applies to what we list.
- It must not be built from data extracted from commercial CAD products, nor by driving or scripting them: no geometry, parameters, command lists or ids, test expectations or measurements taken from them. The publisher states this in the listing and the review asks.
- Screenshots in the item's page are of the item running in SolidCraft, never of other products.
- Import and export converters follow public file-format specifications and the project's existing rule for formats.

**Assets and attribution**
- Every non-code file in a package (images, icons, fonts, sample designs, textures, tables) is original or openly licensed and has a row in the package's own `ATTRIBUTION.md` in the format the project uses: file, author, source, licence. The index build fails a package with an unlisted asset, the same way `cargo xtask assets` does for this repository.
- Icons follow the project's icon rules for anything shown as part of SolidCraft's own interface; a plugin's icon passes the existing image checks.
- Fonts and shaders count as assets.

**Vendor data and part generators**
- Dimensions, ordering codes and tolerances are facts and may be used when taken from a public datasheet or standard, cited in `sources`. A generator must not embed a vendor's CAD files, catalogue pages, logos or text unless the vendor's licence allows redistribution under an allowed licence.
- A generator names the vendor in its description only to say what its codes are for, plainly, with no logo and no wording that suggests the vendor made or approves it. The vendor's trademark is theirs.
- Generated geometry comes from the generator's own code and the cited dimensions, never from a downloaded model.

**Names and wording**
- The product is called SolidCraft. An item may not use our name or logo to suggest it is ours, except the "Verified" mark we give it.
- Plain English in listings and the approval screen, with the terms from the project's glossary.

**If a rule is broken.** A report in the "licence or clean-room concern" category is handled first, before ordinary reports. The result is delisting or revocation (section 9.1), the same for Verified and Listed.

## 13. Where it goes in the interface

**What exists.** The Start page is drawn by one shared component, `ui_kit::start_page` (`crates/ui-kit/src/start_page.rs`): a **rail** of entries on the left beside a titled body. SolidCraft's rail (`rail()` in `crates/ui-egui/src/home.rs`) lists Start, New, Open, Recover, Save, Save As, Export, Properties, Settings, Shortcuts, Crash reports and About, then the support and community links, in groups divided by separators. Choosing a rail entry that has a `Section` (Start, Recover, Export) swaps the body. The Start body, built by `start_body()`, shows New tiles, Recent designs and then **Samples** (title `home.samples`), the built-in sample designs with their pictures. Items are chosen through one `FileAction` that the host runs. Plugins are managed in Preferences > Plugins (`crates/ui-egui/src/plugins.rs`), and the install confirmation is `crates/ui-egui/src/plugin_install.rs`.

**Proposal.** Samples is the last block of the Start body, so the page has room beneath it, and the rail has room after its last groups. We use both, with one page behind them:

1. **A Marketplace rail entry** in `rail()`, between Recover and the design entries (or just above Settings; a layout question for the review of the picture), that opens a new `Section::Marketplace` whose body is the browse page. The browse page has a search box, kind filter chips (Plugins, Generators, Themes, Materials, Samples, Templates), a tier filter, and cards. Each card shows the picture if cached, name, publisher, the one-line summary, a tier badge, and Install, Update or Installed.
2. **A "Get more" row under Samples** on the Start body: a short row of the most relevant items (for example sample designs from the index, matched to Samples, and "Browse all"), so a person who looks at Samples finds more of them. It is the same cards, drawn by the same code.
3. **An item page** in the same body, with the long description, screenshots, what the item adds, requested permissions in the approval screen's words, versions, licence, source link, tier, "Report a problem", and the install button. Install opens the existing confirmation screen, with the tier header (section 6).
4. **Add a source** (Custom) is a field on the browse page: type `owner/repo` or a URL, the app looks the repository up and shows the same item page with the Custom banner. Added sources are listed on the marketplace page with a remove button.
5. **Installed and Updates** is one more view on the same page (Installed, Updates available, Revoked), which links to Preferences > Plugins for enable, disable and the plugin's own settings, which stay where they are.

The marketplace page's own settings (index source, "show pre-releases for this item", what the update check does) live on the page, in one small dialog, never as new rows in Preferences, in line with how options are placed in this project.

**Rules this follows (RFC 0015's shared rules):** the host draws every widget with theme tokens, so no item can draw its own page; every new widget exists in the web build before it is used; each widget kind and the new rail entry go through the one-time review with 1280x720 pictures; screenshots for the PR include a German locale one beside the English one, in landscape; all strings go in `assets/locales/pending/<slug>.json` with `cargo xtask lists`-sorted entries; the one outer UI container and the one camera path are untouched; no hard-coded colours.

**Everything is a command.** Browsing and installing have command forms with input and output schemas, so an agent or a script does what the page does, with no dialog:

| Command | Does |
|---|---|
| `market.search` | `{query?, kind?, tier?}` returns cards from the cached snapshot |
| `market.show` | `{id}` or `{repo}` returns the item page data for an index item or a Custom source |
| `market.install` | `{id | repo, version?, approval, archive, approve?, accept_unreviewed?}` the same dry-run, pin and approve rules as `plugin.install`, with the index's pins checked |
| `market.updates` | Lists available updates and held-back ones, and why |
| `market.refresh` | Fetches and verifies a new snapshot (the one network action; refuses if checks are off by policy) |
| `market.sources` | Lists, adds or removes Custom sources |
| `market.report` | Returns the URL a report would open, without opening it |
| `generator.find` | `{code}` returns matching generators |

Installing and removing still run as separate `engine.execute` requests as `plugin.install` does today.

## 14. Phases

| Phase | Delivers |
|---|---|
| 0 | The `craft-market` crate: index and listing types and schema, signature verification, snapshot rules (sequence, expiry), the compatibility chooser, tests with fixed snapshots. `solidcraft-cli market check <dir>` for authors. The index repository with its build workflow, empty. |
| 1 | The Marketplace rail entry and browse, item and installed pages on desktop. Listed and Custom install through the existing install flow, with tier headers and the second step for high-risk grants. `market.*` commands. Index-based update checks. Themes and shaders as the first content packs. |
| 2 | Verified tier: signed snapshot in releases, developers list, review checklist, attestation shown on the page, revocation list applied at start and daily. |
| 3 | Content-pack sections `materials`, `samples`, `templates`, `tables`, each its own small RFC or issue; the "Get more" row under Samples fills with sample designs. |
| 4 | Part generators: the `generators` declaration, `generator.find`, the insert-by-code form, design records and fallbacks. Index checks for `sources`. |
| 5 | Web build: browse, then content packs from the Pages mirror, then plugins and generators with RFC 0002 phase 5. |
| 6 | Whatever people ask for: ratings, in-app attestation checks, a catalog-only generator, an automatic-update choice. |

Phases 0 and 1 are one topic each and one PR each, per the project's rules. A phase is done when its tests pass, the screenshots (English and German, landscape) are in the PR, and, for phase 0, the schema and the signing choices were reviewed in the PR the way RFC 0002 reviews its phase 1.

## 15. Open questions

Two questions are for the maintainers:

1. **Who reviews Verified items, and who holds the offline signing key?** Verified needs a named, small review team, a written checklist and a key policy.
2. **What is the index repository called, and in which organization?** `tangency-labs/solidcraft-index` is a placeholder. A separate organization would let the community own listings.

**Defaults this RFC takes.** The maintainers can override any of these in the pull request.

- **Where it goes.** The Start page's left area under Samples, as found on main: the rail on the left gets a Marketplace entry, and the area under the Samples block gets a "Get more" row (section 13). If a newer left sidebar replaces the Start page rail, the same two places move with it.
- **MPL-2.0** is allowed for Listed and Custom items, and never inside SolidCraft itself.
- **Vendor data.** Dimensions and ordering codes from public datasheets are usable facts. Vendor CAD files and logos are not (section 12).
- **Ratings** are facts only in v1 (section 9).
- **Per-author signing keys** come later. v1 identifies trusted developers by their GitHub account or organization (section 7).
- **The Home Assistant mapping** is as in section 2: integrations are plugins, community content is content packs, add-ons are not copied.
- **Also decided here, open to comment:** build attestations are recorded and checked in the index build and in review, not inside the app, in v1; Custom is GitHub only in v1; the "could not check" notice for a stale revocation list appears after 30 days; the high-risk grants in section 6 start with folder writes, the `file` group, settings, other plugins and raised limits; the new content-pack capability names and formats are placeholders for their own proposals.
