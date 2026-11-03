# Command illustration coverage

The six covered contexts use original SolidCraft captures in both themes. A command keeps text-only help when a picture would obscure its workflow.

| Context | Illustrated | Text-only |
| --- | ---: | ---: |
| Assemble | 16 | 14 |
| Inspect | 15 | 6 |
| Model | 65 | 8 |
| Sheet Metal | 10 | 6 |
| Sketch | 74 | 22 |
| Surface | 16 | 5 |

The catalog contains **193 unique illustrated commands**. Shared commands reuse their existing artwork.

## Illustration choices

Sketch pictures include creators, constraints, modifications, construction guides and reference curves. Twenty-two native construction animations show the defining picks for lines, circles, rectangles, arcs, slots and splines. Break highlights one resulting split segment; Dimension shows a genuine driving dimension changing the length. The circumscribed polygon retains one tangent incircle. Surface pictures use real open surfaces. Assembly pictures use native placements and checked joint values. Inspect pictures retain meaningful native measurement and analysis annotations. Sheet Metal pictures include the actual flat preview; Perforate is never depicted as a perforated formed B-rep.

Native As-Built joints keep the components in place. Motion couplings are demonstrated by a subsequent test drive. Unstitch uses native separated bodies in an exploded layout for clarity.

## Text-only decisions

### Assemble

| Command | Reason |
| --- | --- |
| `component.capture_position` | Records placement state; no model geometry changes. |
| `component.create` | Creates an empty hierarchy entry rather than visible geometry. |
| `component.from_bodies` | Changes component ownership while preserving visible geometry. |
| `component.insert_design` | A design-file insertion workflow whose result depends on the selected file. |
| `component.revert_position` | Restores saved placement state; the written hint explains which state returns. |
| `contact.check` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.create` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.disable_all` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.edit` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.enable_all` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.enable_sets` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `contact.list` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `occurrence.ground` | Locks placement without changing geometry; the lock icon and text explain it. |
| `parts.library` | A library browsing and insertion workflow with no single representative result. |

### Inspect

| Command | Reason |
| --- | --- |
| `assembly.bom` | A parts-list report rather than a geometric operation. |
| `component.properties` | A property report; a model-only picture would omit the actual information. |
| `contact.check` | Contact-set membership, enablement, or reports are state workflows best explained with text. |
| `document.inspect` | A design report rather than a geometric operation. |
| `timeline.dependents` | A dependency report rather than a geometric operation. |
| `timeline.play` | A history playback workflow; a still model would omit the sequence. |

### Model

| Command | Reason |
| --- | --- |
| `component.finish_edit` | Explains leaving the active linked-component editing context. |
| `config.configure` | Chooses named configuration values; an isolated model picture would obscure the choice. |
| `config.table` | A configuration table workflow, best explained with text. |
| `parameters.change` | A parameter editor workflow; a model-only picture would hide named expressions. |
| `sketch.edit` | Opens an existing sketch; text explains selecting and entering that context. |
| `sketch.point3d` | The point icon and text explain placement; extra editing controls would add noise. |
| `solid.remove` | Direct removal consequences belong in text and confirmation. |
| `timeline.delete` | Deletion and dependent features need written explanation and confirmation. |

### Sheet Metal

| Command | Reason |
| --- | --- |
| `config.configure` | Chooses named configuration values; an isolated model picture would obscure the choice. |
| `config.table` | A configuration table workflow, best explained with text. |
| `parameters.change` | A parameter editor workflow; a model-only picture would hide named expressions. |
| `sheet.export_bend_table` | Explains exporting the bend data. |
| `sheet.export_dxf` | Explains an export and its file contents. |
| `sheet.manage_rules` | A rules editor; thickness and bend settings belong in its written guidance. |

### Sketch

| Command | Reason |
| --- | --- |
| `canvas.insert` | Import and positioning depend on the chosen image; the hint explains that workflow. |
| `config.configure` | Chooses named configuration values; an isolated model picture would obscure the choice. |
| `config.table` | A configuration table workflow, best explained with text. |
| `parameters.change` | A parameter editor workflow; a model-only picture would hide named expressions. |
| `sketch.auto_constrain` | Infers multiple relations and dimensions; a clean model-only still would hide that work. |
| `sketch.auto_constrain_from_datum` | Explains which datum drives inferred dimensions. |
| `sketch.cancel` | Discarding edits needs explicit text about what is restored. |
| `sketch.constrainer` | Chooses a relation from the selected geometry; no single still explains every choice. |
| `sketch.constraint.curvature` | Continuity needs a detailed comb and endpoint discussion, rather than a generic smooth curve. |
| `sketch.constraint.fix` | Locks selected geometry without changing its visible shape. |
| `sketch.constraint.polygon` | Equal sides and the common circumcircle are better specified by the written relation. |
| `sketch.copy` | Clipboard selection behavior needs text rather than a geometry change. |
| `sketch.cut` | Clipboard selection behavior and removal need text. |
| `sketch.equation_curve` | The formulas and parameter range define the workflow; a generic curve would not explain them. |
| `sketch.finish` | Ends the current editing context without a characteristic geometry operation. |
| `sketch.finish_auto_constrain` | Combines constraint inference and leaving the editor; the text explains both. |
| `sketch.infinite` | A line-type toggle whose picked-line behavior is explained by text. |
| `sketch.insert_dxf` | A file-import workflow whose result depends on the file. |
| `sketch.insert_svg` | A file-import workflow whose result depends on the file. |
| `sketch.inspect` | A sketch analysis report; the report is the useful content. |
| `sketch.paste` | Placement of clipboard content depends on the copied selection. |
| `sketch.point` | The existing point icon and placement instruction explain this direct action. |

### Surface

| Command | Reason |
| --- | --- |
| `mesh.convert` | Conversion and body-type choices are better explained in the written hint. |
| `surface.fair` | Small curvature changes need native diagnostic inspection rather than a generic shape comparison. |
| `surface.fillet` | Registered as planned and not implemented; do not imply a working surface fillet. |
| `surface.match` | Continuity at the seam needs a detailed diagnostic view; retain the written selection guidance. |
| `surface.reverse_normal` | A direction property; a plain shaded still would conceal its meaning. |

## Maintenance

Apply the [visual guidelines](command-illustration-guide.md) when adding or
reconsidering an illustration. A simple final outline or unchanged silhouette
is not enough to decide whether a command deserves media: first consider its
defining picks, reference context, native annotations or independently selectable
result.

`assets/command-hints/source/coverage.json` is the explicit per-context decision
list. The media test compares it against the real ribbon registry, including
Sketch family variants, and checks every referenced poster. Native still recipes
stay beside it in `*-menu.json`; `sketch-construction.json` owns the Sketch
animations and their posters. Keep those decisions and sources consistent when
applying new feedback.
