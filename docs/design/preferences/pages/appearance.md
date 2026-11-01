# Appearance

Board: `Prefs2_Appearance` (1920x1595). Heading: Look. Subtitle "Colours, text and accessibility."
Tag: In profile (hover "Saved in settings profiles"). Reference boards: `Theme_Options` for the theme options,
`Settings_Interface` for Scale and text and Colour vision. Source today: Interface page
(`prefs/interface.rs`).

## Theme (6 themes)

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Theme cards: Light (Built in), Dark (Built in), System (Follows the OS), Gruvbox (Built in), Studio glass (Imported file) | Carousel of picture cards (88 wide) with previous and next arrows ("Previous themes", "Next themes") | `Prefs.theme` (id string); "System" follows the OS (`app.window_theme`) | `"dark"` | "Has options" on the disc of cards that have theme options | `theme-light`, `theme-dark`, `theme-system-light-half` + `theme-system-dark-half`, `theme-gruvbox`, `theme-imported` | The count "6 themes" is the number of cards. Gruvbox and Studio glass carry the "has options" disc. Build the cards from the theme list in `theme_files` (built-ins in `assets/styles/ui/themes`, imported in `Prefs.custom_themes`); only the first five are drawn on the board, the carousel scrolls to the rest. |
| Import... / theme file | Dashed card at the end of the carousel | theme import (`Prefs.custom_themes`) | n/a | none | none | "Import..." with the sub-label "theme file". |

### Theme options panel ("Studio glass", "6 options", Reset)

Follows `Theme_Options.dc.html`. Shown under the carousel for a theme that has options. "Reset"
restores the theme's own option values (`Prefs.theme_options[<theme>]`). Two columns.

| Label | Control | Edits | Default | Hover | Notes |
|---|---|---|---|---|---|
| Opacity | Slider with "72 %" readout | `Prefs.theme_options[theme]["opacity"]` (standard option id `opacity`; the only provider registered on main, `theme_options::PROVIDERS`) | the theme's | "Opacity" (the label) | aria-label "Panel opacity". |
| Blur | Slider (dimmed, thumb grey) with the reason "Software graphics" | `theme_options[theme]["blur"]` (standard id `blur`; no provider on main yet, so it is not offered until one registers) | the theme's | "Blur" | aria-label "Blur behind panels". Unavailable on this computer: it stays in place, disabled, with the reason as text. On `Theme_Options` the value reads "18 px" with the hover "Relative GPU cost", and the stops are "Off" (hover "No GPU work"), "8 px", "18 px", "40 px"; 0 is off and the cost meter flatlines. |
| Tab colours | Switch + "Fade" and "240 ms" (a duration stepper, aria "Fade duration") | `theme_options[theme]["tab_colours"]` and `["tab_fade_ms"]` (standard ids) | the theme's | "Tab colours" | The theme defines the tab colours: 3 surfaces per tab (panel, view top, view bottom; hovers on `Theme_Options`). |
| Contrast | Segmented: Standard / High | a theme-declared option (custom id, `OptionDef` in the theme file) | the theme's | "Contrast" | aria-label "Contrast". Not a standard behaviour on main: it appears only where the theme file declares it. |
| Corners | Segmented: Square / Soft / Round | a theme-declared option | the theme's | "Corners" | As Contrast. |
| Density | Switch (aria "Compact density") | a theme-declared option | the theme's | "Density" | As Contrast. |
| More . 3 | Disclosure | Style, Edge glow, Shadows (`Theme_Options`), theme-declared options | the theme's | none | The page board does not draw the extra three; `Theme_Options` lists them. |

Disabled options keep their place with a reason badge: "Software graphics" for Blur; "System: Reduce
transparency" for Opacity on `Theme_Options`. The adaptation is one ordered list: the standard options
first, then the theme's own.

## Viewport and logo

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Background | Segmented: Theme's gradient / Colour, plus a hex field "#FFFFFF" | `Prefs.viewport_background` (`None` = the theme's gradient; `Some([r,g,b])` = Colour) | `None` | none | none | The hex field is active only for Colour. |
| 3D view colours | Segmented: Theme's own / Light / Dark | `Prefs.viewport_theme` (`ViewportTheme`) | Theme | none | none | |
| Logo | Segmented: Theme's colours / Accent | `Prefs.logo_accent` | `false` | none | none | |
| Follow a theme file | Checkbox + "Choose..." button + badge "Desktop only" | `Prefs.theme_follow` (path; empty = off) | empty | "Reloads when another tool rewrites the file." | none | "Desktop only" is a badge: the control is hidden or disabled on the web build. |

## Scale and text

Source: `Settings_Interface.dc.html` (section "Scale and text").

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Interface scale | Slider with "110 %" readout and six chips 75%, 100%, 110%, 125%, 150%, 200% | `Prefs.ui_scale` (accepted 0.5 to 2.5) | `1.0` | none | none | aria-label "Interface scale". Chip row selects a preset; the selected chip is accent. |
| Live preview | A framed sample: "Distance" field ("OK"), a command row "Extrude 112.5 mm" with a 16 px icon | none (sample) | n/a | none | none | The preview rescales as the slider moves. Placeholder content. |
| Font | Dropdown "IBM Plex Sans (built in)" (aria "Interface font"), "Choose font file..." button, "Reset" | `Prefs.ui_font` (path; empty = built-in) | empty | none | none | |
| The quick brown fox . O12.50 mm . 45.0 deg . 0123456789 | Sample line | none | n/a | none | none | Placeholder sample text. |

## Colour vision

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Mode | Segmented: Off / Protan / Deutan / Tritan | `Prefs.color_vision` (`ColorVision`) | Off | "Changes the colours that carry a meaning: sketch states, axes, bends, analyses and icons." | none | The sample row previews the chosen mode (the preference previews before applying; `Tokens::get_in`). |
| Status chips: Selected, Warning, Error, Done | Four 13 px icons with names | none (sample) | n/a | none | none | |
| Sample row: Fixed, Projected, Axes, Bends, Draft, Failed | Six 84x44 cells | none (sample) | n/a | none | `colour-vision-fixed`, `-projected`, `-axes`, `-bends`, `-draft`, `-failed` | Drawn with the mode's colours as the app does today. |

## Mismatches

* The dark board's accent `#8CBEF5` has no token (see `tokens.md`).
* Only `opacity` has a provider on main (`theme_options.rs`); blur, tab colours and the fade are standard ids
  without one yet, so the board shows them ahead of the code.
* Only 5 of the 6 theme cards are drawn on the board.
