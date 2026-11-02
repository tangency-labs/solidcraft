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
| Name, version, permission badge, description | Text and a badge ("No permissions", "Reads files") | none (from the plugin manifest) | n/a | none | none | Sample content: Gear generator 1.2.0 "Adds a parametric gear feature.", Bracket tools 0.4.1 "Batch-place mounting brackets." (Reads files), Thread library 2.0.0 "Standard thread profiles.". Build from the installed plugins. |
| Show in folder | Small button (114.6 x 24) | opens the plugin's folder | n/a | none | none | |
| Enabled | Checkbox (16 px) | `Prefs.disabled_plugins` (a plugin is enabled when its id is not in the set) | enabled | none | none | |

## Footer row

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Show plugins folder | Button (152 x 28) | opens the plugins folder | n/a | none | none | The page has no "Restore page defaults" button on the board; the footer shows "Cancel" and "OK" only. |

## Mismatches

* Plugin cards are sample content (see README).
* The board footer omits "Restore page defaults" because nothing on this page has a default.
