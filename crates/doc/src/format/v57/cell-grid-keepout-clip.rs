//! A `cell_grid` feature can carry a `keepout_clip` (`"whole"`, `"snap"` or `"free"`, absent when it
//! is the same as `clip`): what happens to a cell the edge of a keep-out loop crosses, while `clip`
//! keeps governing the field's outer edges. Older designs have no `keepout_clip`, so nothing in
//! them is rewritten, and a build that does not know the field ignores it (the feature then uses
//! `clip` for every loop).
