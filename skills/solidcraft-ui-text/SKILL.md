---
name: solidcraft-ui-text
description: "Use when adding or changing any user-visible text in SolidCraft: a label, tooltip, message, command label or a new language. Covers the message catalogs in assets/locales, sorted lists, which locales a new key needs, UK English and terminology."
---

# UI text and translations

SolidCraft embeds its translations and works offline. The long form is `docs/localization.md`.

## Adding a message
1. Add the id and English text in a new pending file (next section); `assets/locales/en.json` itself is only edited to change an existing message. Ids are stable: keep them when you change wording.
2. Use `app.tr("ui.some_id")` for static messages and `Locale::format` for whole sentences with named placeholders. Translate the whole sentence, punctuation included, so a translator can reorder arguments. Use stable ids directly for new messages, not the English-label adapter (`app.tr_label`), which exists for older tables.
3. Command labels live under `cmd.<command id>`; `cargo xtask i18n --add-missing` writes the ones that are missing into `en.json`.
4. Add the UK spelling to `assets/locales/en-GB.json` when the US spelling differs (Centre, Colour, Licence, ...). That file holds only the differing messages.
5. A new label needs `en`, `ja`, `zh`, `de` and `ko`. The other catalogs are partial and fall back to English; `cargo xtask i18n` reports their gaps without failing, and `every_complete_catalog_carries_every_english_message` fails a missing `ja` or `zh` entry.
6. Run `cargo xtask lists --fix` before committing, then `cargo xtask i18n`.

## Sorted catalogs
Catalogs are one message per line, sorted by id, with no repeated id. `cargo xtask lists` (part of `cargo xtask ci` and the Lists workflow) fails on an unsorted file. Catalogs are no longer edited for new messages (see Pending locale files); for an edit, let `--fix` place it. Two PRs that add neighbouring keys still collide on these files; rebase and run `--fix` again.

## Pending locale files
New strings go in a new `assets/locales/pending/<slug>.json` (lowercase, dashes, named after the change), never into the catalogs. It maps a locale code to `{"message id": "text"}`: give English and any other locale you can translate; a locale you leave out falls back to English. The id must not already be in that locale's catalog or in another pending file (the build and `cargo xtask lists` fail otherwise); to change an existing message, edit the catalog. Do not sort or fold: the app, `cargo xtask i18n` and the tests read pending files as part of the catalogs. After a merge, the Generated files workflow runs `cargo xtask fold-pending` on `main`; never run it in a PR. New ATTRIBUTION.md rows likewise go in a new `attribution/pending/<slug>.md`, never into the table. Each folder's README has the details.

## Other sorted lists
- Languages (`crates/ui-egui/src/i18n.rs`): one per line, English first, then by id; a test checks it. Adding a language is an embedded catalog plus one registration there.
- `mod` lines: one per line, in the sorted group they belong to.
- `docs/features.md` and `THIRD-PARTY-LICENSES.txt` are generated on `main` after each merge: never edit or regenerate them in a PR.

## Wording
Use the terms in the glossary (`docs/book/src/reference/glossary.md`); words with several meanings (Construction, Flat, Round, Normal, Gap, Clearance) have one catalogue entry per meaning. A new idea adds its glossary term. Command labels are Title Case, other text sentence case. Tests also require tabs, panels, menu labels and command labels to be in `en.json`.
