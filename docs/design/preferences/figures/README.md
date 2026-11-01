# Preferences figures

Every illustration on the Preferences boards, as a standalone SVG with the board's own geometry.
`<name>.light.svg` is the figure as drawn in the light dialog and `<name>.dark.svg` as drawn in the
dark dialog. Open any file in a browser to see it. They were extracted verbatim from the boards: only
the wrapper styles were removed (width 100 %, aspect ratio, corner radius), the `xmlns` was added and
`width` and `height` were set to the `viewBox` size.

The figures are original artwork made for SolidCraft; the Orientation pair is copied from the app.
Their ATTRIBUTION row is in `attribution/pending/preferences-v2.md`.

Not here: the 24x24 line icons on the boards (page-list icons, section icons, the (i) help icon, the
chevrons and checks). They are stand-ins for the app's own icon set, drawn with
`stroke="currentColor"`; use the app's icons. Also not SVG on the boards, and so not here: the
navigation-bar preview strip and slot chips, the theme-options controls, the live-input bars and the
shortcut key caps. Those are HTML on the boards; `pages/*.md` and `measurements.md` give their sizes.

## House style

* **Canvas** 120x64 for card figures. Exceptions: theme thumbnails 124x76, Orientation 120x80,
  Layout schematic and the History and Browser mini-schematics 226x148, mouse figure 96x120, sketch
  exit panels 30x30 nested inside a 120x64 canvas, colour vision samples 84x44, agent pointer 230x84,
  style icon 64x64.
* **Ground**: a full-canvas rectangle (light `#F7F8FA`, dark `#1C1F26`) under every card figure.
* **Cubes** are isometric (30 degree axes) in three fixed tones: top face lightest, left face mid,
  right face darkest. Light: `#FFFFFF`, `#E4E9F0`, `#C9D2DE`. Dark: `#6B7790`, `#515B72`, `#394256`.
  Outline 0.9, round joins, `#6F7C8F` light and `#A3AEC2` dark.
* **Accent blue** (`#236EAD` light, `#8CBEF5` dark) marks what the setting acts on.
* **Amber** (`#8A5300` light, `#E8C24A` dark) marks a reference point: the pivot, the origin, the
  pointer.
* **Red, green, blue** appear only for the X, Y and Z axes (values in `tokens.md`).
* **Green, amber, red** mean ok, warning, error. **Grey** is context.
* **Line weights**: 0.9 for outlines and dashed reference lines, 1.2 to 1.6 for emphasis, 2 for the
  thing being shown (an orbit arc), 2.4 for the colour vision samples. Dashes `3 3` for reference
  geometry. Text inside figures is IBM Plex Mono at 6.5 to 7 px, for tiny captions only (the `16`
  under the tessellation figures, `before`, `sketch`, `after`).
* **Corners**: the card rounds the figure (5 px on the board); the figure itself is square.
* The Anti-aliasing figures are pixel tiles: 6x6 squares of the accent at 0.25, 0.5, 0.75 and 1.0
  opacity along an edge, so they are long (8 to 9 KB) but regular.

## Painters

Step 2 of #9863 paints these in `crates/ui-egui/src/ui_kit/preferences_figures.rs`, one function
per figure, with `Tokens` colours instead of the literals in these files. `tokens.md` lists which
token each literal maps to.

## Index

| File name | Canvas | Board | Shows |
|---|---|---|---|
| `theme-light` | 124x76 | Prefs2_Appearance | Theme carousel card, built-in Light theme |
| `theme-dark` | 124x76 | Prefs2_Appearance | Theme carousel card, built-in Dark theme |
| `theme-system-light-half` | 124x76 | Prefs2_Appearance | Theme card "System": light half (clipped to one triangle of the card) |
| `theme-system-dark-half` | 124x76 | Prefs2_Appearance | Theme card "System": dark half (the other triangle) |
| `theme-gruvbox` | 124x76 | Prefs2_Appearance | Theme carousel card, Gruvbox (has options) |
| `theme-imported` | 124x76 | Prefs2_Appearance | Theme carousel card, imported file "Studio glass" (sample) |
| `colour-vision-fixed` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Fixed" |
| `colour-vision-projected` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Projected" |
| `colour-vision-axes` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Axes" |
| `colour-vision-bends` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Bends" |
| `colour-vision-draft` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Draft" |
| `colour-vision-failed` | 84x44 | Prefs2_Appearance | Colour vision sample cell "Failed" |
| `agent-pointer-speed` | 230x84 | Prefs2_Assistant | Assistant > Agent pointer: the pointer path preview |
| `orientation-z-up` | 120x80 | Prefs2_Design | Design > Orientation card "Z up" (copied from the app) |
| `orientation-y-up` | 120x80 | Prefs2_Design | Design > Orientation card "Y up" (copied from the app) |
| `tess-coarse` | 120x64 | Prefs2_Graphics | Curved surfaces card "Coarse" |
| `tess-normal` | 120x64 | Prefs2_Graphics | Curved surfaces card "Normal" |
| `tess-fine` | 120x64 | Prefs2_Graphics | Curved surfaces card "Fine" |
| `tess-ultra` | 120x64 | Prefs2_Graphics | Curved surfaces card "Ultra" |
| `aa-off` | 120x64 | Prefs2_Graphics | Anti-aliasing card "Off" |
| `aa-2x` | 120x64 | Prefs2_Graphics | Anti-aliasing card "2x" |
| `aa-4x` | 120x64 | Prefs2_Graphics | Anti-aliasing card "4x" |
| `aa-8x` | 120x64 | Prefs2_Graphics | Anti-aliasing card "8x" (badge "not on this GPU") |
| `view-grid` | 120x64 | Prefs2_Graphics | Current view tile "Grid" |
| `view-shadow` | 120x64 | Prefs2_Graphics | Current view tile "Ground shadow" |
| `view-perspective` | 120x64 | Prefs2_Graphics | Current view tile "Perspective" |
| `ao-on` | 120x64 | Prefs2_Graphics | Current view tile "Ambient occlusion" |
| `style-shaded-edges` | 64x64 | Prefs2_Graphics | Visual style dropdown icon, 64x64 |
| `view-angles` | 120x64 | Prefs2_Graphics | In the view tile "View angles" |
| `vertex-markers` | 120x64 | Prefs2_Graphics | In the view tile "Vertex markers" |
| `quick-measure` | 120x64 | Prefs2_Graphics | In the view tile "Quick measurement" |
| `frame-rate` | 120x64 | Prefs2_Graphics | In the view tile "Frame rate" |
| `layout-schematic` | 226x148 | Prefs2_Layout | Layout > Panels: schematic of the window (226x148, shown 262 px wide) |
| `history-bottom` | 226x148 | Prefs2_Layout | History card "Bottom" |
| `history-left` | 226x148 | Prefs2_Layout | History card "Left" |
| `history-under-browser` | 226x148 | Prefs2_Layout | History card "Under browser" |
| `history-in-browser` | 226x148 | Prefs2_Layout | History card "In browser" |
| `browser-column` | 226x148 | Prefs2_Layout | Browser card "Column" |
| `browser-card` | 226x148 | Prefs2_Layout | Browser card "Card" |
| `mouse-mapping` | 96x120 | Prefs2_Navigation | Navigation > Mouse: the mouse button figure |
| `orbit-origin` | 120x64 | Prefs2_Navigation | Orbit around card "Origin" |
| `orbit-centre` | 120x64 | Prefs2_Navigation | Orbit around card "View centre" |
| `orbit-cursor` | 120x64 | Prefs2_Navigation | Orbit around card "Under the cursor" |
| `navigator-axes` | 120x64 | Prefs2_Navigation | View navigator card "Axes, signed" |
| `navigator-cube` | 120x64 | Prefs2_Navigation | View navigator card "Cube, compact" |
| `sketch-guides` | 120x64 | Prefs2_Sketch | Sketch > Snapping and guides figure |
| `sketch-exit-previous` | 120x64 | Prefs2_Sketch | "When a sketch ends" card "Return to the view before" (before / sketch / after trio) |
| `sketch-exit-keep` | 120x64 | Prefs2_Sketch | "When a sketch ends" card "Keep" (default) |
| `sketch-exit-unless-turned` | 120x64 | Prefs2_Sketch | "When a sketch ends" card "Unless turned" |
| `sketch-state-icons` | 120x64 | Prefs2_Sketch | Marks in the sketch tile "State icons" |
| `sketch-conflict` | 120x64 | Prefs2_Sketch | Marks in the sketch tile "Conflict colour" |
| `sketch-hide-glyphs` | 120x64 | Prefs2_Sketch | Marks in the sketch tile "Hide glyphs zoomed out" |
| `select-faces-edges` | 120x64 | Prefs2_Sketch | Click selects card "Faces and edges" |
| `select-bodies` | 120x64 | Prefs2_Sketch | Click selects card "Whole bodies" |
| `tool-hole` | 120x64 | Prefs2_Tools | Tools > Create: Hole figure |
| `tool-extrude` | 120x64 | Prefs2_Tools | Tools > Create: Extrude figure |
| `tool-placement` | 120x64 | Prefs2_Tools | Tools > Create: Rest, Boss, Snap Fit figure |
| `assembly-move` | 120x64 | Prefs2_Tools | Tools > Assembly Move figure |
| `measure-axis-distances` | 120x64 | Prefs2_Tools | Measure and pointer tile "Axis distances in Measure" |
| `tool-cursor` | 120x64 | Prefs2_Tools | Measure and pointer tile "Tool at the cursor" |
| `direct-interaction` | 120x64 | Prefs2_Tools | Measure and pointer tile "Direct interaction" |
