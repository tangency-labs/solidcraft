# Plugin effects: host-drawn looks (`effects@0.1`)

A plugin can offer the host an effect that the host draws itself. The kinds are `hue-wave`,
`galaxy` and `rain`. `hue-wave` is the RGB wave: the ribbon's command icons, button blocks and UI panels take a
hue from their screen position and the time, so one colour wave runs across the window. `galaxy` is a
sky behind the model. `rain` is rain on a frosted window in front of a picture. The plugin does not draw any of them; running plugin code per widget or per pixel is
not practical, so the plugin only switches the host's effect on.

## Manifest

```json
"capabilities": ["effects@0.1"],
"effects": [{"id": "rgb-wave", "name": "RGB wave", "kind": "hue-wave"}]
```

A galaxy, with a picture from the plugin's own package (optional):

```json
"capabilities": ["effects@0.1"],
"effects": [{"id": "stars", "name": "Galaxy", "kind": "galaxy", "picture": "sky/galaxy.jpg"}]
```

- `id` is unique within the plugin and a valid id; `name` is 1 to 80 printable characters.
- `kind` must be a kind this version of SolidCraft draws: `hue-wave`, `galaxy` or `rain`. A plugin with another
  kind is refused at install with "needs effect kind X; update SolidCraft or the plugin".
- `picture` is optional and only for a kind that shows one (`galaxy`, `rain`): a `.png`, `.jpg` or `.jpeg`
  file in the plugin's own package, as a relative path with `/` and no `..`. The install checks that the
  file is in the package and is a PNG or JPEG, like the plugin's other files; a package that fails is
  refused. SolidCraft ships no pictures for effects.
- At most 8 effects per plugin. The capability is needed for any effect.
- A plugin may have effects and no runtime, and it may also have commands.

## Choosing one

On the plugin's card in Preferences > Plugins, each effect has a switch, "Effect: <name>". One effect
is on at a time (the preference `ui_effect`, `"plugin-id/effect-id"`, default empty). When it is
on, its settings show under the switch:

- `ui_effect_icons`, `ui_effect_buttons`, `ui_effect_panels`: which parts the wave tints (ribbon
  icons, ribbon button blocks, UI panels). All default off.
- `ui_effect_length` (300 to 8000 points, default 2400): how long one colour cycle is on screen.
- `ui_effect_speed` (10 to 1500 points a second, default 150): how fast the wave travels.

The wave also tints the logo while it is on. Nothing is added to the Interface page.

## Galaxy

While a `galaxy` effect is chosen and its plugin is on, the host draws a sky behind the model, in
place of the view's background and the Background picture preference. With a `picture`, the host shows
it: a picture about twice as wide as tall (2:1) is a 360 panorama that turns with the view, and any
other picture covers the window from its centre. Without a `picture` the host draws its own night sky
of stars and a faint band of dust, so no picture is needed. A picture that cannot be read is reported
in the status bar and the host's sky is drawn instead.

A glass theme's panels see the sky too. The galaxy has no settings.

## Rain

A `rain` effect shows a flat picture behind a frosted window with rain on it. A picture covers the
window from its centre; without a `picture` the host draws a dark night with soft lights in it. The
drops are simulated on the CPU and drawn by the host: small drops land and stay, drops that meet
merge, one heavy enough slides and wipes a trail, and a sliding drop leaves only by merging into a
bigger drop or running off the bottom. When the glass is full, new rain joins the nearest drop. Glass
panels see the rain too.

While it is chosen, its settings show under the switch:

- `rain_amount` (0 to 1, default 0.6, "Rain amount"): how hard it rains; 0 lets no new drops fall.

The rain redraws about 30 times a second while the window is focused and 10 when it is not.

The wave draws only while the chosen effect's plugin is installed and turned on. Turning the plugin
off, or removing it, stops it at once, and the choice stays so that turning it on again resumes it.

Scripts and agents set the same preferences with `ui.prefs`, for example
`ui.prefs {"ui_effect": "rgb-wave/rgb-wave", "ui_effect_icons": true}`.

## Install review

The install confirmation lists a plugin's effects under "Visual effects".

## Limits

The host checks the manifest when the plugin is installed. It runs no plugin code to draw an effect.
An effect that the host cannot draw leaves the window as it was, and its plugin's card still lists the
switch.

No plugin asking for `effects@0.1` changes nothing: the wave stays off, no sky is drawn and Preferences
shows no effect rows. A plugin whose effects are `hue-wave` only installs and draws exactly as before.
