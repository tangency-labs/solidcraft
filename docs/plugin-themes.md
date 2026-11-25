# Plugin themes (`themes@0.1`)

A plugin can add colour themes to SolidCraft's theme picker. A theme is a
`solidcraft-theme/1` file in the plugin's folder, the same format as an imported theme. The host
reads it with its own theme reader and lists it after the built-in themes, labelled with the
theme's `name`. A plugin with themes needs no runtime.

## The manifest

`capabilities` must list `"themes@0.1"`. Each entry of `themes`:

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Lowercase letters, digits and `-`, unique in the plugin. The preference names it `plugin:<plugin-id>/<id>`. |
| `source` | yes | A `.json` file name in the plugin's folder (lowercase, no folders). |

A plugin declares at most 8 themes. Each theme file is at most 64 KiB.

```json
{
  "id": "solid-glass",
  "name": "Solid Glass",
  "version": "1.0.0",
  "capabilities": ["themes@0.1"],
  "themes": [{"id": "solid-glass", "source": "solid-glass.json"}]
}
```

## The theme file

A theme file is a `solidcraft-theme/1` file: `"format": "solidcraft-theme/1"`, a `name`, a `base`
(`"dark"` or `"light"`), and the colours, radii and borders a theme sets. The format is described in
[the control protocol](control-protocol.md) (`theme_import`) and [the UI tour](book/src/guide/ui-tour.md). A
file the host's reader refuses is not offered.

## What people see

- The theme picker lists a plugin's themes after the built-in and imported ones, while the plugin is
  turned on.
- The plugin's card in Preferences > Plugins has a line "Themes: <names>".
- The install review lists the theme ids the package offers.

## When a theme is not available

A chosen plugin theme stays chosen while its plugin is on. When its plugin is turned off or removed,
or its file can't be read, the default theme is turned on and the status bar says which theme is not
available. The offer is read again only when a plugin is installed, removed or switched, so the
status line appears once.

Turning the plugin back on lists the theme again; the default theme stays on until the person
chooses the plugin theme again.

## Versions

`themes@0.1` is additive: a plugin with themes is refused by a host that lacks the capability, with
the usual "update SolidCraft or the plugin" message. The manifest fields are optional, so existing plugins are unchanged.
