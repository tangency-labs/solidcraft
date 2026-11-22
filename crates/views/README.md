# solidcraft-views

The views of a design as plain data. A design keeps several views (Model, Sheet, Render,
Animation); this crate holds what they are, which one is shown and the saved form. It knows no UI
toolkit, no document and no app, and depends only on the viewport crate for a view's camera, so the commands, the tab strip
and any other front end read one model and draw it their own way.

## API

| Item | What it is |
|---|---|
| `ViewKind` | What a view shows of its design: its stable key (`key`, `from_key`) and the order the "+" menu lists |
| `DesignView` | One view: id, kind, name, and the sheet it shows |
| `ViewSet` | The views of one design in tab order: open (reusing a view of the same subject), activate, close (never the last), close others or to the right, reopen, rename, duplicate, move, the layout the Model views are tiled in (`set_layout`, `show_beside`, `set_split`, `untile`, kept right as views close), and the saved form (`to_json`, `from_json`, safe on hostile input) |
| `Tiles`, `Preset`, `Axis`, `Edge`, `Rect`, `Splitter`, `MAX_TILES` | How a window lays views out side by side: a tree of splits whose leaves are view ids. The presets the Layout menu offers, the rectangles and splitters for a front end to draw, drop-on-an-edge splitting, swap, remove, the splitter ratio, and the saved form (no UI toolkit; at most four views) |
| `MAX_VIEWS`, `MAX_NAME` | The most views a design keeps and the longest view name |

View layouts are remembered in user settings, outside the design document. `from_json` migrates
legacy Sketch tabs by dropping them and their closed-history entries. An active legacy tab falls
back to a surviving Model in the same window, keeping that Model’s camera; if none exists, a new
Model is created. Removed identities stay reserved. Sketch features are untouched, and a sketch
is edited in a Model view rather than represented by a view kind.
