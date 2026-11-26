# RFC 0017: the Workshop, a marketplace for plugins, generators and themes

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-25 |
| Apps | SolidCraft (the index and the shared client crate are app-agnostic, so other Craft apps can use them) |
| Discussion | [#22331](https://github.com/tangency-labs/solidcraft/issues/22331) |
| Extends | [RFC 0002](0002-plugins.md) (plugins; this RFC is its phase 6, "curated, signed index") |
| Related | [RFC 0015](0015-plugin-ui-workspaces.md) (plugin UI), [#20425](https://github.com/tangency-labs/solidcraft/pull/20425) (`min_host_version` and `optional_capabilities`), [#5586](https://github.com/tangency-labs/solidcraft/pull/5586) (install from a GitHub link), [#7558](https://github.com/tangency-labs/solidcraft/issues/7558) (extras and accessory packs: what users want), [RFC 0018 / #22338](https://github.com/tangency-labs/solidcraft/pull/22338) (Send to App; the same `solidcraft://` link handler) |

The **Workshop** is the Workshop we propose: a place inside SolidCraft, and a website, where people find and install community work. People should be able to find a plugin, a part generator or a theme inside SolidCraft, and to install one from a button in its repository, read who made it and what it may do, and install it in a few clicks, without us shipping every one of them and without us pretending to have vetted code we have not. This RFC says how, in three trust tiers, on top of the sandbox, permissions and approval screen RFC 0002 already built.

Today there is a plugin runtime, an install flow from a `.zip`, a "Update to x.y.z" check against a plugin's own GitHub releases, and Preferences > Plugins to enable, disable and remove. There is no place to discover anything, no signing, no list of who is trusted, and no way to pull a bad plugin back.

**One idea runs through every section: the Workshop adds discovery and trust, never power.** An installable from the Workshop runs in exactly the sandbox, with exactly the permissions and the same approval screen, as the same package installed from a file. The tier changes what we tell the user and how much friction the riskiest grants carry, not what the sandbox allows. The one exception is the reserved `net` permission, which RFC 0002 already limits to reviewed, signed plugins.

## Decisions

These are proposed here and are open for comment until the RFC is accepted.

1. **Three kinds of installable:** plugins, generators and themes (section 2). Each is a package with one manifest. What a package may contain decides its kind; there is no second packaging format. More kinds (materials, sample designs, templates, tables) come later, each as its own kind.
2. **Three trust tiers:** Verified (reviewed and signed), Listed (in the index, checked by machine only, not reviewed) and Custom (any GitHub `owner/repo` or URL the user types) (section 3). The user always sees which tier an item is in.
3. **The index is a public Git repository** of small JSON files, built into one signed snapshot. SolidCraft downloads the snapshot and reads it. It never calls a server of ours that holds user data (section 4).
4. **Artifacts stay on GitHub Releases** in the author's own repository, as RFC 0002 section 5 already prefers. The index points at them and pins their hashes. We do not host packages in v1.
5. **Everything needs the existing approval.** Nothing runs before the user approves the exact manifest and component (`package_hash`). A Workshop install never auto-approves, and a package whose manifest or component changed asks again (section 6). Other files are pinned by the archive hash (section 6).
6. **Versions follow what #20425 built.** The index shows, and the installer picks, the newest release whose `min_host_version` and required capabilities this SolidCraft meets (section 5).
7. **Delisting and revocation are different.** Delisting hides an item from browse. Revocation switches an installed package off (section 9.1).
8. **Offline is the normal case.** An installed package works with no network. The index is cached. Browsing offline shows the cache with its date (section 10).
9. **Listed items follow our clean-room and licence rules** (section 12). Custom items are the user's own risk, and the app says so.
10. **No new Preferences rows.** Workshop settings live on the Workshop's own page.
11. **An "Install in SolidCraft" button** in a repository opens the item's install screen in the app through one opt-in `solidcraft://` handler, shared with RFC 0018. Nothing installs without a click in the app (section 13.1).

## 1. Goals and non-goals

**Goals**
- Browse, search, install, update and remove community work from inside SolidCraft, on desktop first.
- Let a person who does not know what a WebAssembly sandbox is tell a reviewed item from an unreviewed one at a glance.
- Let an author publish by adding one small file to their repository and opening one pull request, with automated checks that tell them what to fix.
- Let an agent do all of it over the command surface, with no dialog (everything is a command).
- Keep the user's designs opening without the Workshop and without the network (RFC 0002: SolidCraft never refuses to open a file).

**Non-goals**
- No payments, licences keys, paid listings or advertising. a free Workshop only.
- Not a replacement for core features. If many people need something, it belongs in SolidCraft itself.
- Not hosting. We do not store or serve authors' packages in v1 (the web build is the one question, section 11).
- Not a social network. No accounts, comments or follower counts of our own in v1.
- No native executables, no separate processes and no containers. Home Assistant's add-ons, which run as separate programs beside the host, are the part of that model we deliberately leave out: a SolidCraft installable is sandboxed WebAssembly or data, and nothing else.

## 2. The three kinds

We took Home Assistant's split as the model: code that runs inside the host and is described by a manifest the host reads before loading any code (its integrations); heavier things that run beside the host (its add-ons), which we do not copy; and the community content that a store such as HACS also carries, which is data and configuration (themes, templates, dashboard parts). SolidCraft starts with three kinds, plugins, generators and themes:

| Kind | What it is | Runs code | Exists today |
|---|---|---|---|
| **Plugin** | Commands, importers, exporters, panels, workspaces: a sandboxed WebAssembly component with a `runtime` in its manifest (RFC 0002, RFC 0015) | Yes | Yes, in `crates/craft-plugin` |
| **Theme** | Data only: colour themes and the view backgrounds and tints that go with them | No | Yes (`themes@0.1`, `shaders@0.2`, a package with no runtime) |
| **Part generator** | Builds a part from a vendor ordering code or from parameters (section 2.3) | Yes (a plugin with a `generators` declaration) | No |

A package may be more than one thing, as a plugin that ships its own theme already may. The Workshop shows its **primary kind** (the first that applies in the order generator, plugin, theme, so a plugin that declares `generators` is shown as a generator) and lists the rest as contents.

### 2.1 Plugins

No change to the package. The Workshop adds a listing around it (section 4.2). A plugin keeps every field RFC 0002 defines: `id`, `name`, `version`, `capabilities`, `runtime` with entries, permissions, limits, listeners and slots.

### 2.2 Themes

A theme is a package whose manifest has no `runtime` and one or both of two data sections, each behind a capability the host already serves:

| Section | Capability | Contents |
|---|---|---|
| `themes` | `themes@0.1` (exists) | Colour themes for the theme picker |
| `shaders` | `shaders@0.2` (exists) | View backgrounds and tints, shown with the theme |

A theme is data: no code, no network, nothing to grant. Its approval screen is short: name, publisher, licence, what it adds, and nothing to grant. A host that predates a section refuses the package with a clear reason ("needs a newer SolidCraft"), because the manifest keeps its strict field set (`deny_unknown_fields`) and an unserved capability is refused before anything is used; the index records each release's required capabilities and `min_host_version`, so the Workshop does not offer such a package to an older host in the first place.

**Later kinds.** Materials and appearance libraries, sample designs (shown on Start under Samples), drawing and design templates, and standard-size tables for the profile and fastener commands are not part of this RFC's first phases. Each becomes its own kind, with its own capability (`materials@0.1`, `samples@0.1`, `templates@0.1`, `tables@0.1` are placeholder names) and its own small proposal. A new kind follows the rules a theme follows: it is data, it adds no code, the host checks it against schemas and size caps before use, an older host refuses it with a reason, and the three tiers, the index, the install link (section 13.1) and the commands handle it with one new value of `kind`. The Workshop does not wait on them.

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
| Updates | Offered once that release is reviewed; a diff review for trusted developers (section 8) | Offered, with the changed-permission rule | Offered, with the changed-permission rule |
| Index signature covers its hash | Yes | Yes | Not in the index |
| Revocable by us | Yes | Yes | Only if its hash or repository is on the revocation list |

**Verified.** The review reads the source and the component's build (section 7), the manifest's permissions against what the code needs, and the clean-room checks (section 12). A "trusted developer" is a GitHub account or organization on the index's developers list, added after at least one reviewed plugin and an agreement to the publisher rules. Every Verified release is reviewed: the badge means this release was read, not only that its publisher is trusted. A trusted developer gets a faster review (a maintainer reads the diff against the last reviewed release, not the whole source) and a first listing still gets the full one. Until a new release is reviewed, the previous reviewed release stays the one offered, and the new one shows as an update that is not yet reviewed. This is the HACS idea of a default list that maintainers curate, with the Home Assistant idea that custom integrations are not supported by the project, turned into an explicit label.

**Listed.** The name is honest: the index lists it, we have not vetted it. Automated checks cover the manifest, version rules, licence, attribution, sizes, a built component that matches the listing, and a scan of the package for banned content (section 12). They cannot prove behaviour.

**Custom.** The user adds a source, as HACS lets a person add a custom repository. We accept GitHub only in v1 (`owner/repo`, `owner/repo@tag`, or an `https://github.com/owner/repo` URL), because the release API gives us a digest to check (section 7). Other hosts need a reason to trust their digests and are left to a later proposal; a `.zip` from anywhere still installs from a file. The same install checks, the same approval, and a clear banner. The app never lists a Custom source in search. The user's added sources are kept in the Workshop page and can be removed.

## 4. The index and the listing

### 4.1 The index

One public repository, `tangency-labs/solidcraft-index` (name to confirm), holds:

- `verified/<id>.json` and `listed/<id>.json`: one small file per item (section 4.2).
- `developers.json`: trusted developers.
- `delisted.json`: items hidden from browse and updates (section 9.1). Records are kept, never dropped, so a delisted item does not fall back to the per-repository update check.
- `revoked.json`: revocations (section 9.1).
- A build workflow that validates every file, fetches each item's releases, checks them (section 4.3), and writes one `index.json` plus a detached signature `index.json.sig`, published as a release asset and at `https://workshop.solidcraft.io/index.json` and `index.json.sig`.

SolidCraft downloads `index.json` and `index.json.sig`, verifies the signature with public keys built into the app (section 7), and reads the verified snapshot. A snapshot has an issue time, a sequence number that only increases, and an expiry. The app refuses an older sequence than it has already seen, so an old snapshot cannot be replayed to hide a revocation.

The index is small: id, name, short description, kind, tier, publisher, licence, repository, tags, and per release the version, `min_host_version`, required and optional capabilities, size and hashes. It carries no package bytes and no pictures (a mirror may add `asset_url` and picture hashes, section 10). Pictures are loaded when the user opens an item's page (section 10).

The index schema is generated from Rust types in a shared crate, `craft-workshop` (no SolidCraft dependency, builds for wasm; `cargo xtask layers` forbids otherwise), the way the plugin manifest schema is generated from `craft-plugin`.

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
- A listing for a theme names its sections in the build output, not the listing. The listing's `kind` is `plugin`, `generator` or `theme`.

### 4.3 What the index build checks

Failing any check fails the pull request with a message that names the field and how to fix it. These are machine checks, not a review:

1. The listing parses and the id is unused or is the same repository's.
2. A stable release exists, tagged as a `major.minor.patch` version, with an asset `<id>.zip` or `<id>-<version>.zip` and a GitHub-listed sha256 digest (the rule `updates.github` already uses).
3. The package passes `plugin check` (and the theme equivalent): manifest, schemas, size caps (`MAX_MANIFEST_BYTES`, `MAX_COMPONENT_BYTES` and the rest of `craft-plugin`), safe archive rules, an icon that passes the image checks.
4. The manifest's id, version and `updates.github` agree with the listing and the tag.
5. The licence in the listing is on the allowed list and matches the repository's licence file (section 12).
6. Every non-code asset has an attribution row (section 12).
7. For a generator, `sources` is present.
8. The repository is public, not archived, and has issues enabled, so reports have somewhere to go.

Every release the build sees is re-checked on every run, so a release whose digest later changes is dropped from the snapshot.

## 5. Manifests, versions and compatibility

**Per kind, what the host reads.** The installed package's manifest is the one in `crates/craft-plugin`: `id`, `name`, `version`, `min_host_version`, `description`, `capabilities`, `optional_capabilities`, `runtime`, `shaders`, `themes`, `updates` and the inert `extensions`. A plugin fills `runtime`; a theme fills only its data sections; a generator fills `runtime` and `generators`. The listing (section 4.2) adds the Workshop-only facts. Nothing in the listing can widen what the package may do.

**`min_host_version`.** A complete semantic version, the oldest SolidCraft the package supports. The host already refuses a package that needs a newer SolidCraft, with that reason, before publishing it. The index build records it for every release.

**`capabilities` and `optional_capabilities`.** Required capabilities must be served by this host, or the package is refused before any code runs. Optional ones are granted when supported and the plugin must provide a fallback. The index records both lists for every release, as `name@major.minor`.

**Choosing the version.** For an item with several releases, the Workshop offers the newest release that this SolidCraft can run: `min_host_version` is not newer than the host, every required capability is served, and the release is not revoked. If the newest release needs a newer SolidCraft, the item page says so ("version 2.1 needs SolidCraft 0.54; version 2.0 is offered"), the way HACS lets a person install an older version when Home Assistant is too old for the latest. The installer re-checks on download and never trusts the index's reading of compatibility, because the host's own check is the authoritative one.

**Version rules.** Package versions are strict `major.minor.patch`, as the update check already requires. A pre-release is never offered unless the user turns on "show pre-releases" for that one item. A listed version cannot be reused with different contents: the index pins the hash of each version, and a hash mismatch is a revocation reason, not an update.

**API compatibility.** The WIT version the package was built for is part of its required capabilities (`commands@0.1` and so on), so a package built for an older API keeps loading, as RFC 0002 requires. The index shows the oldest and newest host versions each release runs on, and the author is told at listing time when a host release is about to drop a capability their package needs (a deprecation list in the index).

**Theme format versions.** Each data section carries its capability version, so a newer theme is refused by an older host with a clear message, never half-loaded.

## 6. Permissions and the sandbox, per tier

The sandbox and the permission model do not change. This section only says how the tier shapes what the user sees and what extra care applies.

What exists and stays exactly as it is (RFC 0002, `docs/plugins.md`): permissions in the manifest (`files` folders with read or write, `clock`, command groups, `input_folder`, the reserved `net`), fuel, memory, stack and deadline limits and the app's ceilings, the approval screen showing what is requested, folders chosen by the user with Browse, approvals stored per plugin id, and the approval bound to `package_hash`, the hash of the exact manifest and component (not images or other data files, by design). The `archive` hash covers the whole `.zip`, including those files, and the Workshop checks it at install and update. A changed manifest or component, even one edited in place, runs only after it is approved again. Approvals can only narrow the manifest's requests.

**What the tier adds**

| | Verified | Listed | Custom |
|---|---|---|---|
| Approval screen header | Publisher, "Verified", licence, source link | Publisher, "Not reviewed", licence, source link | Repository, "Not in the index, not reviewed", licence as GitHub reports it |
| High-risk grants (write access to a folder, the `file` command group, application settings or other plugins' command groups, fuel or memory above the app's default ceilings, `input_folder`) | One confirmation | A second step: the user must tick each high-risk grant on its own line | Same as Listed |
| Install with no folder, no command group and default limits | One confirmation | One confirmation | One confirmation |
| Headless install (`plugin.install` and the new `workshop.*` commands) | Needs `approval` and `archive` from a dry run, as today | Same, plus `"accept_unreviewed": true` | Same |
| Theme | Nothing to grant | Nothing to grant | Nothing to grant |

A second step for the riskiest grants on unreviewed items is the only added friction, so a harmless theme or a read-only importer installs in one click while a plugin that wants to write to a folder makes the user stop and look.

The approval screen's wording is plain: what the item can do, in the words the screen already uses (files, designs, application settings, other plugins). The tier line is never hidden inside a tooltip.

**Not changed, and worth saying again:** there is no network for plugins, a manifest asking for `net` is refused, and Custom and Listed items can never get it. The Workshop itself talks to the network, not the plugin (section 10).

## 7. Signing and provenance

Three separate things need to be true: the index is ours, the package is the one the index described, and the package is what the author built from the source they showed us.

1. **The index is signed.** `index.json` has a detached signature with a project key (ed25519, with a permissively licensed Rust implementation, which the new dependency's PR names as the ground rules ask). The public keys are compiled into SolidCraft, with room for more than one so a key can be rotated: a new key is added to the app, the old one retired after a release cycle. The signing key is kept offline and used by the release process, not by the index repository's workflow, which uploads the unsigned file for a maintainer to sign. A signature that does not verify means the app uses its last good snapshot and says so. **An organization's own index.** An organization that mirrors the index signs its modified snapshot (with its mirror URLs and hashes added) with its own ed25519 key, and its policy sets `index_url` and lists that public key as the trusted key. In that mirror mode the app trusts only the organization's key, not ours, and the organization decides what is in the snapshot. The organization may re-sign our entries unchanged or drop some. The sequence number and expiry rules apply to the organization's snapshots the same way.
2. **The package matches the index.** The index pins the `archive` sha256 of every offered release, and the `package_hash` the reviewed manifest and component give. The download is checked against the hash GitHub lists for the asset and against the pin; both must match. In mirror mode (section 10) the GitHub digest is not consulted, because the app makes no request to GitHub; the pinned hash in the organization-signed index is the check. For Custom there is no pin, so GitHub's digest over TLS is the only check, which is the check the update flow already makes.
3. **The package was built from the source.** Verified requires that the release asset is built by public CI from the tagged commit, and records the repository, commit and workflow in a build attestation published with the release (GitHub's artifact attestations or an equivalent signed statement). The review reads the workflow. Verifying attestations inside the app is not in v1 (it needs a verifier we would have to trust and ship); v1 records them in the index, shows them on the item page, and checks them in the index build and in review. Verifying them in the app is a later phase.
4. **Source must be public and reproducible enough to read.** Verified plugins are open source (RFC 0002 section 5 already says so). A component with no public source is not eligible.
5. **Developer keys.** Trusted developers are identified by their GitHub account or organization in `developers.json`, not by a key of theirs. Per-author signing keys come later; they add a second thing to lose and a second thing to revoke.

The approval hash the user sees binds the manifest and the component: if the manifest or component changes after review, `package_hash` changes, the stored approval no longer matches, and the plugin stops until approved again, the existing behaviour. `package_hash` does not cover other package files, such as a theme's data files or images. Those are pinned by the `archive` hash in the index at install and update time, and a content pack grants nothing, so an edited data file on disk gains no permission. Whether to hash data files into the approval too is a design question for the content-pack phase (phase 3), and would update `docs/plugins.md` with it.

What this does not give: it does not prove a verified plugin is free of bugs or of a later malicious update that passes review. That is why Verified updates that change permissions or entries are reviewed again, and why revocation exists.

## 8. Updates

The update flow RFC 0002's `updates.github` field and `docs/plugins.md` describe is the base, and stays: a daily and at-start check, one request per plugin (at most 20), a stable release with a version tag newer than the installed one, an asset with a GitHub-listed digest, a download into the hidden updates folder, the same confirmation as install, an undoable replacement with no restart, folders kept by the existing rules.

What the Workshop adds:

- **One index request replaces many.** For an item that is in the index, the check reads the signed snapshot (one small download) instead of calling GitHub once per plugin. Custom items still use the per-repository call. A delisted item is not "missing": the snapshot carries a signed `delisted.json` record for it (section 9.1), and an installed package that matches a record (by id and repository) gets no update offers from either path. Only a package with no index entry and no delisting record is treated as Custom. The update setting that turns checks off, and the organization's policy over it, turn this off too.
- **Newest compatible, not newest.** The offered update is the newest compatible release (section 5), and the card says when a newer one is held back and why.
- **Verified updates: diff review, not silent trust.** Every Verified release is reviewed before the index admits it with the Verified badge, as in section 3. The review is lighter for a trusted developer when the declared permissions, command groups, entries, slots and limits equal the previous reviewed release's (compared on the manifest): a maintainer reads the diff of the code. A release that changes any of them gets the full review. Until a release is reviewed it is not admitted as Verified: it is shown as an update that is "not yet reviewed" and the user is told, and the previous reviewed release stays the one offered. RFC 0002's rule stands: a code-only update never adds an entry or surface silently, and a change in what was approved stops the plugin until the user approves again.
- **Never silent.** Updates are offered, never installed without the user's click and the confirmation. There is no auto-update in v1. A later "update trusted plugins automatically" choice, if people want it, is a separate decision with its own page, never a Preferences row.
- **Rollback.** The previous version is kept until the next update, so "Restore previous version" works offline. It is the same package and the same approval hash, so nothing new runs.
- **Themes** update the same way. A package that adds a theme and a plugin updates as one package.

## 9. Ratings and reports

**v1 does not collect ratings.** We do not run accounts, and a star count we cannot defend (fake votes, review brigades) would be worse than none. Instead the item page shows facts the index build already has: the publisher, the licence, the date of the last release, how many releases, the compatible host versions, the repository's open-issue count and star count as reported by GitHub on the day the snapshot was built (labelled as GitHub's, with that date), and whether the item is Verified.

**Reports.** Every item page has **Report a problem**. It opens, in the user's browser, a pre-filled GitHub issue form on the index repository with the item id, version and category, and nothing else from the app (no design, no path, no machine details unless the user adds them). Categories: it does not work, it behaves badly, it asks for too much, licence or clean-room concern, security. A security category points to a private way to report (GitHub's private vulnerability reporting on the index repository) rather than a public issue. The app sends nothing itself; the user's browser does.

**Handling.** The maintainers triage reports. A licence, clean-room or security report can lead to delisting or revocation (section 9.1). A report about behaviour goes to the author's repository. Reports that name an author are answered in public on the issue.

**Later.** Ratings may come back as "works for me" counts if we find a way to count one person once without accounts. That is not a promise.

### 9.1 Delisting and revocation

Two actions, kept apart, because one hides and the other stops.

- **Delisting** removes the item from browse and search and from "Update available". Already-installed copies keep working, and keep their approvals. Used when an item is abandoned, the author asks, or the licence or listing rules are no longer met and no harm is involved.
- **Revocation** is for harm: malware, data theft, a sandbox escape, a clear licence or clean-room violation, a replaced package whose hash no longer matches what was reviewed. `revoked.json` lists records, each with exactly one selector (a package `id` with a version range, an exact `archive` hash, or a `repo` in `owner/repo` form), a reason, a severity and a date. A `repo` selector matches any installed package whose `updates.github` or recorded source repository equals it (case-insensitively, as GitHub names are), so it also catches a Custom source and any new release from that repository.
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
- **Pictures and long descriptions** are fetched when the user opens an item's page, from the author's repository over `https` (or, in mirror mode, from the mirror: the mirror serves them at the URLs its signed snapshot names, with hashes), and cached with a size cap. A missing picture never blocks anything. The app draws everything itself: text from the listing and README is shown as plain text, with no links that run, and no remote content is rendered as markup. This keeps the page from being a way to put anything on screen.
- **What the app sends.** To read the index: a plain `GET` for two files, with no identifiers. To download an item: a plain `GET` of the release asset from GitHub. Nothing else, and no usage counts. The same controls as the update check govern it: the update setting, and an organization policy that can turn it off, or point the app at the organization's own mirror of the index (an `index_url` the policy sets). For a mirror that keeps traffic inside the organization, the index schema lets each release carry an optional `asset_url` and each item optional picture entries with a sha256, and the app uses those instead of the author's GitHub URLs. The download is still checked against the same pinned `archive` hash, and a picture or description against its hash. We do not host anything: the organization does. **In mirror mode the app makes no request outside the mirror:** packages, pictures and long descriptions all come from the mirror, and the GitHub digest check is replaced by the pinned hash in the organization-signed snapshot. Custom GitHub sources are off in mirror mode. The one exception is an explicit policy opt-in that allows them, and with it on, the app does contact GitHub for those sources only (the no-requests-outside-the-mirror rule is stated as broken for exactly that case, and the user sees the Custom banner as always). Mirror mode fails closed: if a release or picture the app needs has no mirror URL or hash in the snapshot, the install or the picture fails with a message naming the missing field, and the app never falls back to GitHub. A mirror of the index alone is therefore not a private Workshop; the organization must fill those fields.
- **Air-gapped use.** `solidcraft-cli` can verify and install from a downloaded snapshot and package files (`workshop install --from <dir>`), so a machine with no network can still use a reviewed set carried over on a drive.

## 11. The web build

The browser has no plugins folder, and RFC 0002 leaves plugin hosting in the web build to its phase 5. The Workshop follows it, in three steps:

1. **Browse only.** The snapshot is a static file; the index build publishes it with permissive CORS headers at `workshop.solidcraft.io`, so the web build can read it, verify the signature (the verifier is plain Rust and builds for wasm) and show items. Install is replaced by "Open in the desktop app" and by a link to the item's repository.
2. **Themes.** A theme is data, as later kinds will be. It can be stored in the browser's own storage and used. GitHub's release downloads do not allow cross-origin reads from a page, so themes for the web are mirrored by the index build onto the Workshop site, with their hashes in the signed snapshot. This is redistribution, which is why Verified and Listed themes must carry an open licence that allows it (section 12). Custom items cannot be installed on the web.
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

1. **A Workshop rail entry** in `rail()`, between Recover and the design entries (or just above Settings; a layout question for the review of the picture), that opens a new `Section::Workshop` whose body is the browse page. The browse page has a search box, kind filter chips (Plugins, Generators, Themes; each later kind adds a chip), a tier filter, and cards. Each card shows the picture if cached, name, publisher, the one-line summary, a tier badge, and Install, Update or Installed.
2. **A "Get more in the Workshop" row under Samples** on the Start body: a short row of the most relevant items (for example the newest Verified items of any kind, and "Browse all"; when a samples kind exists, sample designs from the index), so a person who looks at Samples finds more of them. It is the same cards, drawn by the same code.
3. **An item page** in the same body, with the long description, screenshots, what the item adds, requested permissions in the approval screen's words, versions, licence, source link, tier, "Report a problem", and the install button. Install opens the existing confirmation screen, with the tier header (section 6).
4. **Add a source** (Custom) is a field on the browse page: type `owner/repo` or a URL, the app looks the repository up and shows the same item page with the Custom banner. Added sources are listed on the Workshop page with a remove button.
5. **Installed and Updates** is one more view on the same page (Installed, Updates available, Revoked), which links to Preferences > Plugins for enable, disable and the plugin's own settings, which stay where they are.

The Workshop page's own settings (index source, "show pre-releases for this item", what the update check does) live on the page, in one small dialog, never as new rows in Preferences, in line with how options are placed in this project.

**Rules this follows (RFC 0015's shared rules):** the host draws every widget with theme tokens, so no item can draw its own page; every new widget exists in the web build before it is used; each widget kind and the new rail entry go through the one-time review with 1280x720 pictures; screenshots for the PR include a German locale one beside the English one, in landscape; all strings go in `assets/locales/pending/<slug>.json` with `cargo xtask lists`-sorted entries; the one outer UI container and the one camera path are untouched; no hard-coded colours.

### 13.1 The "Install in SolidCraft" button

A repository's README can show a button that opens the item's install screen in SolidCraft, ready to go. We took the idea from the "open your instance" badge that Home Assistant's own link service offers: a badge links to a static web page that hands the visitor to their own installation. Only the idea is used here.

**The badge.** The project hosts one badge image, original artwork with a row in ATTRIBUTION.md, as a static file. The author copies one line of Markdown into their README:

```markdown
[![Install in SolidCraft](https://workshop.solidcraft.io/badge/install.svg)](https://workshop.solidcraft.io/install?repo=owner/repo&kind=plugin)
```

The proposed home is `https://workshop.solidcraft.io`, which also hosts the browsable listing and the signed index snapshot. Who owns and runs it is a maintainer question (section 15). A listed item may use `id=` in place of `repo=`. An author who prefers the other wording may use the alt text "Open in SolidCraft Workshop".

**The redirect page.** `https://workshop.solidcraft.io/install` is one static page.
- It has no tracking, no login, no cookies, no storage, no third-party script, font or image, and no server code: the parameters are read in the visitor's browser and never sent anywhere. It is served with `Referrer-Policy: no-referrer` and a content security policy that allows only itself. The badge image is fetched from the same host, and we do not count its requests.
- It validates the parameters with the grammar below. An invalid link shows a plain message naming the problem and does nothing else.
- For a valid link it shows what will be opened (the repository or id, the kind, the version if given) and three things: **Open in SolidCraft** (a button, never an automatic redirect, that opens the `solidcraft://` link below), **Open in the web app** (section 11: today only themes can be installed there; for a plugin or generator the web app shows the item's details and says it needs the desktop app), and **Get SolidCraft** (the download page).
- If SolidCraft is not installed, nothing happens when the button is pressed. The page therefore also shows the manual steps: install SolidCraft, open the Workshop, choose Add custom source, paste `owner/repo`.

**The link.** The button opens `solidcraft://install?...` with the same parameters.

```
install-link = "solidcraft://install?" param *( "&" param )     ; each name at most once; order free
param        = repo / id / kind / ref
repo         = "repo=" owner "/" name        ; "/" may be written %2F
id           = "id=" 1*64( lowercase-letter / digit / "-" / "_" )
kind         = "kind=" ( "plugin" / "generator" / "theme" )
ref          = "ref=" 1*64( letter / digit / "." / "-" / "_" / "+" )
owner        = 1*39( letter / digit / "-" )      ; not starting with "-"
name         = 1*100( letter / digit / "-" / "_" / "." )     ; not "." or ".."
```

- Exactly one of `repo` and `id` is required, and `kind` is required. `ref` is optional and names a release tag; it must be a version tag (`v1.2.3` or `1.2.3`, as the update check requires). Without `ref`, the newest compatible release is used (section 5).
- `repo` always means a repository on `github.com`. A full URL, another host, a path beyond `owner/repo`, a query inside a value and any percent-escape other than `%2F` are refused. The owner and name rules are the ones the plugin manifest's `updates.github` check already uses (`valid_github_repo`).
- `id` must be a listed item's id, found in the cached index; an `id` the index does not know is refused. `kind` must equal the item's kind when the index knows it.
- Unknown parameters, repeated parameters, an empty value and a link over 256 bytes are refused, with a message that names the parameter. Nothing is fetched while a link is being checked: the check is syntax and the cached index only.

**One handler for both RFCs.** The `solidcraft://` handler that [RFC 0018](0018-send-to-app.md) proposes for reporting back from a local program is the same operating-system registration this RFC needs. We register it once. It dispatches on the route in the link (`app-report` for RFC 0018, `install` here), and each route has its own rules:
- RFC 0018's security rules apply to every route. The handler is opt-in (the installer asks, and the Workshop page has a button to register it later; no new Preferences row). Any web page or program can open such a link, so a link only ever asks the app to show something. It never runs a command, installs anything, opens a design or changes a setting. Links are rate limited (one pending install screen at a time, the rest dropped), and a link that fails validation is dropped with a message in the window.
- The `app-report` route is ignored when SolidCraft is not running, as RFC 0018 says. The `install` route may start SolidCraft, or bring the running window forward, because opening the install screen is the whole point of the button and it only shows information. That difference is stated in the route table of the handler, not left to each route.

**What the app does with a valid link.** It opens the Workshop's item screen, ready to go: the item's name, its kind, its tier (Verified, Listed or Custom, as the index says; a repository the index does not list is Custom), the version, the permissions it asks for, and **Install** and **Cancel**.
- For a Verified or Listed item the screen is filled from the cached signed index, so nothing is fetched to show it. **Install** downloads the package, checks it against the pinned hashes, and shows the existing approval screen with the package's own manifest. If the package asks for more than the screen showed, the app stops and shows the difference.
- A **Custom** link (a repository that is not in the index) gets a second confirmation: the first screen shows only the repository, the kind, the tier ("Custom, not reviewed") and a **Look up on GitHub** button. Only that click fetches the repository's release, and the full screen with permissions follows, with the second step for high-risk grants that section 6 already requires.
- Nothing installs without the click on Install, and an install from a link needs exactly the approvals an install from the Workshop needs.
- Links are commands too: `workshop.show` takes the same `{id}` or `{repo, kind, ref?}`, so a script does what the link does, with no dialog.

**Everything is a command.** Browsing and installing have command forms with input and output schemas, so an agent or a script does what the page does, with no dialog:

| Command | Does |
|---|---|
| `workshop.search` | `{query?, kind?, tier?}` returns cards from the cached snapshot |
| `workshop.show` | `{id}` or `{repo}`, with `kind` and optional `ref`, returns the item page data for an index item or a Custom source (what an install link opens) |
| `workshop.install` | `{id \| repo, version?, approval, archive, approve?, accept_unreviewed?}` the same dry-run, pin and approve rules as `plugin.install`, with the index's pins checked |
| `workshop.updates` | Lists available updates and held-back ones, and why |
| `workshop.refresh` | Fetches and verifies a new snapshot (the one network action; refuses if checks are off by policy) |
| `workshop.sources` | Lists, adds or removes Custom sources |
| `workshop.report` | Returns the URL a report would open, without opening it |
| `generator.find` | `{code}` returns matching generators |

Installing and removing still run as separate `engine.execute` requests as `plugin.install` does today.

## 14. Phases

| Phase | Delivers |
|---|---|
| 0 | The `craft-workshop` crate: index and listing types and schema, signature verification, snapshot rules (sequence, expiry), the compatibility chooser, tests with fixed snapshots. `solidcraft-cli workshop check <dir>` for authors. The index repository with its build workflow, empty. |
| 1 | The Workshop rail entry and browse, item and installed pages on desktop. Listed and Custom install through the existing install flow, with tier headers and the second step for high-risk grants. `workshop.*` commands. Index-based update checks. The Install in SolidCraft button: the badge, the redirect page and the shared `solidcraft://` handler (section 13.1). Themes (with their shaders) as the first data kind. |
| 2 | Verified tier: signed snapshot in releases, developers list, review checklist, attestation shown on the page, revocation list applied at start and daily. |
| 3 | Later kinds: `materials`, `samples`, `templates`, `tables`, each its own small RFC or issue; the "Get more in the Workshop" row under Samples fills with sample designs once a samples kind exists. |
| 4 | Part generators: the `generators` declaration, `generator.find`, the insert-by-code form, design records and fallbacks. Index checks for `sources`. |
| 5 | Web build: browse, then themes from the Workshop site mirror, then plugins and generators with RFC 0002 phase 5. |
| 6 | Whatever people ask for: ratings, in-app attestation checks, a catalog-only generator, an automatic-update choice. |

Phases 0 and 1 are one topic each and one PR each, per the project's rules. A phase is done when its tests pass, the screenshots (English and German, landscape) are in the PR, and, for phase 0, the schema and the signing choices were reviewed in the PR the way RFC 0002 reviews its phase 1.

## 15. Open questions

Two questions are for the maintainers:

1. **Who reviews Verified items, and who holds the offline signing key?** Verified needs a named, small review team, a written checklist and a key policy.
2. **Who owns and hosts the Workshop, and what is the index repository called?** We propose `https://workshop.solidcraft.io` for the website, the browsable listing, the signed snapshot and the install page, with the index repository `tangency-labs/solidcraft-index` (a placeholder name) behind it. A separate organization would let the community own listings.

**Defaults this RFC takes.** The maintainers can override any of these in the pull request.

- **Where it goes.** The Start page's left area under Samples, as found on main: the rail on the left gets a Workshop entry, and the area under the Samples block gets a "Get more in the Workshop" row (section 13). If a newer left sidebar replaces the Start page rail, the same two places move with it.
- **MPL-2.0** is allowed for Listed and Custom items, and never inside SolidCraft itself.
- **Vendor data.** Dimensions and ordering codes from public datasheets are usable facts. Vendor CAD files and logos are not (section 12).
- **Ratings** are facts only in v1 (section 9).
- **Per-author signing keys** come later. v1 identifies trusted developers by their GitHub account or organization (section 7).
- **The Home Assistant mapping** is as in section 2: integrations are plugins, community content is themes (and later kinds), add-ons are not copied.
- **Also decided here, open to comment:** build attestations are recorded and checked in the index build and in review, not inside the app, in v1; Custom is GitHub only in v1; the "could not check" notice for a stale revocation list appears after 30 days; the high-risk grants in section 6 start with folder writes, the `file` group, settings, other plugins and raised limits; the capability names and formats of the later kinds are placeholders for their own proposals.
