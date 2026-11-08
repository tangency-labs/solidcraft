# solidcraft-ui-registry

The widget registry: UI components name what they draw and record where it was drawn this
frame, so UI tests, scenarios and the control channel can find a widget by name instead of by
pixel. It depends only on `egui` and holds no app types, so any UI component can publish
without depending on the app.

The registry is per thread and is cleared as each frame starts. Lookups search from the end,
so when a name is published more than once in a frame the latest publish wins.

## API

| Function | What it does |
|---|---|
| `publish_handle(name, at)` | A handle (a point to click or drag) drawn this frame |
| `publish_rect(name, rect)` | A painted part's rectangle this frame |
| `publish_target(name, rect)` | Both: the rectangle and a handle at its centre, under one name |
| `publish_target_as(handle, rect_name, rect)` | Both, with separate names for the handle and the rectangle |
| `publish_count(name, value)` | A number about this frame's drawing |
| `handle_at(name)` | Where a handle was drawn last |
| `handles(prefix)` | Every handle whose name starts with `prefix` |
| `rect_of(name)` | A part's rectangle |
| `rects(prefix)` | Every rectangle whose name starts with `prefix` (for checks) |
| `count_of(name)` | A published number |
| `clear_handles()` | Forget every handle, rectangle and count (called as a frame starts) |
