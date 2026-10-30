# UI localization

SolidCraft embeds its translations and works offline. English, Arabic, Bosnian, Czech, Danish, German, English (UK), Spanish, French, Galician, Hebrew, Croatian, Italian,
Japanese, Korean, Malay, Norwegian Bokmål, Dutch, Polish, Brazilian Portuguese, Russian, Serbian, Turkish, Ukrainian and Chinese are currently registered in `crates/ui-egui/src/i18n.rs`. Adding a language requires an
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
  Arabic, Hebrew, Japanese and Chinese are delivered complete; Bosnian, Croatian and Serbian carry
  every English message today but are not held to it, so a new label may follow later in them; Danish, German, Spanish, Galician, Italian, Korean, Malay, Norwegian Bokmål, Polish, Brazilian Portuguese, Russian, French, Ukrainian and
  Turkish are partial — their gaps fall back to English and are reported by `cargo xtask i18n` without
  failing the build. A new label needs en, ar, he, ja, zh, de and ko; Russian, French, Ukrainian and Malay may
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

The Bosnian (`bs`, Bosanski), Croatian (`hr`, Hrvatski) and Serbian (`sr`, Српски) catalogs are
original translations kept in step: Bosnian and Croatian are ijekavian in Latin script, Serbian is
ekavian in Cyrillic. They share Skica (Sketch), Ograničenje (Constraint), Kota (Dimension),
Ekstrudiraj (Extrude), Zaobljenje (Fillet), Skošenje (Chamfer), Niz (Pattern) and Spoj (Joint), and
follow each standard where it differs: Tijelo/Tijelo/Telo (Body), Ravan/Ravnina/Ravan (Plane),
Tačka/Točka/Tačka (Point), Ivica/Brid/Ivica (Edge), Ugao/Kut/Ugao (Angle), Rupa/Provrt/Rupa (Hole),
Sačuvaj/Spremi/Sačuvaj (Save). A timeline feature is Operacija in Bosnian and Serbian and Značajka in
Croatian; the Browser panel is Stablo modela, so it is never confused with a web browser
(preglednik/pregledač); the undo history is Historija/Povijest/Istorija and the timeline Vremenska
traka. In Serbian, brand and file names, acronyms, units, key names and placeholders stay in Latin
script inside the Cyrillic text. Command IDs, shortcuts, file formats, units and user-provided names
are not translated.

The Norwegian Bokmål catalog (`nb`, shown as Norsk bokmål) uses Skisse (Sketch),
Kropp (Body), Funksjon (timeline Feature), Binding (Constraint), Avrunding (Fillet)
and Fas (Chamfer). Ground/Unground are Fest posisjon/Frigjør posisjon, Draft is
Slippvinkel, Flat Pattern is Utbretting, and Counterbore/Countersink are Sylindrisk
forsenkning/Konisk forsenkning. Capture Position is Registrer posisjon, while
sketch snapping is Punktfangst. Loft, Sweep, Spline and Offset remain technical
terms. Command IDs, shortcuts, file formats, units and user-provided names stay
unchanged.

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

In the browser, `solidcraft-web` ships self-hosted script fonts (SIL OFL 1.1) in the web distribution
artifacts only, loaded asynchronously from the same origin without contacting any external CDNs or
third-party servers:

- Japanese (and Hangul-less Korean UI for now): `./fonts/BIZUDPGothic-Regular.ttf`
- Simplified Chinese (`zh`): `./fonts/CraftUI-SC.otf` (a locale-glyph subset of Noto Sans SC, renamed
  under the OFL so it does not use the reserved name Noto)
- Traditional Chinese (`zh-TW`): `./fonts/CraftUI-TC.otf` (same for Noto Sans TC)

A CJK interface language fetches the matching face on demand. With any other interface language, the
first time Japanese text (kana, kanji or full-width forms in a design name, file name, body name or
typed text) is drawn, the Japanese face is fetched (#720). A session that never shows such text
downloads no script font. If the needed face fails to load, the web interface shows English for that
session only to prevent missing glyphs (tofu); the saved language preference is kept, so the next
visit tries again. Korean Hangul coverage on the web remains follow-up work.

## Right-to-left text

egui 0.36 shapes each run of one font with harfrust, so a run of Hebrew letters is drawn right to
left, but it has no bidirectional layout: the runs of a line (words, spaces, numbers, punctuation)
are placed left to right in the order they are stored. A Hebrew sentence stored in reading order
would show its words in the wrong order and its punctuation on the wrong side. `crate::bidi` runs
the Unicode Bidirectional Algorithm (the `unicode-bidi` crate, UAX #9) over every message of a
catalog that has right-to-left text, once, when the catalog is first used, and `Locale::text`
returns the order egui needs: the runs of the line in display order, brackets inside
right-to-left text mirrored, and the letters of each Hebrew run left in reading order for the
shaper to reverse. Each line is its own paragraph and takes its direction from its first strong
letter, so English fallback text is unchanged. `hebrew_messages_land_on_screen_in_display_order`
lays messages out with a real Hebrew font and checks where the glyphs land.

Arabic takes the same path. Its letters join, which the shaper does from the font's own tables, so
each Arabic word is one run: the Arabic font comes after the Latin interface font, which keeps the
spaces, digits and Latin text, and it is the first fallback for an Arabic UI (Segoe UI, Tahoma or
Arial on Windows; SF Arabic, Geeza Pro, Damascus or Tahoma on macOS; Noto Sans or Naskh Arabic,
KACST One or DejaVu Sans on Linux; `NotoSansArabic-Regular.ttf` in `CRAFT_FONTS_DIR`). Write Arabic
messages with Western digits, which keep their left-to-right order inside the sentence.
`arabic_messages_land_on_screen_in_display_order` checks the word order on screen.

Placeholders are kept whole where they stand, so `format` substitutes values into the
display-order text and a value keeps the order it has: a number, a file name, a label that is
already in display order. Translators write normal Hebrew in reading order; they never add
directional marks. When a prefix letter joins a Latin word or a placeholder, use the maqaf
(`ב־{name}`, `ל־STEP`), which is a Hebrew letter to the algorithm, not an ASCII hyphen.

This relies on Hebrew letters and the spaces and punctuation between them coming from different
fonts (the bundled IBM Plex Sans has no Hebrew), which is how the default fonts are set up; an
interface font that covers both Hebrew and Latin puts a whole line in one run and can reverse it.
What this does not do yet: the layout is not mirrored (the browser stays on the left, labels stay
left-aligned), and a message egui wraps onto several lines keeps the order of the whole string, so
its lines read bottom to top. Names people type in Hebrew are shown as stored. The desktop host
appends a system Hebrew font (Arial Hebrew or SF Hebrew on macOS, Segoe UI or Arial on Windows,
Noto Sans Hebrew or DejaVu Sans on Linux); the web build has no Hebrew font yet, so a Hebrew
interface shows empty boxes there.

The current messages support named substitution, not locale-specific plural selection,
number/date formatting, right-to-left layout mirroring, or shaping for joining scripts such as Arabic. Add those capabilities when
introducing messages or languages that need them; use an established message formatter for
plural rules instead of adding language branches to panels. The catalogs cover the UI surfaces
introduced by this PR, not every engine error or every newer command dialog.
