# Plugin effects: host-drawn looks (`effects@0.1`)

A plugin can offer the host an effect that the host draws itself. The first kind is `hue-wave`,
the RGB wave: the ribbon's command icons, button blocks and UI panels take a hue from their screen
position and the time, so one colour wave runs across the window. The plugin does not draw it; running
plugin code per widget is not practical, so the plugin only switches the host's effect on.

## Manifest

```json
"capabilities": ["effects@0.1"],
"effects": [{"id": "rgb-wave", "name": "RGB wave", "kind": "hue-wave"}]
```

- `id` is unique within the plugin and a valid id; `name` is 1 to 80 printable characters.
- `kind` must be a kind this version of SolidCraft draws. Today only `hue-wave`. A plugin with another
  kind is refused at install with "needs effect kind X; update SolidCraft or the plugin".
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

No plugin asking for `effects@0.1` changes nothing: the wave stays off and Preferences shows no effect rows.
