# solidcraft-frame

The title bar's regions for native hit testing. The UI lays the title bar out each frame; the
operating system asks the window procedure what is under the pointer (`WM_NCHITTEST` on
Windows) at any time, on the window's thread, possibly while the UI is in the middle of a
frame. `CaptionMap` carries the answer across that gap without a lock: a sequence lock over
plain atomics, so a reader never blocks and never sees half of one layout and half of another.

It has no dependencies and no clock of its own, so it builds for every target, wasm included,
and is tested deterministically. Coordinates are client-area physical pixels (the window's
top-left is 0,0).

## API

| Item | What it is |
|---|---|
| `PxRect` | A rectangle in client pixels, `min` inclusive and `max` exclusive |
| `CaptionLayout` | One frame's bar: the bar, its drag areas, its controls and the maximize button |
| `CaptionHit` | What a point is: `Drag` (the bare bar, the OS's caption), `MaximizeButton`, `Client` (a control) or `Outside` |
| `CaptionMap::publish(layout)` | The UI's new layout, written without blocking a reader |
| `CaptionMap::hit(x, y)` | What is at a point, for the window procedure's hit test |
| `CaptionMap::bar_and_maximize()` | The bar and the maximize button, for non-client hover tracking |
| `CaptionMap::clear()`, `is_published()` | No layout (the OS frame answers), and whether there is one |
| `MAX_INTERACTIVE`, `MAX_DRAG` | How many control and drag rectangles a layout holds |
