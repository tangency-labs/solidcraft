# WR-383: a range end must be judged from its own endpoint

![The oriented range runs from start zero to original end one. An accurate candidate lies near zero. The legacy guard measures from the start, accepts the candidate and moves the end almost onto the start. The corrected guard measures from the end, rejects the long jump and keeps the original end. Magnified endpoint neighborhoods show the same unchanged 0.002 shift windows.](wr383-endpoint-shift.svg)

The end guard measures a candidate's shift from the end, preserving a range when its local solve returns to the start.

The editable `.vectorcraft` document and live-text SVG were created and exported by the official VectorCraft 0.24.1 headless CLI. No SVG was authored manually, no source build or global install occurred, and no desktop/user document was accessed. All drawing steps are retained in `commands.json`; `provenance.json` records the commands, source measurements, release and file hashes, and inspection. Raw authoring logs are not bundled. Output paths in the archived command data are relative to this directory; original and sanitized JSON hashes are distinguished in the provenance.

The diagram uses the first recorded issue948 endpoint event from `../issue948-root-cause.json`. The full interval and both inset axes use the measured oriented fractions; the insets explicitly magnify their endpoint neighborhoods. These are schematic mechanism coordinates, not a new body mesh or a correctness certificate.

The source and SVG request Helvetica (20 pt bold title, 14 pt labels, 12 pt notes), on a white artboard with the prescribed palette. The SVG contains 31 live text nodes and was exported with `outlineText: false` after inspecting the VectorCraft-rendered PNG. The PNG is 1920×1170.

**Preview font limitation:** Helvetica is absent on this Linux machine. VectorCraft reports that it rasterized the preview in its bundled Source Sans 3; the editable source and live SVG retain Helvetica. No font was installed or relabeled. Re-export the preview on a licensed Helvetica-equipped machine if an exact Helvetica raster is required.
