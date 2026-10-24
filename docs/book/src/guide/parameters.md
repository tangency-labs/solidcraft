# Parameters and expressions

Every dimension and feature input is a **model parameter** (`d1`, `d2`, …). **User parameters**
are ones you add yourself, with a unit, an expression and an optional comment. Change them in
**Change Parameters** (`parameters.change`). The timeline rebuilds from the first feature that
uses them.

When creating or editing a sketch dimension, its value editor shows its model parameter
name (`d1`, `d2`, …). Click another driving dimension to use its name: when the value is
just a number (such as `10` or `10 mm`) the name replaces it; otherwise it goes in at the
cursor, replacing selected text and keeping what you typed, so typing `10 + ` and clicking
`d1` gives `10 + d1`. Add arithmetic such as `/ 2` to build a dependent dimension. Press **Enter** to apply
the expression or **Escape** to cancel the edit. When placing a new dimension, `width = 40` adds
the user parameter `width` with that value (a bare number is in the design's units) and
dimensions with it; a name that is already taken is refused.
Add and edit user parameters through the existing **Change Parameters** command.

In the MANAGE tab's PARAMETERS section, click **Parameters** to open the user and model
parameter editor. The **Configurations** button in CONFIGURATIONS opens the configuration table;
**Configure** remains in the dropdown for configuring columns.

Press **Enter** in the new parameter's name, expression or comment field to add it.
Valid entries return focus to the name field for the next parameter; invalid entries remain
editable. In expression fields, type a parameter or dimension name for autocomplete,
or press **Ctrl+Space** to open suggestions, including in an empty field.
**Up/Down** selects an autocomplete suggestion,
**Enter/Tab** accepts it, and **Escape** dismisses the suggestions. Accepting a suggestion
does not also submit the parameter. When what you typed is already a complete name
(`width`), longer names such as `width_extra` stay listed without a selection: **Enter**
applies what you typed, and **Up/Down** picks one of them first. A unit right after a number
(`10 mm`) is left alone; anywhere else a unit is a prefix like any other, so `rad` followed by
**Enter** gives `radius`.

Expressions are unit-aware: `2 * width + 5 mm`, `angle / 2`, `sqrt(area)`, `1 in + 3 mm`, using
`+ - * / ^`, parentheses, `sin cos tan asin acos atan atan2 sqrt abs min max floor ceil round mod
trunc` and unit conversion. A decimal comma works too, outside parentheses: `2,5 mm` is
`2.5 mm` (inside a function's parentheses the comma separates its arguments, so write
`max(2.5, 3)` there). A mixed number is one value, as imperial sizes are written: `2 1/8"` and
`3 7/16 in` are 2.125 in and 3.4375 in (a whole number, a space, then a proper fraction of whole
numbers). A length where an angle is expected (or the other way round) is an
error, not a silent conversion. Cycles are reported with their path (`a → b → a`), renaming a
parameter updates every expression, dimension and feature input that uses it, and a parameter
in use can't be deleted (the error lists its users).

The design's units (Document Settings in the browser: mm, cm, m, in or ft) set how a bare number
typed into a feature is read. In an inch design `2` means 2 in. The Box, Cylinder, Sphere, Torus
and Coil dialogs start with their sizes in the design's units, unit shown (`20 mm`, `20 in`).
Changing the units never moves existing geometry. A size worked out from a bare number keeps
the unit the number was given in: a sphere with a bare diameter of 20 has the radius
`(20 mm) / 2` in a millimetre design. Beside a unit-less expression such as `2 * 5` or a
unit-less parameter, which is read in the design's units, the number stays bare and is read
with it.

Parameters export to and import from CSV or JSON (`parameters.export`, `parameters.import`).

**Linked parameters** let several designs share one set of sizes. Keep the shared sizes as user
parameters in one design, then use **Link Parameters** (`parameters.link_design`) in each design
that needs them: all of the linked design's user parameters come in (give `scope` `favorites` for
its favourite ones only). Linked parameters are used by name in any expression and keep their own
units. They can't be changed, renamed, deleted, imported over or configured in the design that
links them: change them in the linked design, save it, and run **Update Parameter Links**
(`parameters.update_links`) to rebuild with the new values. The update reports each value that
changed, and is refused (changing nothing) when it would bring in a name this design already
uses, or remove a parameter that is still in use. A parameter renamed in the linked design counts
as removed and added, so rename its uses here first, or break the link. Sheet-metal and plastic
rules don't count as uses yet (#4420): check them before an update that removes a parameter.

The design keeps the values it last read, so it opens and rebuilds when the linked design is
missing; opening it warns when the file can't be found or read. A design file that holds one name
twice (from a merge: its own parameter and a linked one, two linked designs with the same name,
or one design linked twice) still opens, uses its own parameter (then the first linked design's),
and says so. Exports (`parameters.export`, CSV or JSON) hold this design's own user parameters
only: linked ones belong to the linked design. **List Parameter Links**
(`parameters.links`) says whether each linked design is up to date, changed since the last update,
missing, unreadable, or outside the folders an agent may use. **Relink Parameters**
(`parameters.relink`) reads the parameters from another file instead, such as the linked design
after it moved; **Break Parameter Link** (`parameters.break_link`) turns the linked parameters
into ordinary user parameters. A design can't link itself, a design that links it in turn, or
the same design twice.

A design file keeps each linked design's path relative to its own folder, so a folder of designs
can move, or sync to another computer, and keep its links. Keep the linked design in the same
folder tree as the designs that use it. A crash report holds the design without its linked
designs, so a replay uses the values last read. In the browser, only designs in its own storage
can be linked.

In the Parameters window, **Link Parameters…** picks the design to link. Linked parameters are
listed under **Linked Parameters**, one group per linked design, with its state and **Update**,
**Relink…** and **Break Link** buttons. Their rows show the name, unit, value and comment but
can't be edited; a linked parameter can still be a favourite.

**Configurations** (`config.*`) keep a table of rows, each with its own parameter values and
suppressed features, physical materials and body appearances, switched with `config.activate`.
The table's Library picker selects material or appearance values. Choose **Follow material**
to clear an appearance override. Custom looks retain their colour and opacity across
activation, undo and save/reopen. See
[configuration commands and examples](https://github.com/bherbruck/solidcraft/blob/main/docs/configurations.md).
