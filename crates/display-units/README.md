# solidcraft-display-units

Length display: a value stored in millimetres shown in one of the five design units (`mm`, `cm`,
`m`, `in`, `ft`) with a number of decimal places. Plain functions with no dependencies, so any
application or UI component can show lengths as SolidCraft does without its document model.
`solidcraft-doc` re-exports them from `display_units`.

| Function | What it does |
|---|---|
| `unit_scale(units)` | Units per millimetre and the unit's name; an unknown name is millimetres |
| `length_precision(value, units, decimals)` | Places to show (at most 8); inches keep three, or four where the fourth digit means something |
| `length_number(mm, units, decimals)` | The length in `units`, without a suffix |
| `format_length(mm, units, decimals)` | The length in `units`, with its suffix |

Non-finite values keep Rust's `NaN`, `inf` and `-inf` text; callers that need finite
measurements check them first.
