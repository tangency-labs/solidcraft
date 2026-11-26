# solidcraft-icon-svg

The toolkit-free parser for SolidCraft's palette-role icon SVG subset, shared by the UI icon
renderer and plugin package checks. It depends only on `tiny-skia` and contains no artwork,
palette, theme state or UI toolkit.

## API

`parse(src, role_names, role_source_hex) -> Result<Doc, String>` parses SVG in a host's colour
roles. The two role slices correspond by index; source colours identify the paint roles used
by the host's renderer. Unsupported elements, path commands, colours and references return an
error.

`Doc` contains the parsed `items`, `masks` and `clips`. `Item` is a shape `Node` or a group of
items with optional clipping and opacity. Each `Node` exposes its tiny-skia path, fill rule,
fill and stroke `Paint`, stroke width and dashes, opacity, and optional mask. `Paint` identifies
no paint, a knockout, the line role, a role index or a fixed mask colour.

The UI kit's `IconSet::parse` wraps this document with its host set; plugin checks call the same
parser with SolidCraft's source palette. Package paths, source byte limits and view boxes are
checked by callers before parsing.

The parser was extracted from `solidcraft-ui-kit` and retains its SolidCraft Brand and Interface
Licence; see `LICENSE`.
