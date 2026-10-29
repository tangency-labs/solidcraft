# Render workspace icon drafts

All ids below are **Open** for owner review, except `render_scene` and `render_image`,
which are existing accepted icons used only as references. No commands, toolbar
entries, browser selectors or runtime request manifests are changed by this draft set.
`render_remove_override` is the one remaining alias (of `render_from_material`);
`render_execute`, `render_environment` and `render_fit_product` now have their own
masters in `design/svg/model/`. Accepted source masters stay unchanged.

Review with `python3 -B assets/icons/ui/design/tools/icon_status.py --serve`,
search `render_`, and select Open. See [STYLE_GUIDE.md](STYLE_GUIDE.md) for
acceptance; drafts do not imply implemented commands.

The finished references in each row were read and previewed in both themes
before authoring. Draft contact sheets (96, 24 and 16 px, both themes) are local
review evidence, not shipped assets. The icons are grouped in five families, each
with one shared design rule.

## Lights

Rule: a slate source glyph with light passive fill, 2.55-unit outlines and a dotted
accent stem (three dots, 4.25 diameter, 7 units apart) showing the direction.

| Meaning | Id | Finished references | Drawing |
|---|---|---|---|
| Light area | `render_light_area` | `box`, `camera` | Slanted flat panel on the canonical axes above the dotted stem. |
| Light spot | `render_light_spot` | `camera`, `material` | Cone with a curved rim above the dotted stem. |
| Light point | `render_light_point` | `camera`, `material` | Round source disc with a centre dot above the dotted stem. |
| Sun | `render_sun` | `sphere`, `camera` | Source disc of the same size with eight short radial rays instead of a stem. |
| Light by highlight | `render_light_by_highlight` | `sphere`, `material` | Shaded sphere with a ringed highlight target; the dotted stem arrives from above. |
| Solo light | `render_solo_light` | `fit`, `camera` | Point source and stem inside four corner focus brackets. |

## Looks

Rule: the lit look sphere (top-left highlight, bottom-right shade) plus one cue that
says what happens to it; receiving bodies stay canonical grey or cube shapes.

| Meaning | Id | Finished references | Drawing |
|---|---|---|---|
| Appearance | `render_appearance` | `material`, `box` | Shaded look sphere on the top corner of an accent-shaded cube: a look assigned to a body. |
| Paint | `render_paint` | `material`, `paste` | Flat brush with an accent tip, light ferrule and slate handle. |
| Pick up | `render_pick_up` | `material`, `copy` | Eyedropper with a grey bulb, slate crossbar and accent sampling tip. |
| Replace all | `render_replace_all` | `copy`, `box` | Grey sample sphere with an arrow to an accent sphere, beside a second accent sphere. |
| Copy look | `render_copy_look` | `material`, `copy` | Look sphere in a square frame, offset from a second frame outline. |
| Remove override | `render_remove_override` | `appearance`, `box` | Alias of `render_from_material`: the custom look is crossed off. |
| Same look | `render_same_look` | `select`, `box` | Look sphere and three cubes; the two matching cubes are ringed. |
| From material | `render_from_material` | `material`, `extrude` | Look sphere struck through above a plain grey slab, so the material shows. |

## Studio

Rule: the standard grey cube stands in the set-up it is lit or placed in.

| Meaning | Id | Finished references | Drawing |
|---|---|---|---|
| Library | `render_library` | `library`, `copy` | Row of three look spheres, the middle one accent, on a shelf bar. |
| Lighting presets | `render_lighting_presets` | `new`, `render_scene` | Two stacked cards, the front one holding a light bulb. |
| Environment | `render_environment` | `environment_map`, `render_scene` | Dome with a grey cube inside. |
| Ground | `render_ground` | `box`, `render_scene` | Grey cube on a slab with an accent contact patch. |
| Backdrop | `render_backdrop` | `render_scene`, `render_image` | Grey cube in front of a tall upright accent panel. |

## Camera and shots

Rule: one chunky camera body with a lower-right round badge for shot actions; the
other camera icons are simple frames.

| Meaning | Id | Finished references | Drawing |
|---|---|---|---|
| Shot | `render_shot` | `camera`, `new` | Camera body with a plus badge: save a shot. |
| Update shot | `render_update_shot` | `camera`, `undo` | Camera body with a refresh badge. |
| Render all shots | `render_all_shots` | `copy`, `render_image` | Stacked camera bodies with a play badge. |
| Shots strip | `render_shots_strip` | `camera`, `copy` | Strip of three small frames, the middle one in accent. |
| Fit product | `render_fit_product` | `fit`, `camera` | Grey cube inside four corner brackets. |
| Lens / field of view | `render_lens` | `camera`, `fit` | Concentric lens rings around an accent centre. |
| Exposure | `render_exposure` | `material`, `environment_map` | Disc split light and accent, with plus and minus marks. |
| Aspect frame | `render_aspect_frame` | `render_image`, `fit` | Wide rounded frame with two opposite corner brackets. |
| Frame guides | `render_frame_guides` | `render_image`, `fit` | Rounded frame with a rule-of-thirds grid and four accent nodes. |

## Output and result

Rule: one picture base (frame with landscape) with a lower-right round badge; the
size, background and quality icons draw their own simple subject.

| Meaning | Id | Finished references | Drawing |
|---|---|---|---|
| Render image | `render_execute` | `render_image`, `camera` | Picture with a play badge: create a rendered image. |
| Render output size | `render_output_size` | `render_image`, `fit` | Picture frame with a diagonal double-headed dimension arrow. |
| Transparent background | `render_transparent_background` | `material`, `render_image` | Grey cube on a checker tile. |
| Quality | `render_quality` | `list`, `library` | Three rising bars, the tallest in accent. |
| Check render | `render_check` | `render_image`, `new` | Picture with a check badge. |
| Cancel render | `render_cancel` | `render_image`, `delete` | Picture with a square stop badge. |
| Export image | `render_export_image` | `render_image`, `copy` | Picture with a large outward diagonal arrow at the upper right. |
| Compare | `render_compare` | `render_image`, `material` | Two grey pictures split by a vertical slider with opposed arrows. |
| Results | `render_results` | `render_image`, `copy` | Stack of two pictures, the front one showing a landscape. |
