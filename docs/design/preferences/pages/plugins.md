# Plugins

Board: `Prefs2_Plugins` (1920x876). Heading: Extend. Subtitle "Sandboxed add-ons." No "In profile"
tag. Source today: Plugins page (`crates/ui-egui/src/plugins.rs`, `Prefs.disabled_plugins`, page
`PAGE_PLUGINS`).

## Toolbar of the page

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Search plugins | Search field with magnifier, full width | filters the cards only | empty | none | none | |

## Installed (3)

One card per installed plugin: 12 px padding inside its border, 36 px icon tile, 12 px gaps, 13 px semibold title, 11 px version and 12 px description. The text lines have a 2 px gap. The icon, text block, unframed 24 px folder action and 16 px checkbox are vertically centred. Cards grow for wrapped text. Disabled plugins use muted content.

| Part | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Name, version, description | Text from the plugin manifest | none | n/a | none | none | Build from installed plugins. The current manifest has no permission metadata, so omit permission badges. |
| Show in folder | Unframed action with folder icon | opens the plugin's folder | n/a | none | none | |
| Enabled | Checkbox (16 px) | `Prefs.disabled_plugins` (a plugin is enabled when its id is not in the set) | enabled | none | none | |

## Footer row

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Show plugins folder | Button with folder icon | opens the plugins folder | n/a | none | none | Creates the folder if needed. Per-plugin Show in folder opens that plugin's own folder. |
| Restore page defaults | Shared footer action | clears `Prefs.disabled_plugins` | empty (all plugins enabled) | none | none | Retain the existing action and Cancel transaction semantics alongside Cancel and OK. |

## Mismatches

* Plugin cards are sample content (see README).
* The board footer omits "Restore page defaults"; the retained disabled-plugin preference does
  have a default, so the implementation keeps the shared action.
