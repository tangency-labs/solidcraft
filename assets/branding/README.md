# SolidCraft brand kit

Logo files for stickers, shirts, the web and social posts. Everything in `logo/` is drawn
from the app icon (`assets/icons/app/macos/source/SolidCraft.icon/Assets/03-solid-blue-cutout.svg`),
which is also the last frame of the loading animation (`assets/styles/web/loading-logo.svg`).
The SVGs are plain paths: no scripts, fonts, animation or external links, so any printer can use them.

## Files

| File | Use it for |
|---|---|
| `logo/solidcraft-mark.svg` | The mark in full colour. First choice for stickers, print and the web. |
| `logo/solidcraft-mark-mono-black.svg` | One-colour print on light material: screen-printed shirts, stamps, engraving, vinyl. |
| `logo/solidcraft-mark-mono-white.svg` | One-colour print on dark material, such as a white print on a dark shirt. |
| `logo/solidcraft-mark-512.png`, `-1024.png`, `-2048.png` | Transparent square PNGs for places that don't take SVG: avatars, social posts, slides, print shops that ask for a bitmap (send the 2048). |
| `logo/solidcraft-wordmark.svg` | The name on its own, outlined to paths. |
| `logo/solidcraft-lockup-horizontal.svg` | Mark and name side by side: headers, banners, shirt fronts, wide stickers. |
| `web/source/og.png` | The existing link preview card for the docs site. |

The gaps between the faces are part of the logo. In the colour and one-colour files they are
transparent, so the material shows through. For a die-cut sticker, cut around the outer shape
and let the backing colour (white is best) fill the gaps.

## Colours

The mark's five faces each use a two-stop gradient from the icon source:

| Face | From | To |
|---|---|---|
| Top | `#51adff` | `#2b83f1` |
| Left | `#258df5` | `#1461da` |
| Right | `#1b66cf` | `#124596` |
| Lower step (top) | `#459fff` | `#2379e7` |
| Upper step (inner side) | `#1d63c6` | `#15499e` |

Other colours, all already used in the project:

| Colour | Hex | Where it comes from |
|---|---|---|
| Wordmark blue | `#15499e` | loading logo `--word` |
| Interface accent | `#2678da` | light theme `accent` token (`assets/styles/ui/palettes.rs`) |
| Light background | `#e8ebf0` | web loading screen (`assets/styles/web/loading.css`) |
| Dark background | `#1f232b` | dark theme `file_background` token |
| Black / white | `#000000` / `#ffffff` | one-colour versions only |

If a printer can't do gradients, use the one-colour files, or fill every face with `#2678da`.

## Clear space and size

- Keep empty space around the mark of at least one fifth of its height on every side. Treat
  the lockup the same way, measured from the mark.
- Smallest mark: 16 px tall on screen, 12 mm tall in print. Below that the gaps fill in.
- Smallest lockup: 120 px wide on screen, 40 mm wide in print.

## Don'ts

- Don't recolour it outside the colours above.
- Don't stretch, squash, rotate, skew or crop it. Scale it evenly.
- Don't add shadows, outlines, glows, bevels or other effects.
- Don't fill or close the gaps between the faces, or move the faces apart.
- Don't rearrange the lockup, retype the name in another font, or put the mark on a busy
  background where it loses contrast.

## Name and logo

The SolidCraft code and these files are licensed MIT OR Apache-2.0 (see the [README](../../README.md#licence)),
so you may copy and change them. The name SolidCraft and this logo identify the SolidCraft
project. Use them to refer to the project, for example in an article, a talk, or a sticker for a
meetup. If you publish a fork or a modified build, give it its own name and logo so people
don't mistake it for SolidCraft.

The wordmark is set in IBM Plex Sans Bold (SIL Open Font License 1.1, the interface font in
`assets/fonts/ibm-plex-sans/`) and outlined, so you don't need the font to print it.
