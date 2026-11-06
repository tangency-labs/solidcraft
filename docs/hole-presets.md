# Personal Hole presets

Open **Hole**, expand **Personal presets**, and choose **Save new**. Enter a name
in the confirmation prompt to store the current hole parameters in this application's personal profile. The presets are
available in every project using that profile and survive restarting the application.

Selecting a different saved name immediately fills the open Hole form. The same
selection is not reapplied every frame, so subsequent manual edits stay intact.
A modified selected preset displays `* ` before its name. The small Save icon to the
right overwrites that same preset without a naming prompt; it is disabled when no
preset is selected or the parameters are unchanged. Reverting the parameters clears
the marker. A successful save clears the marker and disables Save again. Invalid
active values or unrecognized future options report an error without replacing the
stored preset. The marker is display-only and never becomes part of the preset name.

Each newly opened Hole starts with no selected preset. Recall does not create or edit a feature;
the normal Hole **OK/Apply** operation validates and commits it. Cancel closes the
Hole form without changing the model. Saving, renaming or deleting a personal preset
is a preference change, independent of cancelling the Hole operation.

Presets include simple or drilled holes, counterbores, countersinks, tapping and
the extent mode. Dimensions, thread designations, expression text and explicit units
and the independent flat/angled bottom choice are retained without baking them into numeric results. Implicit length units are
written explicitly when saving, including mm; unitless expressions keep their text
inside an explicit unit expression. An expression using a named project parameter is evaluated
in the destination project by the normal Hole operation; the preset does not copy
that project's parameters.

Positions, faces, sketch identifiers, placement mode, target bodies, direction picks,
history and connection data are excluded. **To Object** retains only the extent
choice and requires a fresh destination selection after selecting the preset. Current placement
picks remain in the open form.

Use **Rename** to open a name prompt prefilled with the selected name. Confirm
to rename it; Cancel leaves the library and Hole form unchanged. Empty, overlong
or duplicate names report an error in the prompt. **Delete** asks for a second
confirmation and removes only the personal preset. Neither operation edits existing
holes. Names are unique ignoring case. **Save new** with an existing name reports a conflict
instead of replacing it silently; the adjacent Save icon explicitly updates the selected preset.

The library is limited to 64 entries, names to 96 UTF-8 bytes and each parameter
expression to 512 UTF-8 bytes. Malformed saved rows are ignored without discarding
other valid preferences. Preference-page resets do not erase the library. There is
no design-file format change or dependency on a project, PDM service or connection.

Older drilled presets still imply an angled bottom; older recess presets default to flat.

Presets retain Right/Left thread direction and Full/Partial tapping independently
of drill depth. Partial-depth expressions carry explicit source units, so recalling
an mm preset in an inch design keeps the same physical depth. Older records without
these options retain Right/Full defaults. A recalled preset clears an unfinished
Custom thread draft; saving an invalid active Custom draft reports an error instead
of saving its last valid value. Inactive diameter and full-depth fields do not block
saving a valid tapped preset.

Unrecognized future thread options survive renaming and serialization. Recall
rejects them before changing any Hole field, rather than silently discarding them.
