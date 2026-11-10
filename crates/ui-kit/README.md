# solidcraft-ui-kit

The UI kit: the tokenised egui controls every dialog, panel and menu is built from, and the
components built from them. A component takes plain data (labels, icons, entries, values and the
host's prefix for its names), draws with `solidcraft_theme::Tokens`, reads its text through
`solidcraft_ui_text`, publishes its handles into `solidcraft_ui_registry` and returns what the user
chose for the host to run. Nothing here reads an app's state.

What a host draws with is host data, installed once per process before the first frame or passed
in: its icon set (`icons::install`; `icons::IconSet::extended` draws another host's set with a few masters of its own), its glyphs, its tab table (`solidcraft_theme::install_tabs`),
its navigation catalogue (`navigation_bar::install_catalogue`), its message catalogs
(`solidcraft_ui_text::install`), its built-in themes (`theme_files::install_builtin`) and its
fonts. With none installed the kit still draws: icons draw nothing, text shows its message ids and
there are no built-in themes.

## API

| Item | What it is |
|---|---|
| Rows and fields: `row`, `expr_row`, `text_row`, `choice_row`, `count_row`, `slider`, `checkbox`, `dropdown`, ... | The form controls, with aligned label columns |
| `Container`, `Window`, `Panel`, `Menu`, `Confirm` | Every outer window, popup, panel and menu |
| `icons` | The icon renderer over the host's installed icon set |
| `type_face` | The kit's font weights over the host's fonts |
| `ribbon`, `app_bar`, `history_bar`, `nav_bar`, `navigation_bar`, `view_compass`, `prefs_window`, `start_page`, `window_chrome` | Components: plain data in, the action chosen out |
| `frame` | Dock dragging and bounded anchors |
| `blur` | The blurred backing behind translucent panels |
| `theme_files`, `theme_options` | Theme files: reading, validating, resolving to `Tokens`, their options; the host's built-in themes |

The `test-support` feature adds the kit's test fixtures for a host's own tests of the kit with its
data: `test_support` (a context with egui's default faces and the kit's English test catalog,
headless frames and events, the CPU rasteriser), `gallery` (the specimens and their checks) and
`theme_files::fixtures`.

See `docs/ui-kit.md` for the controls and their rules.

## Licence

This crate is part of SolidCraft's brand and interface, under the SolidCraft Brand and Interface
Licence ([LICENSE](LICENSE)). Use it as part of SolidCraft; using it in another program or product
needs the maintainers' written permission first.
