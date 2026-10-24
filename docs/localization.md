# UI localization

SolidCraft embeds its translations and works offline. English, Czech, Danish, German, English (UK), Spanish, French, Galician, Italian,
Japanese, Korean, Malay, Polish, Brazilian Portuguese, Russian, Turkish, Ukrainian and Chinese are currently registered in `crates/ui-egui/src/i18n.rs`. Adding a language requires an
embedded catalog and one registration there; the preferences selector and validation use the same
registry.

## Messages

`assets/locales/en.json` defines stable message IDs and English fallback text.
Keep IDs when changing wording. Other catalogs use the same IDs; missing messages fall back
to English. Use `app.tr("ui.preferences")` for static messages and
`Locale::format` for full sentences with named placeholders. Translate the entire sentence,
including punctuation, so a translator can reorder its arguments. Placeholder values are
inserted literally, even if a document name contains braces.

The English-label adapter (`app.tr_label`) supports existing engine and UI tables. It maps
display text to a message ID, then uses the same catalog lookup. New UI messages should use
stable IDs directly. The adapter does not change command IDs, model names, or scenario handles.
When a table's English wording changes, update the corresponding English catalog entry too.
Generated browser labels use named templates while their underlying model names stay intact.

Catalogs are parsed once and looked up through ordered maps. This is offline localization,
not a claim of zero runtime overhead. The catalog tests check registered locale IDs,
translation keys, and placeholder consistency. Run `cargo xtask ci` before submitting changes.

## Terms

The [glossary](book/src/reference/glossary.md) gives one English term for each idea, and each
entry has a note for translators: what the term must not be confused with. Its "Words with more
than one meaning" table lists the English words that need a separate catalogue entry for each
meaning (Construction, Flat, Round, Normal, Gap, Clearance).

## English (UK)

`en-GB.json` holds only the English messages whose spelling differs in UK English (Centre,
Customise, Organisation, Licence, colours, …); every other message falls back to English. When an
English message with a US spelling is added, add its UK spelling there too.

## Coverage

A message a locale never received falls back to English silently, so a gap shows up only as a row
that looks untranslated. Two guards catch that in the build:

- `every_complete_catalog_carries_every_english_message` — adding a key to `en.json`
  without giving it to a complete catalog fails the tests. English is the source catalog,
  Japanese and Chinese are delivered complete; Danish, German, Spanish, Galician, Italian, Korean, Malay, Polish, Brazilian Portuguese, Russian, French, Ukrainian and
  Turkish are partial — their gaps fall back to English and are reported by `cargo xtask i18n` without
  failing the build. A new label needs en, ja, zh, de and ko; Russian, French, Ukrainian and Malay may
  follow later.
- `every_toolbar_tab_and_panel_is_in_the_english_catalog` — tabs and panels reach the UI as bare
  source strings, resolved only by the English-label adapter, so a heading missing from `en.json`
  can never translate.

Command labels are the same case on a larger scale: every label in the engine's registry now has
an entry under `cmd.<command id>`, and `every_command_label_is_in_the_english_catalog` fails if a
command is added without one.

The context menu and the browser are the same case again, and they carry the most labels of all:
`every_menu_label_is_in_the_english_catalog` opens every menu the app can reach, flips the state
each conditional label branches on (origin shown, bodies locked, bodies hidden) so both branches
are seen, and fails on any label the catalog has never seen. Unit symbols are left out of the
sweep: `mm` is `mm` in every language, so they are not prose to translate. The sweep also asserts
it keeps reaching a broad set of labels, so it cannot pass by finding nothing.

`cargo xtask i18n` prints the same picture across every locale — which messages each one is
missing and which command labels English cannot translate. `--check` turns it into a non-zero exit
for CI. `--add-missing` writes each uncovered label into `en.json` under `cmd.<command id>`, with
the English label as its text, so the label adapter can find it and only the translations are left
to do. One key per distinct label: two commands that share a label (`Trim`, `Extend`) must not both
claim it, or the English-text lookup would silently pick one.

## Rendering and coverage

The Italian catalog uses original translations of SolidCraft's English messages, not text
extracted from commercial CAD software. Its core terminology is Schizzo (Sketch),
Estrusione (Extrude), Raccordo (Fillet), Smusso (Chamfer), Corpo (Body), Componente
(Component) and Vincolo (Constraint). Draft geometry is Sformo, while a report draft
is Bozza; Counterbore is Lamatura and Countersink is Svasatura. Established technical
terms such as Loft, Sweep, Spline and Offset remain unchanged. Command IDs, shortcuts,
file formats, units and user-provided names are not translated.

One immutable `Locale` type handles all translations. Table widgets without an app reference
use a render scope that restores the previous locale when it exits, including nested renders.
Standalone translated panels enter the same scope. Measure rendered text to size custom
widgets; UTF-8 byte lengths do not describe their displayed width.

The desktop host tries system CJK fonts across Windows, macOS and Linux, the Flatpak host font
folders (`/run/host/fonts`, `/run/host/user-fonts`), the folders listed in `SOLIDCRAFT_FONT_DIRS`
and an optional `CRAFT_FONTS_DIR` font pack, and skips files that cannot be parsed. Chinese uses its own fallback
chain after the general CJK font: many Japanese CJK fonts (Hiragino Gothic, Meiryo, Yu Gothic)
match the CJK font search but lack simplified characters, so a second fallback found by the
Chinese font-name list (`PingFang`, `Microsoft YaHei`, `Noto Sans CJK SC`, …) covers them per
glyph. No fonts are redistributed in native desktop distribution packages; native builds rely
entirely on existing operating system fonts or craft-fonts.

In the browser, `solidcraft-web` bundles two self-hosted CJK fonts, licensed under SIL OFL 1.1, within the
web distribution artifacts only: a Japanese font (`./fonts/BIZUDPGothic-Regular.ttf`, ~4.7 MB) and a Chinese
one (`./fonts/noto-sans-sc/NotoSansSC-Regular.ttf`, ~3.3 MB, Noto Sans SC at weight 400 cut down to GB 2312,
Big5 level 1 and every character of the `zh` and `zh-TW` catalogs by `assets/fonts/noto-sans-sc/subset.py`).
Each is loaded asynchronously from the same origin on demand, without contacting any external CDNs or
third-party servers: the Japanese font for a Japanese or Korean interface, the Chinese font for a Chinese
(`zh`, `zh-TW`) interface, which leads with it. With any other interface language, the Japanese font loads the
first time CJK text (kana, kanji or full-width forms in a design name, file name, body name or typed text)
is drawn, and the Chinese font once drawn Chinese characters remain that the fonts loaded so far lack. A
session that never shows such text downloads no font. If font loading fails with a CJK interface, the web
interface shows English for that session only to prevent missing glyphs (tofu); the saved language
preference is kept, so the next visit tries again. Korean (Hangul) is not covered yet, so a self-hosted Korean
font remains follow-up work for full web coverage. When a Chinese catalog gains a character the subset
lacks, a `fonts.rs` test names it; rebuild the font with `subset.py`.

The current messages support named substitution, not locale-specific plural selection,
number/date formatting, or right-to-left layout and shaping. Add those capabilities when
introducing messages or languages that need them; use an established message formatter for
plural rules instead of adding language branches to panels. The catalogs cover the UI surfaces
introduced by this PR, not every engine error or every newer command dialog.
