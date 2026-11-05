# Interface fonts

SolidCraft bundles IBM Plex Sans as its default interface font on desktop and web.
The unmodified variable font provides regular (400), medium (500), semibold (600)
and bold (700) faces. Titles and selected labels use semibold; compass letters
and the primary OK action use bold. Selection fields and status messages use
medium. Numeric fields and sketch dimensions use IBM Plex Mono Regular.

Both fonts are licensed under the SIL Open Font License 1.1. Their licence texts
are in `assets/fonts/ibm-plex-sans/OFL.txt` and
`assets/fonts/ibm-plex-mono/OFL.txt`; provenance is recorded in
[ATTRIBUTION.md](../ATTRIBUTION.md).

The `ui_font` preference puts the chosen font first in every sans family,
including the three weighted families. A chosen static font retains its own
weight; SolidCraft does not simulate bold by drawing text twice. Clearing the
preference restores Plex. Monospace fields keep Plex Mono.

Egui's symbol and emoji fallbacks remain available. Desktop also appends the
available Japanese/Chinese, Hangul and Thai system fonts to every family, ordered
for the selected language. The web app installs Plex at startup and adds its
bundled Japanese, Chinese and Korean fonts asynchronously when their interface
language needs them, preserving the interface faces and any font preference.
The Korean web face is unmodified IBM Plex Sans KR Regular (SIL OFL 1.1), with
its license in `assets/fonts/ibm-plex-sans-kr/OFL.txt`. Each downloaded face stays
available when the language changes. A failed download keeps the existing
session-only English fallback and preserves the saved language choice.

## Type scale and metrics

Text sizes come from `theme::Tokens`, with identical values in the light and dark
palettes in `assets/styles/ui/palettes.rs`. The compact scale runs from
`font_badge` (9.5 pt) through `font_heading` (14 pt). Special roles preserve the
existing designs: `font_triad` (8), `font_compass` (13.5), `font_subtitle` (15),
`font_intro` (16), `font_article_heading` (17), `font_title` (22), and
`font_display` (26). Scaled labels derive from these tokens. Weight helpers in
`fonts.rs` choose the family independently of size.

Dense forms also use tokens for control height/radius, label width, row gap,
header/footer/card dimensions, icons, text insets, checkbox and action heights,
and section letter spacing. Existing `space_*` tokens supply shared padding.
The `font_literals_tests` source guard scans production Rust, including the
viewport icon sources, and rejects numeric font sizes, private font families
outside `fonts.rs`, and direct dense-form spacing literals. Its small per-file
allowance table documents graphic spinner diameters; it permits no text sizes.
Run it with `cargo test -p solidcraft-ui-egui --lib font_literals_tests`.
