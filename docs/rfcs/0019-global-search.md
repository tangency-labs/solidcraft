# RFC 0019: One global search, and declarations that make plugins findable before they're installed

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-25 |
| Apps | SolidCraft (the search registry and the declaration matcher are app-agnostic, so other Craft apps can use them) |
| Discussion | [#22427](https://github.com/tangency-labs/solidcraft/issues/22427) |
| Extends | [RFC 0017](0017-workshop.md) (the Workshop: phase 3 of this RFC reads its index), proposed in [#22332](https://github.com/tangency-labs/solidcraft/pull/22332) and not yet on main, so its file is linked here until that PR merges |
| Related | [#19042](https://github.com/tangency-labs/solidcraft/issues/19042) and [#19043](https://github.com/tangency-labs/solidcraft/issues/19043) (one search control for panels), [#9080](https://github.com/tangency-labs/solidcraft/issues/9080) (the "Search commands" box shows unrelated results) and [#7054](https://github.com/tangency-labs/solidcraft/issues/7054) (command palette), [#18041](https://github.com/tangency-labs/solidcraft/issues/18041) and [#18609](https://github.com/tangency-labs/solidcraft/issues/18609) (pin plugin commands), [#20425](https://github.com/tangency-labs/solidcraft/pull/20425) (importer manifest fields), [#22207](https://github.com/tangency-labs/solidcraft/issues/22207) (preferences search) |

## 1. Summary

One search in SolidCraft that finds everything: commands, settings, help, samples, the open design's
features, bodies, sketches and parameters, library parts, and every installed plugin, generator and theme.
Plugins and generators make themselves findable by declaring, in their manifest, what they handle: labels,
keywords, and for generators the ordering codes they understand (as anchored patterns with examples).
The host evaluates those declarations as data. Once the Workshop index exists ([RFC 0017](0017-workshop.md)), the same
declarations from items that are NOT installed show up too, as "Install to use" rows, so typing
`MGN12H2R300` finds the HIWIN generator even if you've never heard of it.

## 2. Motivation

Today (main, cited from the code):

- "Search commands" (`shortcut_box.rs`) searches commands only, plus the open design's feature and
  parameter names. It never searches settings, help, samples, library parts or plugins. Ranking is
  prefix > word start > substring > letters in order, with a hard-coded keyword table of about 10 ids.
- The Parts Library matches file names only; Insert Part has no search.
- Five separate matchers behave differently: commands, the Browser filter, Preferences search, the
  plugins list, and the MCP command filter.
- A plugin's entries can't reach search at all: they aren't CommandSpecs, and the manifest has no
  keywords field (`deny_unknown_fields`).

So a person who knows the thing they want by name, code or keyword has to know which panel holds it.

## 3. Design

### 3.1 One search, many sources, one registry

Every searchable thing comes from a **source** registered with one search registry:

```rust
struct SearchSource {
    id: &'static str,            // "commands", "settings", "design.features", "parts", "plugins.installed", ...
    surfaces: Surfaces,          // GLOBAL | PARTS_BROWSER | PREFERENCES | BROWSER_FILTER ...
    rank: u8,                    // group rank (3.3), stated, not computed
    budget: Duration,            // per-source time budget; a source over budget contributes nothing
    search: fn(&Query, &Viewer) -> SourceResult,
}
```

- The host asks only the sources that serve the asking surface. The existing panel searches
  ([#19043](https://github.com/tangency-labs/solidcraft/issues/19043)'s shared control) become surfaces of the same registry, so they all match the same way.
- **Built-in sources (phase 1):** commands; settings (a settings registry has to be BUILT first: today a
  setting is only found by being drawn, `settings_search.rs`; phase 1 builds the registry so a new setting
  is findable the moment it exists, with a lint that keeps any hand-written list and the registry in step); help and docs titles; samples; the open design (features, bodies,
  sketches, components, parameters, named views); library parts (names, part numbers, descriptions,
  Standard parts families and sizes, e.g. "M5x12 SHCS"); installed plugins, generators and themes.
- **Matching:** case- and accent-folded; prefix, word start, substring, then a small typo tolerance
  (one edit on words of 5+ letters) only when nothing else matched, labelled "near matches".
  Tokens under 3 characters match exact substrings only. **Typo tolerance never touches a token that
  contains a digit:** one edit there is a different part (MGN12 vs MGN15, M5 vs M6).
- **A source that fails says so.** A result list carries `sources_failed` (errors AND sources that ran over
  their budget) and the UI names them ("Library couldn't be searched"), never an empty success.
- **Sources run off the UI thread,** each in its own cancellable task; a new keystroke cancels the previous
  query's tasks. The UI shows results as groups arrive and never blocks a frame on a source.

### 3.2 Declarations: how a plugin, generator or theme becomes findable

Manifests gain one optional, pure-data block. The host reads it; no plugin code runs to answer
"is this mine?".

```toml
[declares]
labels   = ["Linear rail", "HIWIN MGN"]          # shown as the row's title options
keywords = ["linear guide", "carriage", "block"] # words people type; not a copy of parameter choices
vendor   = "HIWIN"
[declares.labels_l10n]                            # per-locale variants; English is always matched too
de = ["Linearführung", "HIWIN MGN"]

[[declares.matches]]                              # generators: ordering codes they understand
id         = "mgn-set"
generator  = "mgn"
pattern    = '^MGN(?P<size>7|9|12|15)(?P<block>[CH])E?(?P<blocks>\d)?R(?P<length>\d{2,4})'
confidence = "exact"                              # exact | prefix | possible
params     = { size = "{size}", block = "{block}", blocks = "{blocks}", length_mm = "{length}" }
examples          = ["MGN12H2R300Z0C", "MGN 12 C E 2 R1600 E Z1 P M II"]
negative_examples = ["MGN12", "MGW12H2R300", "MGNR12"]
```

The host normalises the query before any pattern sees it: Unicode-fold, upper-case, and drop spaces,
`-`, `.`, `/` and `_`. Patterns are written against that normal form (no `\s*` noise, no `i` flag), and
the row shows the user's original text. Examples are normalised the same way before they are checked.

Rules (enforced by the manifest validator at publish and at load):

- `examples` and `negative_examples` are required for every pattern; a positive example that fails or a
  negative one that passes rejects the manifest.
- Patterns use Rust's `regex` crate (linear time, no backreferences or lookaround). Caps: 400 characters
  per pattern, 32 patterns and 16 captures per plugin.
- Anchored patterns only may claim `exact`; an unanchored one is at most `possible`.
- An `exact` or `prefix` hit also needs its captured values, for the query actually typed, to pass the generator's parameter validator. A pattern matches a query prefix, so `MGN12H2R3000000` would capture a length of 3000 and leave `000` over; a hit whose values fail drops to `possible` and prefills nothing. The publish-time example checks alone do not cover arbitrary queries, which is why this is checked at search time too.
- A `possible` hit is a suggestion row only: it never acts on Enter.
- Every positive example's captured values must pass the generator's own parameter validator at publish
  (sizes, ranges, choices), so a pattern can't claim a code its generator would refuse.
- Keywords that merely repeat a generator's own parameter choices are rejected (the host derives those).
- These fields go in the manifest under the new `extensions`/declared-fields mechanism from [#20425](https://github.com/tangency-labs/solidcraft/pull/20425), so
  older hosts ignore them instead of refusing the plugin.

### 3.3 Ranking

Groups, in this order, each with a stated rank: **best match** (an exact identifier or code hit),
**in this design**, **commands**, **parts**, **settings**, **help**, **installed plugins**,
**from the Workshop** (not installed). Inside a group: confidence (exact > prefix > possible), then
installed before not installed, then pattern specificity (longer literal prefix, more fixed characters),
then a stable id order so the list never reshuffles. Results are de-duplicated by `kind:id`. A group's
count is the true total, not the page size.

**Trust decides what a declaration may claim; curation decides whether it is pushed.** Only a publisher
linked to the vendor (or Official) may claim that vendor's codes as `exact`; Community generators are capped
at `prefix` and Unverified at `possible` for another brand's codes. Search is pull, so any listed item may
appear when searched for; push surfaces (unasked suggestions in the parts browser) show featured items only.

**A not-installed exact hit ranks in "best match"** (a code is the strongest signal there is), but its
row's action is Install to use: Enter opens the install screen only, never installs. An installed exact hit
for the same string ranks above it.

Two exact claims on one string from different generators: SolidCraft does not choose. It shows both with
their publisher and remembers the user's last pick for that conflict (keyed by the set of competing item
ids), stored locally only. The Workshop's index build runs
EVERY item's examples against EVERY other item's exact patterns and fails when two published generators
claim one example as `exact`, unless one declares `supersedes` or both `coexists`.

### 3.4 Not-installed items (phase 3, needs the Workshop)

The Workshop index ([RFC 0017](0017-workshop.md)) carries a compact **match table**: each published item's `[declares]` block
plus the host-derived vocabulary (its parameter-choice words), no code. It is built automatically from the
published manifests, never by hand. Every app build embeds a copy, so a first run offline still finds
things; SolidCraft refreshes it in the background (signed, sequence-numbered) and compiles every pattern
once at load. At install, SolidCraft fetches the item's manifest fresh and re-runs the match against it,
so an older snapshot can never prefill values from a pattern that was since removed. A typed `MGN12H2R300` matches in memory, offline, in microseconds.

The row shows: the item's name, publisher, tier badge, the captured values ("MGN, size 12, H block,
300 mm"), and **Install to use**. Clicking opens the same install screen as the Workshop's "Open in
SolidCraft" button (tier, author, permissions, Install / Cancel). After install, the same row becomes
**Create part** with the values prefilled. Nothing installs from a keypress, and a hit never runs the
item's code. A stale snapshot shows its "as of" date.

### 3.5 UI

- One search box, the existing top-right one, renamed "Search" (Ctrl/Cmd+K and the current shortcut);
  it opens even while the app is still loading.
- One flat list with section headers; arrows move one row; Enter runs the row; Esc closes; the focused row
  is announced to screen readers.
- Each row says why it matched (the matched word highlighted, or the pattern's captured values).
- A search that finds nothing offers a shorter query.
- The parts browser gets the same box scoped to its surface (parts, Standard parts, installed and
  Workshop generators).
- Agents: the same registry behind an MCP `search` command with the same results and `sources_failed`.

### 3.6 Privacy

- Every source gets the viewer; a search never matches a field the viewer can't read (relevant once
  shared and Workshop designs exist).
- Typed queries are never sent anywhere: Workshop matching is local, against the snapshot.

Each declared entry (a generator, a command) carries its own one-line description, used in its search row
and in the MCP tool list (today every entry reuses the plugin's description).

### 3.7 Provenance

Rows for generators and Workshop items show the item's provenance badge: **Checked** (an automatic spec check against the datasheet), **Reviewed** (plus an independent named reviewer) or **Drawing-matched** (plus a match against the vendor's drawing). The declaration carries the badge level, and the Workshop index build sets it from the generated `PROVENANCE` record, so a manifest cannot claim more than was verified ([RFC 0017](0017-workshop.md), section 2.3.1). Nothing below **Checked** is published, so nothing below it is listed or found. Provenance does not change ranking; it is shown on the row beside the tier badge.

## 4. Phases

1. **Global search over installed things** (no manifest change): the registry, the built-in sources,
   ranking, `sources_failed`, panel searches moved onto it. Useful on its own.
2. **Declarations** in manifests (labels, keywords, generator patterns) with the validator and example
   runner; installed generators answer codes in the parts browser and global search.
3. **Workshop items**: the signed match-table snapshot, Install-to-use rows, conflict checks at index build.

Phase 1 replaces the five matchers (it covers what [#9080](https://github.com/tangency-labs/solidcraft/issues/9080) and [#22207](https://github.com/tangency-labs/solidcraft/issues/22207) ask; #22207 is claimed by its contributor, whose work comes first and which this RFC would build on); the old ones are removed, not kept alongside.
Phase 2 dogfoods the format on SolidCraft's own Standard parts before any third party uses it.

Consequence tests written into the work (each proven red first):
nothing installs from a keypress; a `possible` match never runs on Enter; an MGW code never offers the MGN
generator; a mistyped code never offers a different size; a source that times out is named in the results;
a search never matches a field the viewer can't read; a stale snapshot always shows its date; a pattern's
examples hold at publish and at load.

Each phase ships with the test or lint that stops its failure class (e.g. a source that ignores the query,
a pattern whose examples don't hold, a stale snapshot without its date).

## 5. Alternatives considered

- **Plugins answer search by running code.** Rejected: it needs the plugin installed and running, costs a
  call per keystroke, and runs untrusted code to decide whether to show itself.
- **Keywords only, no patterns.** Rejected for generators: ordering codes are structured, and a keyword
  list can't say "MGN12H2R300 is mine, MGW12H2R300 isn't".
- **Search hits the Workshop server per keystroke.** Rejected: leaks what people type, fails offline, and
  adds latency. The snapshot is small and signed.

## 6. Open questions

1. Prefix filters (`>` commands, `@` design, `#` parts): leave room in the query format, don't build them.
2. Standard parts expose declarations too (yes, per review): "M5x12 SHCS" and "ISO 4762 M5" resolve to the
   built-in generator with values filled in, through the same matcher, so built-in and plugin generators
   behave alike.
3. Snapshot size budget for the web app.
4. Brand claims (yes, per review): only a vendor-linked or Official publisher may mark a vendor's codes
   `exact`; Community generators are capped at `prefix` and Unverified at `possible` for another brand's codes.

## 7. Prior art

Cobblr's command palette and resolvable registry (ranked groups, exact identifiers first, per-source
failure named, viewer on every query, keywords derived from declarations, a catalog built from shipped
manifests); Home Assistant / HACS (discoverable integrations by declared domains); VS Code's command
palette and extension `activationEvents` (declare, don't execute).
