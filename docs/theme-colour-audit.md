# Theme colour audit (#5007)

The audit covers all 20 shipped theme files and the shared SVG icon/tab rendering paths.
This work stacks on the theme radii and border changes (#4923, #4954).

## Findings and decisions

- Command SVGs previously read only the compiled light/dark palette. The existing
  `colors.icon` and `colors.icons.*` settings colour other markers/controls, not
  SVG active faces, passive faces or contours. Optional `icon_palette` overrides
  now supply those roles without editing accepted artwork or semantic aliases.
- Texture identity previously contained light/dark mode, semantic accent, size and
  finish, but no theme palette. It now includes the resolved palette; switching
  palettes frees the previous theme's cached textures. Forced-dark app-bar icons
  coexist with themed icons without repeatedly clearing the cache.
- Eleven files repeated one fill across Surface, Mesh, Form, Sheet Metal and Render.
  Six Gruvbox files inherited those workspace settings from the default UI palette.
  All files now define the seven workspace colours, paired ink and soft surfaces.
- The six Gruvbox variants omitted form/dialog tokens, leaving Preferences and
  other form surfaces in the default light/dark palette. They now explicitly
  colour dialog bodies, footers, rules, controls, text, selection and status states.
- Text on soft Model selections incorrectly reused solid-selection ink even when
  its contrast was insufficient (notably 95). The tab chooses a readable theme ink.
  Selected preview metadata now uses selected text ink instead of dim body text.

| Family | Colour treatment |
| --- | --- |
| Gruvbox dark/light, normal/hard/soft | Blue, orange, yellow, purple, aqua, red and green from the family palette; shared accent geometry across contrast variants |
| Nord, Tokyo Night, Catppuccin Mocha, Nosferatu | Distinct workspace hues within each palette's muted or pastel character |
| Cupertino, One Dark | Retain their semantic workspace families, with matching shaded SVG ramps |
| Blueprint | Chalk-like faces, blue passive geometry and restrained workspace hues |
| Graphite, Platinum 9 | Muted, distinct workspace colours over neutral chrome |
| Solarized Light | Solarized workspace hues and warm neutral icon faces |
| High Contrast | Strong outlines and readable saturated workspace colours |
| SolidCraft 95 | Classic grey icon geometry; navy, brown, olive, purple, teal, burgundy and green workspace accents |
| Amber/Green Phosphor | Deliberately single-hue palettes, with different intensities for workspace groups |

Shared utility tabs (Assemble, Insert, Inspect, Manage, View) retain the Model
family. The seven workspace groups are Model, Surface, Mesh, Form, Sheet Metal,
Render and Sketch. Their labels remain the primary identifier; colour supplements them.

## Readable pairs on the shared fills

`form_primary` is meant for text on a pale `selected_bg`, and a theme may make the two
the same (SolidCraft 95's navy and CGA do). `Tokens::selected_fg` keeps the primary unless it is
as good as gone, under 2:1, and then takes `selected_ink`, for selected tabs, menu rows and text
selected in a form. `Tokens::readable` does the same for any ink on a known fill, and
`Tokens::on_view` for text straight on the 3D view, at 2.5:1. A pair above those floors is how a
theme looks and stays as it is, even under 4.5:1, so Dark, Light and most themes are unchanged.

Icons have their own palette, so nothing above reaches them. Under the same 2:1, a selected icon,
including a pressed ribbon button's, takes one ink, the one its label uses, through
`Tokens::selected_icon_ink` and `icons::paint_on_selected`; other themes keep the artwork's
colours. In SolidCraft 95 the strong stroke of every ramp reaches 4.5:1 on a button: Surface, Mesh,
Sketch and construction take the theme's darker workspace inks.

SolidCraft 95's view is a mid-tone teal, so the classic mid-tone green, olive and purple vanish
on it. What is drawn on the view takes the light half of the 16-colour palette instead, or the
pale `#ffc0ff` of its Form ramp for projected and lost links, and reaches 3:1. `Tokens::sketch_stroke`
gives geometry being drawn `preview_sketch` where `sketch_accent`, kept for the ribbon and panels,
does not show on the view. The bright colours stand on silver bodies by hue rather than brightness;
the Graphics preference Halo under sketch curves draws a black rim beneath them. Axis colours stay
red, green and blue by convention: red and blue on teal are 1.2:1 and 1.8:1, separated by hue.

## Theme file contract

`solidcraft-theme/1` accepts an optional `icon_palette` object. Single opaque hex
colours override `ink`, `line`, `guide`, `passive-1/2/3` and `active-1/2/3`.
Three-colour arrays, ordered pale face / medium face / strong stroke, override
`solid`, `surface`, `mesh`, `form`, `sketch`, `constraint` and `construction` ramps.
A missing role/ramp retains the compiled palette. A base Solid override never
recolours an omitted semantic ramp. Unknown names, incorrect array lengths,
translucency and overrides of the transparent GAP eraser are rejected.

Import/export and direct preference updates use the same validation. Invalid
hand-edited saved palettes fall back safely. The default Light/Dark palettes stay
unchanged. SVG outlines, guides, passive face ordering, masks, knockouts and alpha
remain intact. Explicit monochrome controls keep their surface-specific ink;
colour-vision variants take precedence over conflicting theme ramp entries.

Most custom ramps match the base ramp's OKLab lightness per face, with chroma
reduced to stay in sRGB. Phosphor intensities and 95's classic palette are deliberate
exceptions. Workspace ink/soft and solid-tab pairs are checked for 4.5:1 text contrast.

## Separate work

Home/File colour inheritance is owned by #4868 / #4924 and is not duplicated here.
Radii, borders, bevels and SVG artwork/assignments remain owned by their existing changes.
