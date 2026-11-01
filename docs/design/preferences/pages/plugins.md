# Plugins

Board: `Prefs2_Plugins` (1920x876). Heading: Extend. Subtitle "Sandboxed add-ons." No "In profile"
tag. Source today: Plugins page (`crates/ui-egui/src/plugins.rs`, `Prefs.disabled_plugins`, page
`PAGE_PLUGINS`).

## Toolbar of the page

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Search plugins | Search field, full width | filters the cards only | empty | none | none | |

## Installed (3)

One card per installed plugin, 61.7 px high.

| Part | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Name, version, description | Text from the plugin manifest | none | n/a | none | none | Build from installed plugins. The current manifest has no permission metadata, so omit permission badges. |
| Show in folder | Small button (114.6 x 24) | opens the plugin's folder | n/a | none | none | |
| Enabled | Checkbox (16 px) | `Prefs.disabled_plugins` (a plugin is enabled when its id is not in the set) | enabled | none | none | |

## Footer row

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Show plugins folder | Button (152 x 28) | opens the plugins folder | n/a | none | none | Creates the folder if needed. Per-plugin Show in folder opens that plugin's own folder. |
| Restore page defaults | Shared footer action | clears `Prefs.disabled_plugins` | empty (all plugins enabled) | none | none | Retain the existing action and Cancel transaction semantics alongside Cancel and OK. |

## Mismatches

* Plugin cards are sample content (see README).
* The board footer omits "Restore page defaults"; the retained disabled-plugin preference does
  have a default, so the implementation keeps the shared action.
