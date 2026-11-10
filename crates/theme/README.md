# solidcraft-theme

The token contract every UI component draws with. A component reads its colours and sizes from
`Tokens::get()` and its tab colours from `TabAccent::current(ctx)`; with nothing set, it draws with
SolidCraft's built-in dark tokens. The crate holds no data files and knows no theme-file format, so a
component vendored into another app draws with that app's tokens and needs no theme files, built-in
themes or icon artwork.

SolidCraft's theme system (theme files, their options, the built-in themes and the per-tab surface
fades) stays in the app: it resolves `Tokens` and hands them to this crate.

## API

| Item | What it is |
|---|---|
| `Tokens` | Every colour and size the UI draws with; `Tokens::light()` and `Tokens::dark()` are SolidCraft's values, written in code |
| `Tokens::get()`, `Tokens::view()` | The tokens to draw with now (the window's, and those drawn straight onto the 3D view) |
| `set_dark`, `set_vision`, `set_viewport_dark`, `set_viewport_background`, `set_logo_accent` | The thread's theme state, which the host sets every frame |
| `set_custom`, `custom`, `scoped_tokens`, `scoped_panel_tokens` | A host's own palette in place of the built-in one, for the thread or for a scope |
| `ColorVision` | Colour vision modes: a second set of values for the colours that carry a meaning |
| `TabAccent`, `AccentRamp`, `TabTable`, `TabStyle` | A tab's selection colours, published per egui context. The theme defines the accent ramps; the host lists its tabs in a `TabTable` with the ramp and icon accent of each, and installs it (`install_tabs`, `installed_tabs`) for theme files keyed by tab name |
| `set_published`, `PublishedTab`, `Environment`, `Surfaces`, `Fade` | A frame's per-tab palettes, as a host's theme system resolves and fades them |
| `apply(ctx)` | egui's visuals and spacing from the tokens |
| `contrast_ratio`, `composite`, `panel_surface_fill`, ... | WCAG contrast checks on token pairs |
| `icon_strokes` | The primary strokes the tokens share with SolidCraft's icon artwork |
