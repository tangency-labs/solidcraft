# Built-in post processors

One file per controller, embedded in the program (`BUILTIN_FILES` in `src/post.rs`). A post is
data: the `Post` struct in `src/post.rs` as JSON. `cam.post` takes a post by its `name` or one of
its `aliases`, in any case; `description` says what controller it is for and what it writes.
`max_tool` is the highest tool number the controller takes; `min_move` (mm) is the shortest
straight cutting move its planner handles well: the writer counts the moves shorter than it and
`cam.post` reports them as `short_moves`, changing nothing.
Beside each post is `<name>.sample.gcode`, the crate's shared sample program
(`drill_program()` in `src/tests.rs`) posted through it, which the tests hold the file to.

## Adding a controller

1. Copy the closest `<name>.json` to `<controller>.json` and change what the controller needs.
   Write it from the controller's own documentation or source, never from another CAM product's
   post. Say in `description` which firmware revision you checked it against, and how.
2. Add the file to `BUILTIN_FILES` and its name to `Post::BUILTIN` in `src/post.rs`, and to
   `SAMPLES` in `src/tests.rs`.
3. Post the sample: run the tests once, copy the text the failing assertion prints into
   `<controller>.sample.gcode`, and read it line by line against what the controller accepts.
4. Add a test in `src/tests.rs` for anything the controller refuses or needs that the sample does
   not show (a tool number limit, a code it does not parse, an order of blocks).
5. If you can, run a posted program through the controller's parser or simulator and say so in
   the pull request. A real machine run is evidence too; a parser pass is not a machine run.
