# solidcraft-look

SolidCraft's look, ready to install into the UI kit (`solidcraft-ui-kit`), which ships no artwork,
faces or themes of its own. SolidCraft installs it from here, and so can any other host that wants
the same look: one copy of each piece.

| Item | What it is |
|---|---|
| `install(ctx)` | Everything: `install_tables()` and `fonts::install(ctx)`. Call it before the first frame and in every test context |
| `install_tables()` | The process-wide parts: the icon set, the glyphs and the built-in themes |
| `icons::DATA`, `icons::SET`, `icons::install()` | The icon masters (`icons/design/svg`), colour roles, accents, default accents and aliases (`palette.toml`, `aliases.toml`), compiled by `build.rs`; the set over them; installing it |
| `glyphs::GLYPHS`, `glyphs::install()` | The code-drawn glyphs (transport, caption, badges, pin, cross, breadcrumb, compass ends, agent pointer, brand mark), with `compass` and `pointer` |
| `fonts::definitions()`, `fonts::apply(ctx, defs)`, `fonts::install(ctx)`, `fonts::PLEX_SANS`, `fonts::PLEX_MONO` | IBM Plex Sans (four weights of one variable face) and Plex Mono bound to the kit's weight families, egui's faces after them and Hack last |
| `themes::BUILTIN`, `themes::install()` | The built-in theme files (`themes/*.json`) |

A host that draws SolidCraft's icons plus a few masters of its own installs
`solidcraft_ui_kit::icons::IconSet::extended(solidcraft_look::icons::DATA, extension)` with
`solidcraft_ui_kit::icons::install`, then the other parts (`glyphs::install`, `themes::install`,
`fonts::install`). The extension's masters, aliases and default accents win a name both have.

The workspace tab table is the app's, not the look's. The accent ramps are in `solidcraft-theme`.

Read `icons/AGENTS.md` and `icons/STYLE_GUIDE.md` before icon work.

## Licence

The crate's licence expression is `(MIT OR Apache-2.0) AND LicenseRef-SolidCraft-Brand AND OFL-1.1`,
because it ships three kinds of file:

- **Code** (`src/`, `build.rs`, the icon tools in `icons/design/tools/`), the palette and alias
  tables and the built-in themes (`themes/*.json`): MIT OR Apache-2.0, like the rest of SolidCraft.
- **Interface icon artwork**, the SVG masters in `icons/design/svg/`: the SolidCraft Brand Licence
  ([LICENSE-BRAND](../../LICENSE-BRAND)), except masters whose row in ATTRIBUTION.md names another
  licence. Use them as part of SolidCraft; using them in another program needs the maintainers'
  written permission first. A host that installs the kit with its own icon set uses none of them.
- **IBM Plex faces** (`fonts/`): SIL OFL 1.1, with the licence text beside each (`fonts/*/OFL.txt`).

Every file has a row in the repository's ATTRIBUTION.md naming its licence.
