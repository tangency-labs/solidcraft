# Design configurations

The configuration table stores named design variants. Add parameter, feature suppression,
physical material, body appearance, component appearance, and semantic face appearance columns
with `config.configure` or `config.column`.
The desktop Configuration Table's **Add Column** menu offers the same targets. Material and
appearance cells have a **Library** picker; their text fields also accept exact saved values.

```json
{"command":"config.configure","params":{"params":["width"],"materials":["Box1"],"appearances":["Box1"]}}
{"command":"config.row","params":{"name":"Aluminum blue","values":{"param:width":"80 mm","material:Box1":"Aluminum","appearance:Box1":"Aluminum - Anodized Blue"}}}
{"command":"config.activate","params":{"row":"Aluminum blue"}}
```

Use the body's actual Browser name in place of `Box1`. `config.table` returns the column keys,
rows, active name, and `matches`: whether the current design still has the active row's values.
Changing an assignment by hand makes `matches` false. Activating another row is undoable.

Material values are names from `material.list` (case insensitive), with `Default` clearing the
body's physical material assignment. Appearance values are names from `appearance.library`,
an empty string or `Default` to clear the body's appearance, or a custom look object:

```json
{"command":"config.row","params":{"name":"Transparent blue","from":"Aluminum blue","values":{"appearance:Box1":{"name":"Custom blue","color":[40,90,186],"opacity":0.4}}}}
```

The table stores custom looks without losing colour or opacity. `color` contains three byte
values, names support up to 128 Unicode characters, and `opacity` must be finite and between
zero and one. Style values are validated when
editing a row and again when activating a saved row. An invalid value leaves the design unchanged.
Clearing a body appearance allows component appearance or material colour to show through;
face appearances keep their existing priority. Physical material changes affect measured mass.

`config.row` supports `from` to copy another row, `rename` to rename an existing row,
`index` to move an existing row to that place in the table (0 is the first; the other rows keep
their order and the active row stays active), and `delete` to remove it. Names must be unique.
In the table, double-click a row's name to rename it, and use its up and down arrows to move it. To remove a style column, use
`config.column` with `material` or `appearance` set to the target body and `delete: true`.

Each row also has a fixed id, which `config.table` lists with the row. A rename keeps the id.
Deleting a row does not recycle its id, but undoing its creation can. `config.activate` and `config.preview` take `row`
as a name or an id. `config.row` takes `name` and `from` the same way. A row picked by its id
must exist. A name it doesn't find adds a new row. A number is always an id and text is always a
name, so a row named `"17"` is not row `17`. The table still saves the active row by its name.

Suppression cells accept `true`/`false`, `yes`/`no`, and `1`/`0`, ignoring case and surrounding
whitespace. Creating, copying, or editing a row checks its final suppression values before saving.
An invalid value leaves the table and undo history unchanged; accepted text keeps its spelling.

Body style columns target body names. Assignments remain stored when a row suppresses the
feature that produces that body, and become visible when it returns.

## Link several configurations from one file

`component.insert_design` accepts `configuration` as a row name or row id. Each linked
component can select a different row from the same source file. Select the row through command
parameters; the Insert Design dialog does not yet include a configuration picker. A bolt design
with rows `M6x20` and `M6x30` can supply both sizes in one assembly:

```json
{"command":"component.insert_design","params":{"path":"bolt.sc","name":"Short bolt","configuration":"M6x20"}}
{"command":"component.insert_design","params":{"path":"bolt.sc","name":"Long bolt","configuration":"M6x30","translate":[20,0,0]}}
```

The source file stays unchanged. The assembly stores the selected row's id and creation
identity, so renaming the row preserves the link. Undoing row creation and adding another
row cannot retarget the link, even if the numeric id is reused. Older rows without a creation
identity keep that absence; a newly created row has a different identity.
`component.update_links` reapplies the selected row even when
the source has another row active. Deleting the selected row reports an error and keeps the
last linked bodies; creating a new row with the old name does not retarget the link.
`component.links` includes `configuration` with the stored row id for selected links.

Omit `configuration`, or pass `null`, to follow the source's saved state. Existing links
keep this behavior. With `reuse: true`, insertion shares a linked component only when the
file and selected row match. A link following the saved state does not share a definition
with a link selecting a row explicitly, even if both currently show the same shape.

`component.relink` also accepts `configuration`. Supply `occurrence` to change only one
direct placement when a linked component has several placements. Use the component and
occurrence ids returned by insertion; for example, if they are 7 and 12:

```json
{"command":"component.relink","params":{"component":7,"occurrence":12,"path":"bolt.sc","configuration":"M6x30"}}
```

Relinking the same source, including a symlink alias, without `configuration` preserves the
selected row. If the row was replaced, select the replacement explicitly to accept it.
Relinking a different source without `configuration` follows that file's saved state. Explicit `null` clears the
selection in either case. Save, reopen, undo and redo preserve the selection. The assembly
can still open with its last linked bodies when the source file is unavailable.

These commands select a row for a direct linked component or occurrence. They do not override
an inner linked component separately inside each occurrence of a repeated subassembly.
Edit in Place refuses a link with a selected configuration. Edit that configuration in the
source design, save it, then update the assembly's links.

## Component and face appearances

**Add Column** offers component appearances and a **Face Appearance** submenu organized by
body and semantic role. You can also select a viewport face before opening the menu and use
**Selected face on ...**. These assignments affect every occurrence sharing the definition.
Component columns use stable component IDs, so renaming a component preserves its rows.
Face columns resolve a definition body through certified lineage, never through a saved display face index.

```json
{"command":"config.configure","params":{"component_appearances":["Part"],"face_appearances":[{"body":"Box1","point":[5,5,10]}]}}
```

Use a point on the current surface to capture a face. The menu carries its already certified target directly, avoiding a second nearest-face pick on thin concentric walls. World occurrence picks are converted
to the shared definition coordinates. `config.table` returns canonical keys and readable
target labels. Use its returned face key when editing a row or deleting a face column:
`config.column` accepts `face_appearance: "<returned key>"` with `delete: true`.
Component columns can be removed with `component_appearance: "Part"` and `delete: true`.

Supported stable face roles are box axis faces, cylinder caps and unsplit side, profile-plane
extrude caps, and one-sided revolve caps. From/nonzero or variable Start Offset/Two Sides extrude caps and
symmetric/Two Sides revolve caps are not yet certified. Mesh faces, ambiguous cap names,
generic index-derived names, and sketch side names based on nearest-curve heuristics are
rejected. Untapered New Body extrusions additionally support line, circle and arc sides
with exact source-edge construction certificates. Ordinary face names and saved sketch or Emboss anchors retain their legacy naming; strict styles resolve separate source metadata and also require the original producing feature. All constructed pieces of the same
sketch curve share one appearance, including split circle seams; distinct coincident
curves are ambiguous and cannot be selected. Unsupported downstream modifiers remove
the certificate rather than transferring paint by proximity. Coplanar split pieces of one supported semantic face share
the configured appearance. If that lineage disappears while its body remains, Apply and
Preview reject the row atomically. There is no nearest-face substitution; later ordinary
topology edits also cannot transfer its paint to a replacement face. A suppressed owning
feature keeps the assignment dormant until it returns.

The Library's **Follow inherited appearance** removes only the selected direct component
or face override. Face paint takes priority over body, component, and physical material;
clearing it reveals inherited or imported paint. Body renames maintain face targets.
Deleting a component or originating feature removes its configured targets and row cells.
Deleting one occurrence preserves the shared definition configuration.

Embedded STEP faces use certificates bound to the exact stored payload, source item and
original STEP face entity. Kernel seam pieces of that same source face share the override.
Replacing the embedded payload invalidates the old face target, even when the replacement
has identical geometry or reuses entity numbers. Body renames and exact rigid placements
preserve source identity; ordinary parameter edits rebuild construction certificates.

Independent component copies duplicate mapped configuration columns, row values, and rules
without retargeting the source. An independent copy of the same embedded STEP payload
preserves its source certificate under the new feature. Copying strict paint on a foreign
body that requires re-export still fails explicitly because that exporter cannot yet
certify the source-to-output face mapping.

Activation evaluates all row parameter expressions together, so column order does not create
temporary dependency cycles. Preview and activation reject new or changed geometry errors
without changing the design. An identical existing error remains visible. A saved row cannot
recreate a deleted parameter: remove its configuration references before deleting it. Deleting
a feature removes its input and suppression columns and any rules referring to that feature.

## Ordered rules and preview

Open **Add or edit rule** below the table. Give the rule a name, choose when it applies, then
choose a driven parameter, driven suppression, numeric range, or allowed values. Numeric
fields accept ordinary design expressions and explicit units. **Edit**, the arrow buttons,
the enable checkbox, and **Delete** update existing rules. Choose **All**, **Any**, or **Not**
to build compound conditions with nested controls, **Add condition**, and **Remove condition**.

**Preview Row** rebuilds a scratch design and shows effective expressions and matched or
skipped rules. Driven and restricted values identify the rule responsible. It leaves the
current geometry, active row, and undo history unchanged. **Apply** or **Activate** publishes
the validated variant as one undoable action.

Rules execute in list order. Each condition sees earlier drives; restrictions from matched
rules all apply to the final values. A range includes its endpoints. Allowed values compare
numeric values with matching dimensions, so `1 cm` equals `10 mm`. An angle never equals a
length. Bare expressions use the target parameter's unit, or the document's unit for feature
length inputs. Equality uses a relative/absolute tolerance of `1e-9` in internal units.
The rule editor's **Length units** selector sets the meaning of bare feature-length
expressions; explicit units override it. Rules capture this unit context when authored. Changing document units
preserves mixed expressions such as `width + 1` without treating `width * 2` as a length
multiplier. Bare saved feature length cells and rule thresholds gain the previous explicit
unit; explicit units and nonliteral source expressions retain their text. A matching active
row rederives its driven values when units change. User parameters retain their own units.

Rules recompute on configured edits, **Apply**, and **Preview Row**. An ordinary parameter edit
runs only the rules that switch a feature's suppression and whose condition reads a parameter whose
value changed (or a feature such a rule just switched), in their order, so a switch parameter alone
turns its features on and off; `parameters.change` lists them as `switched`. It does not drive
parameters or enforce restrictions: those, and a matching configuration, wait for an edit of the
configuration or an activation.
Cross-unit drives store effective numeric lengths; their saved source expressions reevaluate
on the next Apply or Preview rather than following later ordinary parameter edits live.
Saved source cells are preserved: disabling a drive restores the row's original choice.

```json
{"command":"config.rule","params":{"rule":{"id":"depth-rule","name":"Large parts use deeper stock","condition":{"type":"parameter","parameter":"width","operator":"ge","expression":"80 mm"},"effect":{"type":"drive_parameter","parameter":"depth","expression":"width / 2"}}}}
{"command":"config.rule","params":{"rule":{"id":"width-range","name":"Stock width","condition":{"type":"always"},"effect":{"type":"parameter_range","parameter":"width","minimum":"10 mm","maximum":"100 mm"}}}}
{"command":"config.preview","params":{"row":"Aluminum blue"}}
{"command":"config.rule","params":{"id":"width-range","index":0}}
```

Configure the `depth` column before adding this example drive. Conditions may reference any
existing design parameter; effect targets must be configured columns. `config.rules` lists
rules. `config.rule` replaces a rule with the same ID, deletes with `id` and `delete: true`,
or moves it to a zero-based `index`. Editing a rule validates every saved row before committing;
`rules: [...]` adds or replaces several rules in one step and validates the rows once, with all
of them in place (rules that only hold together, such as suppressing a body and the cut on it).
Editing a row validates that row. Invalid geometry, dependency cycles, incompatible units,
or restrictions leave the previous document intact and explain the failure. Delete rules
before removing the last row. Parameter renames update rule targets and expressions; rule
references prevent parameter deletion until removed.

Tables support at most 2048 rules; conditions allow eight levels and 64 nodes, and allowed
value lists support up to 128 expressions. These bounds are checked before saved rules are
decoded. Driven appearance/material rules and repeated fixed-point evaluation are outside
the current rule scope.

Newly saved designs use file format 12, including optional durable face attachments and
analytic offset boundary identities and qualified adaptive whole-profile references, including
holes. Older designs upgrade on opening without
changing their existing configuration values. Older SolidCraft versions must be updated to
open newly saved designs.
